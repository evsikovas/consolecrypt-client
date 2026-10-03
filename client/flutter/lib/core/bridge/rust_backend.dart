import 'dart:async';

import 'package:consolecrypt/core/bridge/local_auth_channel.dart';
import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/util/value_stream.dart';
import 'package:consolecrypt/src/rust/api/app.dart' as rs_app;
import 'package:consolecrypt/src/rust/api/credentials.dart' as rs_cred;
import 'package:consolecrypt/src/rust/api/inventory.dart' as rs_inv;
import 'package:consolecrypt/src/rust/api/profiles.dart' as rs_prof;
import 'package:consolecrypt/src/rust/api/ssh.dart' as rs_ssh;
import 'package:consolecrypt/src/rust/api/sync.dart' as rs_sync;

/// Shared state of the flutter_rust_bridge backend: profile / vault / auth
/// state derived from app-core, the decrypted inventory mirror (reloaded on
/// `objects_changed` events and after every write), device-local UI state
/// and prompt routing. The service classes in `lib/core/bridge/` are thin
/// adapters over this hub.
final class RustBackend {
  RustBackend(this.info);

  final rs_app.CoreInfo info;

  // Account-level state -----------------------------------------------------------
  final profiles = ValueStreamController<ProfilesState>(ProfilesState.empty);
  final vaultStatus = ValueStreamController<VaultStatus>(VaultStatus.none);
  final authState = ValueStreamController<AuthState>(AuthState.signedOut);
  final syncStatus = ValueStreamController<SyncStatus>(SyncStatus.paused);
  final devices = ValueStreamController<DevicesSnapshot>(DevicesSnapshot.empty);
  final localSettings = ValueStreamController<LocalSettings>(const LocalSettings());

  // Inventory mirror (empty while locked) -------------------------------------------
  final hosts = ValueStreamController<List<Host>>(const []);
  final groups = ValueStreamController<List<Group>>(const []);
  final jumpProfiles = ValueStreamController<List<JumpProfile>>(const []);
  final credentials = ValueStreamController<List<Credential>>(const []);
  final knownHosts = ValueStreamController<List<KnownHost>>(const []);
  final snippets = ValueStreamController<List<Snippet>>(const []);
  final tunnels = ValueStreamController<List<Tunnel>>(const []);
  final aiProviders = ValueStreamController<List<AiProviderConfig>>(const []);
  final vaultSettings = ValueStreamController<VaultSettings?>(null);
  final tunnelRuntime = ValueStreamController<Map<ObjectId, TunnelRuntime>>(const {});

  /// Last DTO per id, so writes keep fields the Dart models do not carry.
  final Map<String, Json> rawHosts = {};
  final Map<String, Json> rawGroups = {};
  final Map<String, Json> rawJumpProfiles = {};
  final Map<String, Json> rawTunnels = {};
  final Map<String, Json> rawSnippets = {};
  final Map<String, Json> rawAiProviders = {};
  Json? rawVaultSettings;

  /// Credential id → owning host id (inline passwords, ADR-0101 §6a).
  final Map<String, String> credentialOwners = {};

  // Derived-state inputs -------------------------------------------------------------
  List<Json> _profileInfos = const [];

  /// Local profile being onboarded: exists only in Dart until the vault is
  /// created (app-core creates profile + vault in one step).
  Profile? draftProfile;

  /// The profile that was open when the draft started (a stale profile list
  /// still showing it active must not end the draft).
  String? _draftSupersedes;

  void startDraft(Profile profile, {String? supersedes}) {
    draftProfile = profile;
    _draftSupersedes = supersedes;
  }

  /// This (new) device asked a trusted device for approval.
  bool awaitingApproval = false;

  /// Vaults of the signed-in account while the synced profile has none.
  List<Json> remoteVaults = const [];

  /// Profiles whose Recovery Kit check was pending in the device-local UI
  /// store of older builds (read-only fallback; the core now persists the
  /// flag as `ProfileInfo.recovery_kit_pending`).
  final Set<String> _legacyKitPending = {};

  /// A vault is being created: the new vault counts as unconfirmed before
  /// its profile id is known (no flash of the unlocked shell).
  bool creatingVault = false;

  /// Synced profiles whose session expired / were signed out in this run.
  final Set<String> signedOut = {};

  /// Account ids learned at sign-in (not part of `ProfileInfo`).
  final Map<String, String> userIds = {};

  /// OS / biometric unlock can be offered for the open, locked profile
  /// (core `device_unlock_info`).
  DeviceUnlockInfo deviceUnlock = const DeviceUnlockInfo();

  /// Routes `PromptRequest`s (terminal host keys / passwords). Returns
  /// `false` when nobody can answer; the prompt is then declined.
  Future<bool> Function(Json prompt)? onPrompt;

  /// Second chance for prompts no terminal claimed (SFTP, tunnels, exec):
  /// the `PromptService` shows them when a presenter is attached.
  Future<bool> Function(Json prompt)? onUnclaimedPrompt;

  /// Every core event (after the hub's own handling), e.g. edit sessions,
  /// backups, enable-sync progress.
  final List<void Function(Json event)> eventListeners = [];

  /// Called after the vault locked or the profile changed.
  final List<void Function()> onSessionReset = [];

  StreamSubscription<String>? _events;
  StreamSubscription<String>? _prompts;
  Timer? _approvalPoll;
  Timer? _runtimePoll;
  Future<void>? _refreshing;
  bool _refreshAgain = false;
  bool _disposed = false;

  static const kitPendingKey = 'kit_pending_profiles';
  static const localSettingsKey = 'local_settings';

  // ---- start-up ---------------------------------------------------------------------

  /// Subscribes to core events and prompts and loads device-local UI state.
  /// The last profile is reopened only when opted in: profile opening can
  /// display an OS keychain prompt. Otherwise the welcome screen offers it.
  Future<void> start() async {
    _events = rs_app.coreEvents().listen((json) => unawaited(_onEvent(json)), onError: (Object _) {});
    _prompts = rs_app.corePrompts().listen((json) => unawaited(_onPrompt(json)), onError: (Object _) {});
    await LocalAuthChannel.instance.reportToCore();
    _legacyKitPending.addAll(await _readStringList(kitPendingKey));
    final settings = await storeGet(localSettingsKey);
    if (settings != null) {
      try {
        localSettings.value = localSettingsFromJson(decodeObject(settings));
      } on FormatException {
        // Corrupt device-local settings: keep defaults.
      }
    }
    await refreshState();
    if (activeInfo == null && localSettings.value.reopenLastProfile) {
      final last = await guard(rs_prof.profilesLastActiveId);
      if (last != null && _profileInfos.any((p) => p['id'] == last)) {
        try {
          await guard(() => rs_prof.profilesOpen(profileId: last));
        } on AppException {
          // e.g. keychain access denied: stay on the welcome screen; the
          // profile switcher still lists it and reports the error on switch.
        }
        await refreshState();
      }
    }
    if (isUnlocked) await reloadAll();
  }

  // ---- derived state ------------------------------------------------------------------

  Json? get activeInfo => _profileInfos.where((p) => p['active'] == true).firstOrNull;

  bool get isUnlocked => draftProfile == null && activeInfo?['vault_state'] == 'unlocked';

  Profile? get activeProfile => profiles.value.active;

  VaultId? get activeVaultId {
    final v = activeInfo?['vault_id'] as String?;
    if (v != null && v.isNotEmpty) return VaultId(v);
    final remote = remoteVaults.firstOrNull?['vault_id'] as String?;
    return remote == null ? null : VaultId(remote);
  }

  /// The Recovery Kit check of [profileId] is still pending (core flag of
  /// the open profile, or a legacy UI-store entry).
  bool kitPendingFor(String profileId) =>
      (activeInfo?['id'] == profileId && activeInfo?['recovery_kit_pending'] == true) ||
      _legacyKitPending.contains(profileId);

  /// The core sets / clears its flag itself (vault creation, regeneration,
  /// acknowledge); this only retires legacy UI-store entries.
  Future<void> setKitPending(String profileId, {required bool pending}) async {
    if (pending || !_legacyKitPending.remove(profileId)) return;
    await storeSet(kitPendingKey, _legacyKitPending.isEmpty ? null : encodeJson(_legacyKitPending.toList()..sort()));
  }

  /// Re-reads the profile list and recomputes profile / vault / auth / sync
  /// state. A call during a running refresh queues one more pass, so the
  /// returned future always reflects state read after the call.
  Future<void> refreshState() {
    final running = _refreshing;
    if (running != null) {
      _refreshAgain = true;
      return running;
    }
    return _refreshing = _refreshLoop();
  }

  Future<void> _refreshLoop() async {
    try {
      do {
        _refreshAgain = false;
        await _refresh();
      } while (_refreshAgain);
    } finally {
      _refreshing = null;
    }
  }

  Future<void> _refresh() async {
    final wasUnlocked = isUnlocked;
    final previousActive = activeInfo?['id'];
    _profileInfos = decodeList(await guard(rs_prof.profilesList));
    final active = activeInfo;
    if (draftProfile != null && active != null && active['id'] != _draftSupersedes) {
      // Another profile became active (vault created, backup restored):
      // the draft is done.
      draftProfile = null;
    }
    if (active?['id'] != previousActive) {
      awaitingApproval = false;
      remoteVaults = const [];
      _stopApprovalPoll();
    }
    deviceUnlock = const DeviceUnlockInfo();
    if (active != null && (active['vault_state'] == 'locked' || active['vault_state'] == 'unlocked')) {
      try {
        deviceUnlock = deviceUnlockFromJson(decodeObject(await guard(rs_prof.vaultDeviceUnlockInfo)));
      } on AppException {
        deviceUnlock = const DeviceUnlockInfo();
      }
    }
    if (active != null && active['kind'] == 'synced' && active['vault_state'] == 'no_vault' && remoteVaults.isEmpty) {
      try {
        remoteVaults = decodeList(await guard(rs_prof.profilesRemoteVaults));
      } on AppException {
        remoteVaults = const [];
      }
    }
    _publishState();
    if (wasUnlocked && !isUnlocked || active?['id'] != previousActive) {
      clearInventory();
      for (final f in onSessionReset) {
        f();
      }
    }
    await refreshSyncStatus();
  }

  void _publishState() {
    final list = [for (final p in _profileInfos) profileFromJson(p)];
    final draft = draftProfile;
    if (draft != null) list.add(draft);
    final active = activeInfo;
    final activeId = draft?.id ?? (active == null ? null : ProfileId(active['id']! as String));
    profiles.value = ProfilesState(profiles: list, activeId: activeId);
    authState.value = _computeAuth();
    vaultStatus.value = _computeVault();
  }

  AuthState _computeAuth() {
    final a = activeInfo;
    if (draftProfile != null || a == null || a['kind'] != 'synced') {
      return AuthState(lastServerUrl: authState.value.session?.serverUrl ?? authState.value.lastServerUrl);
    }
    final url = Uri.tryParse(a['server_url'] as String? ?? '');
    final email = a['email'] as String? ?? '';
    final id = a['id']! as String;
    if (url == null || signedOut.contains(id)) return AuthState(lastServerUrl: url);
    return AuthState(
      session: AccountSession(
        serverUrl: url,
        email: email,
        userId: userIds[id] ?? '',
        deviceId: DeviceId(a['device_id'] as String? ?? ''),
        deviceName: '',
      ),
      lastServerUrl: url,
    );
  }

  VaultStatus _computeVault() {
    if (draftProfile != null) return VaultStatus.none;
    final a = activeInfo;
    if (a == null) return VaultStatus.none;
    final id = a['id']! as String;
    final local = a['kind'] != 'synced';
    final vaultId = activeVaultId;
    final name = vaultSettings.value?.vaultName ?? a['display_name'] as String?;
    switch (a['vault_state']) {
      case 'unlocked':
        return VaultStatus(
          phase: VaultPhase.unlocked,
          vaultId: vaultId,
          vaultName: name,
          recoveryKitConfirmed: !creatingVault && !kitPendingFor(id),
          deviceTrusted: true,
          deviceUnlock: deviceUnlock,
        );
      case 'no_vault':
        if (remoteVaults.isEmpty) return VaultStatus(phase: VaultPhase.none, vaultName: name);
        return VaultStatus(
          phase: awaitingApproval ? VaultPhase.awaitingApproval : VaultPhase.locked,
          vaultId: vaultId,
          vaultName: name,
        );
      default:
        return VaultStatus(
          phase: awaitingApproval ? VaultPhase.awaitingApproval : VaultPhase.locked,
          vaultId: vaultId,
          vaultName: name,
          // A local vault record exists: this device holds an envelope.
          deviceTrusted: local || vaultId != null,
          deviceUnlock: deviceUnlock,
        );
    }
  }

  Future<void> refreshSyncStatus() async {
    final a = activeInfo;
    if (draftProfile != null || a == null || a['kind'] != 'synced') {
      syncStatus.value = SyncStatus.localOnly;
      return;
    }
    try {
      syncStatus.value = syncStatusFromJson(decodeObject(await guard(rs_sync.syncStatus)));
    } on AppException {
      syncStatus.value = SyncStatus.paused;
    }
  }

  // ---- inventory mirror ---------------------------------------------------------------

  void clearInventory() {
    for (final m in [rawHosts, rawGroups, rawJumpProfiles, rawTunnels, rawSnippets, rawAiProviders]) {
      m.clear();
    }
    rawVaultSettings = null;
    credentialOwners.clear();
    hosts.value = const [];
    groups.value = const [];
    jumpProfiles.value = const [];
    credentials.value = const [];
    knownHosts.value = const [];
    snippets.value = const [];
    tunnels.value = const [];
    aiProviders.value = const [];
    vaultSettings.value = null;
    tunnelRuntime.value = const {};
    _runtimePoll?.cancel();
    _runtimePoll = null;
  }

  static const allKinds = {
    'host',
    'group',
    'jump_profile',
    'credential',
    'known_host',
    'snippet',
    'tunnel',
    'ai_provider',
    'vault_settings',
  };

  Future<void> reloadAll() => reload(allKinds);

  /// Re-reads the given object kinds (cc-models `ObjectKind` wire names).
  Future<void> reload(Set<String> kinds) async {
    if (!isUnlocked) return;
    final jobs = <Future<void>>[];
    if (kinds.contains('host') || kinds.contains('rdp_host')) jobs.add(_loadHosts());
    if (kinds.contains('group')) jobs.add(_loadGroups());
    if (kinds.contains('jump_profile')) jobs.add(_loadJumpProfiles());
    if (kinds.contains('credential') || kinds.contains('secret')) jobs.add(_loadCredentials());
    if (kinds.contains('known_host')) jobs.add(_loadKnownHosts());
    if (kinds.contains('snippet')) jobs.add(_loadSnippets());
    if (kinds.contains('tunnel')) jobs.add(_loadTunnels());
    if (kinds.contains('ai_provider')) jobs.add(_loadAiProviders());
    if (kinds.contains('vault_settings')) jobs.add(_loadVaultSettings());
    try {
      await Future.wait(jobs);
    } on AppException catch (e) {
      // Locked meanwhile: the lock event clears the mirror.
      if (e.reason != AppErrorReason.vaultLocked && e.reason != AppErrorReason.noActiveProfile) rethrow;
    }
  }

  Future<void> _loadHosts() async {
    final list = decodeList(await guard(rs_inv.hostsList));
    rawHosts
      ..clear()
      ..addEntries(list.map((j) => MapEntry(j['id']! as String, j)));
    hosts.value = List.unmodifiable(list.map(hostFromJson));
  }

  Future<void> _loadGroups() async {
    final list = decodeList(await guard(rs_inv.groupsList));
    rawGroups
      ..clear()
      ..addEntries(list.map((j) => MapEntry(j['id']! as String, j)));
    groups.value = List.unmodifiable(list.map(groupFromJson));
  }

  Future<void> _loadJumpProfiles() async {
    final list = decodeList(await guard(rs_inv.jumpProfilesList));
    rawJumpProfiles
      ..clear()
      ..addEntries(list.map((j) => MapEntry(j['id']! as String, j)));
    jumpProfiles.value = List.unmodifiable(list.map(jumpProfileFromJson));
  }

  Future<void> _loadCredentials() async {
    final list = decodeList(await guard(rs_cred.credentialsList));
    credentialOwners
      ..clear()
      ..addEntries([
        for (final j in list)
          if (j['owner_host_id'] case final String owner) MapEntry(j['id']! as String, owner),
      ]);
    credentials.value = List.unmodifiable(list.map(credentialFromJson));
  }

  Future<void> _loadKnownHosts() async {
    knownHosts.value = List.unmodifiable(decodeList(await guard(rs_inv.knownHostsList)).map(knownHostFromJson));
  }

  Future<void> _loadSnippets() async {
    final list = decodeList(await guard(rs_inv.snippetsList));
    rawSnippets
      ..clear()
      ..addEntries(list.map((j) => MapEntry(j['id']! as String, j)));
    snippets.value = List.unmodifiable(list.map(snippetFromJson));
  }

  Future<void> _loadTunnels() async {
    final list = decodeList(await guard(rs_inv.tunnelsList));
    rawTunnels
      ..clear()
      ..addEntries(list.map((j) => MapEntry(j['id']! as String, j)));
    tunnels.value = List.unmodifiable(list.map(tunnelFromJson));
    await refreshTunnelRuntime();
  }

  Future<void> _loadAiProviders() async {
    final list = decodeList(await guard(rs_inv.aiProvidersList));
    rawAiProviders
      ..clear()
      ..addEntries(list.map((j) => MapEntry(j['id']! as String, j)));
    aiProviders.value = List.unmodifiable(list.map(aiProviderFromJson));
  }

  Future<void> _loadVaultSettings() async {
    final j = decodeObject(await guard(rs_inv.vaultSettingsGet));
    rawVaultSettings = j;
    vaultSettings.value = vaultSettingsFromJson(j);
    vaultStatus.value = _computeVault();
  }

  /// Runtime of running / failed tunnels; polled every 3 s while any runs
  /// (byte counters).
  Future<void> refreshTunnelRuntime() async {
    if (!isUnlocked) {
      tunnelRuntime.value = const {};
      return;
    }
    final list = decodeList(await guard(rs_ssh.tunnelStatuses));
    final runtime = {for (final j in list) ObjectId(j['id']! as String): tunnelRuntimeFromJson(j)};
    tunnelRuntime.value = Map.unmodifiable(runtime);
    final anyRunning = runtime.values.any((r) => r.state == TunnelRunState.running);
    if (anyRunning && _runtimePoll == null && !_disposed) {
      _runtimePoll = Timer.periodic(const Duration(seconds: 3), (_) => unawaited(_pollRuntime()));
    } else if (!anyRunning) {
      _runtimePoll?.cancel();
      _runtimePoll = null;
    }
  }

  Future<void> _pollRuntime() async {
    try {
      await refreshTunnelRuntime();
    } on AppException {
      // Transient (locking): the next event refreshes.
    }
  }

  /// Devices + pending requests (synced profiles only).
  Future<void> refreshDevices() async {
    final a = activeInfo;
    if (draftProfile != null || a == null || a['kind'] != 'synced') {
      devices.value = DevicesSnapshot.empty;
      return;
    }
    devices.value = devicesFromJson(decodeObject(await guard(rs_sync.devicesList)), vaultId: activeVaultId);
  }

  // ---- device approval (new device) -------------------------------------------------

  void startApprovalPoll() {
    _approvalPoll?.cancel();
    _approvalPoll = Timer.periodic(const Duration(seconds: 5), (_) => unawaited(tryFinishApproval()));
  }

  void _stopApprovalPoll() {
    _approvalPoll?.cancel();
    _approvalPoll = null;
  }

  void stopApprovalPoll() => _stopApprovalPoll();

  /// `true` once this device was approved and the vault is open.
  Future<bool> tryFinishApproval() async {
    if (!awaitingApproval) return false;
    try {
      final done = await guard(() => rs_sync.devicesFinishApproval(vaultId: activeVaultId?.value));
      if (!done) return false;
    } on AppException {
      return false;
    }
    awaitingApproval = false;
    _stopApprovalPoll();
    remoteVaults = const [];
    await refreshState();
    await reloadAll();
    return true;
  }

  // ---- events & prompts ---------------------------------------------------------------

  Future<void> _onEvent(String json) async {
    if (_disposed) return;
    final Json e;
    try {
      e = decodeObject(json);
    } on FormatException {
      return;
    }
    try {
      switch (e['type']) {
        case 'profiles_changed' || 'recovery_changed':
          await refreshState();
        case 'vault_unlocked':
          await refreshState();
          await reloadAll();
        case 'vault_locked':
          await refreshState();
        case 'objects_changed':
          await reload({e['kind']! as String});
        case 'conflict_resolved' || 'integrity_warning' || 'lagged':
          await refreshState();
          await reloadAll();
        case 'sync_status':
          if (activeInfo?['kind'] == 'synced') syncStatus.value = syncStatusFromJson(e);
        case 'sync_stopped':
          await refreshSyncStatus();
        case 'reauth_required':
          final id = activeInfo?['id'] as String?;
          if (id != null) signedOut.add(id);
          _publishState();
        case 'device_revoked':
          await refreshState();
          await _refreshDevicesQuietly();
        case 'device_approval_requested':
          await _refreshDevicesQuietly();
        case 'device_approved':
          if (e['is_self'] == true) {
            await tryFinishApproval();
          } else {
            await _refreshDevicesQuietly();
          }
        case 'tunnel_started' || 'tunnel_stopped' || 'tunnel_failed':
          await refreshTunnelRuntime();
        case 'signed_out':
          final id = e['profile_id'] as String?;
          if (id != null) signedOut.add(id);
          _publishState();
      }
    } on AppException {
      // Event-driven refreshes are best effort; the next event retries.
    }
    for (final listener in [...eventListeners]) {
      try {
        listener(e);
      } on Object {
        // A failing listener must not stop the others.
      }
    }
  }

  Future<void> _refreshDevicesQuietly() async {
    try {
      await refreshDevices();
    } on AppException {
      // Offline: keep the last snapshot.
    }
  }

  Future<void> _onPrompt(String json) async {
    final Json p;
    try {
      p = decodeObject(json);
    } on FormatException {
      return;
    }
    final handled =
        await (onPrompt?.call(p) ?? Future.value(false)) || await (onUnclaimedPrompt?.call(p) ?? Future.value(false));
    if (handled) return;
    // Nobody can answer (no terminal connecting, no prompt presenter
    // mounted): decline at once instead of letting the core wait for its
    // timeout.
    try {
      if (p['HostKey'] case final Map<Object?, Object?> hk) {
        await rs_app.promptAnswerHostKey(requestId: hk['request_id']! as String, answer: rs_app.HostKeyAnswer.reject);
      } else if (p['Password'] case final Map<Object?, Object?> pw) {
        await rs_app.promptAnswerPassword(requestId: pw['request_id']! as String);
      } else if (p['Passphrase'] case final Map<Object?, Object?> pp) {
        await rs_app.promptAnswerPassphrase(requestId: pp['request_id']! as String);
      }
    } on Object {
      // Already answered or timed out.
    }
  }

  // ---- device-local UI store ------------------------------------------------------------

  Future<String?> storeGet(String key) => guard(() => rs_app.uiStoreGet(key: key));

  Future<void> storeSet(String key, String? value) => guard(() => rs_app.uiStoreSet(key: key, value: value));

  Future<List<String>> _readStringList(String key) async {
    final raw = await storeGet(key);
    if (raw == null) return const [];
    try {
      return decodeStringList(raw);
    } on FormatException {
      return const [];
    }
  }

  // ---- shutdown -------------------------------------------------------------------------

  /// Locks and closes the profile (SQLCipher closed synchronously) — call
  /// before the process exits.
  Future<void> shutdown() async {
    _stopApprovalPoll();
    _runtimePoll?.cancel();
    try {
      await rs_app.coreShutdown();
    } on Object {
      // Exiting anyway.
    }
  }

  Future<void> dispose() async {
    if (_disposed) return;
    _disposed = true;
    _stopApprovalPoll();
    _runtimePoll?.cancel();
    await _events?.cancel();
    await _prompts?.cancel();
    for (final c in <ValueStreamController<Object?>>[
      profiles,
      vaultStatus,
      authState,
      syncStatus,
      devices,
      localSettings,
      hosts,
      groups,
      jumpProfiles,
      credentials,
      knownHosts,
      snippets,
      tunnels,
      aiProviders,
      vaultSettings,
      tunnelRuntime,
    ]) {
      await c.close();
    }
  }
}
