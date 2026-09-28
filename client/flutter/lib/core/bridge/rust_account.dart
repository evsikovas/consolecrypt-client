import 'dart:async';
import 'dart:typed_data';

import 'package:consolecrypt/core/bridge/local_auth_channel.dart';
import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/bridge/rust_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/passphrase_strength.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/src/rust/api/account.dart' as rs_account;
import 'package:consolecrypt/src/rust/api/app.dart' as rs_app;
import 'package:consolecrypt/src/rust/api/inventory.dart' as rs_inv;
import 'package:consolecrypt/src/rust/api/profiles.dart' as rs_prof;
import 'package:consolecrypt/src/rust/api/sync.dart' as rs_sync;

/// Copies the secret for the bridge call and wipes the caller's buffer
/// afterwards (Dart zeroisation is best effort; Rust wraps it at once).
Future<T> withSecret<T>(SecretText secret, Future<T> Function(Uint8List bytes) call) async {
  final bytes = secret.exposeBytes();
  try {
    return await call(bytes);
  } finally {
    bytes.fillRange(0, bytes.length, 0);
    secret.wipe();
  }
}

Future<T> withSecrets<T>(SecretText a, SecretText b, Future<T> Function(Uint8List a, Uint8List b) call) =>
    withSecret(a, (x) => withSecret(b, (y) => call(x, y)));

void _checkNewPassphrase(SecretText passphrase) {
  if (!PassphraseStrength.estimate(passphrase.expose()).acceptable) {
    throw const AppException(
      AppErrorCode.validation,
      'Choose a stronger passphrase', // l10n-ignore: diagnostic (UI renders code/reason)
      reason: AppErrorReason.weakPassphrase,
    );
  }
}

// ---- profiles --------------------------------------------------------------------------

final class RustProfileService implements ProfileService {
  RustProfileService(this._hub);

  final RustBackend _hub;

  @override
  Stream<ProfilesState> watchProfiles() => _hub.profiles.stream;

  @override
  ProfilesState get currentProfiles => _hub.profiles.value;

  @override
  Future<Profile> createLocalProfile({required String name}) async {
    final trimmed = name.trim();
    if (trimmed.isEmpty) {
      throw const AppException(
        AppErrorCode.validation,
        'Enter a profile name', // l10n-ignore: diagnostic (UI renders code/reason)
        reason: AppErrorReason.profileNameRequired,
      );
    }
    // app-core creates profile + vault in one step (with the passphrase):
    // until then the profile is a Dart-side draft in onboarding.
    final previous = _hub.activeInfo?['id'] as String?;
    await guard(rs_prof.profilesClose);
    final profile = Profile(
      id: ProfileId.generate(),
      name: trimmed,
      kind: ProfileKind.local,
      createdAt: DateTime.now().toUtc(),
    );
    _hub.startDraft(profile, supersedes: previous);
    await _hub.refreshState();
    return profile;
  }

  @override
  Future<Profile> createSyncedProfile({
    required Uri serverUrl,
    required String email,
    required SecretText password,
    required String deviceName,
    required bool createAccount,
    String? name,
  }) async {
    final display = (name ?? '').trim().isEmpty ? email.trim() : name!.trim();
    final json = await withSecret(
      password,
      (pw) => guard(
        () => rs_prof.profilesCreateSynced(
          displayName: display,
          serverUrl: serverUrl.toString(),
          email: email.trim(),
          password: pw,
          register: createAccount,
        ),
      ),
    );
    final account = decodeObject(json);
    final info = (account['profile']! as Map).cast<String, Object?>();
    final id = info['id']! as String;
    _hub
      ..draftProfile = null
      ..signedOut.remove(id)
      ..userIds[id] = account['user_id'] as String? ?? ''
      ..remoteVaults = [for (final v in account['vaults'] as List? ?? const []) (v as Map).cast<String, Object?>()];
    await _hub.refreshState();
    return profileFromJson(info);
  }

  @override
  Future<void> switchTo(ProfileId id) async {
    if (_hub.draftProfile?.id == id) return;
    _hub.draftProfile = null;
    await guard(() => rs_prof.profilesOpen(profileId: id.value));
    await _hub.refreshState();
  }

  @override
  Future<void> rename(ProfileId id, String name) async {
    final draft = _hub.draftProfile;
    if (draft?.id == id) {
      final trimmed = name.trim();
      if (trimmed.isEmpty) return;
      _hub.draftProfile = Profile(id: draft!.id, name: trimmed, kind: draft.kind, createdAt: draft.createdAt);
      await _hub.refreshState();
      return;
    }
    await guard(() => rs_prof.profilesRename(profileId: id.value, displayName: name));
    await _hub.refreshState();
  }

  @override
  Future<void> delete(ProfileId id) async {
    if (_hub.draftProfile?.id == id) {
      _hub.draftProfile = null;
      await _hub.refreshState();
      return;
    }
    await guard(() => rs_prof.profilesRemove(profileId: id.value));
    await _hub.setKitPending(id.value, pending: false);
    await _hub.storeSet('backup_schedule:${id.value}', null);
    await _hub.storeSet('recent_backups:${id.value}', null);
    await _hub.refreshState();
  }
}

// ---- auth ------------------------------------------------------------------------------

final class RustAuthService implements AuthService {
  RustAuthService(this._hub, this._clientVersion);

  final RustBackend _hub;
  final String _clientVersion;

  @override
  Stream<AuthState> watchAuthState() => _hub.authState.stream;

  @override
  AuthState get currentAuthState => _hub.authState.value;

  @override
  Future<ServerInfo> probeServer(Uri serverUrl) async => serverInfoFromJson(
    decodeObject(await guard(() => rs_app.serverProbe(serverUrl: serverUrl.toString(), clientVersion: _clientVersion))),
  );

  @override
  Future<AccountSession> signIn({required String email, required SecretText password}) async {
    final active = _hub.activeInfo;
    if (active == null || active['kind'] != 'synced') {
      password.wipe();
      throw const AppException(
        AppErrorCode.unsupported,
        'Local profiles have no account', // l10n-ignore: diagnostic (UI renders code/reason)
        reason: AppErrorReason.localProfileNoAccount,
      );
    }
    final expected = (active['email'] as String? ?? '').trim().toLowerCase();
    if (expected.isNotEmpty && expected != email.trim().toLowerCase()) {
      password.wipe();
      throw const AppException(
        AppErrorCode.invalidCredentials,
        'This profile belongs to another account', // l10n-ignore: diagnostic (UI renders code/reason)
        reason: AppErrorReason.accountMismatch,
      );
    }
    await withSecret(password, (pw) async {
      try {
        await guard(() => rs_sync.syncReauthenticate(password: pw, allowNewDeviceIdentity: false));
      } on AppException catch (e) {
        if (e.code != AppErrorCode.deviceRevoked) rethrow;
        // The identity was revoked / taken: register a new one (the device
        // must be approved again for the vault).
        await guard(() => rs_sync.syncReauthenticate(password: pw, allowNewDeviceIdentity: true));
      }
    });
    _hub.signedOut.remove(active['id']);
    await _hub.refreshState();
    final session = _hub.authState.value.session;
    if (session == null) throw const AppException(AppErrorCode.internal, 'Signed in, but no session');
    return session;
  }

  @override
  Future<void> logout() async {
    final id = _hub.activeInfo?['id'] as String?;
    if (id == null) return;
    // Revokes the session on the server (best effort offline) and forgets
    // the tokens in the core; the profile is also locked here.
    await guard(rs_account.accountLogout);
    try {
      await guard(rs_prof.vaultLock);
    } on AppException {
      // Not unlocked.
    }
    _hub.signedOut.add(id);
    await _hub.refreshState();
  }

  @override
  Future<void> requestPasswordReset({required Uri serverUrl, required String email}) =>
      guard(() => rs_account.accountRequestPasswordReset(serverUrl: serverUrl.toString(), email: email.trim()));

  @override
  Future<void> changeAccountPassword({required SecretText current, required SecretText next}) =>
      withSecrets(current, next, (c, n) => guard(() => rs_account.accountChangePassword(current: c, next: n)));
}

// ---- vault -------------------------------------------------------------------------------

final class RustVaultService implements VaultService {
  RustVaultService(this._hub);

  final RustBackend _hub;

  @override
  Stream<VaultStatus> watchStatus() => _hub.vaultStatus.stream;

  @override
  VaultStatus get currentStatus => _hub.vaultStatus.value;

  bool get _joining => _hub.activeInfo?['vault_state'] == 'no_vault' && _hub.remoteVaults.isNotEmpty;

  @override
  Future<RecoveryKit> createVault({required String name, required SecretText passphrase}) async {
    _checkNewPassphrase(passphrase);
    final draft = _hub.draftProfile;
    final active = _hub.activeInfo;
    final RecoveryKit kit;
    final String profileId;
    _hub.creatingVault = true;
    try {
      if (draft != null) {
        final json = await withSecret(
          passphrase,
          (p) => guard(() => rs_prof.profilesCreateLocal(displayName: draft.name, passphrase: p)),
        );
        final created = decodeObject(json);
        profileId = (created['profile']! as Map)['id']! as String;
        kit = recoveryKitFromJson((created['recovery_kit']! as Map).cast<String, Object?>());
        _hub.draftProfile = null;
      } else if (active != null && active['vault_state'] == 'no_vault' && _hub.remoteVaults.isEmpty) {
        final json = await withSecret(passphrase, (p) => guard(() => rs_prof.vaultCreate(passphrase: p)));
        profileId = active['id']! as String;
        kit = recoveryKitFromJson(decodeObject(json));
      } else {
        passphrase.wipe();
        throw const AppException(AppErrorCode.conflict, 'A vault already exists', reason: AppErrorReason.vaultExists);
      }
      await _hub.setKitPending(profileId, pending: true);
    } finally {
      _hub.creatingVault = false;
    }
    await _hub.refreshState();
    await _hub.reloadAll();
    final vaultName = name.trim();
    final settings = _hub.vaultSettings.value;
    if (vaultName.isNotEmpty && settings != null && settings.vaultName != vaultName) {
      await guard(
        () => rs_inv.vaultSettingsSave(
          settingsJson: encodeJson(
            vaultSettingsToJson(settings.copyWith(vaultName: vaultName), base: _hub.rawVaultSettings),
          ),
        ),
      );
      await _hub.reload({'vault_settings'});
    }
    return kit;
  }

  @override
  Future<void> confirmRecoveryKitSaved() async {
    final id = _hub.activeInfo?['id'] as String?;
    if (id == null) return;
    try {
      await guard(rs_prof.vaultAcknowledgeRecoveryKit);
    } on AppException catch (e) {
      // No kit pending in this session (it was shown before a restart).
      if (e.code != AppErrorCode.notFound) rethrow;
    }
    await _hub.setKitPending(id, pending: false);
    await _hub.refreshState();
  }

  @override
  Future<RecoveryKit> regenerateRecoveryKit() async {
    final id = _hub.activeInfo?['id'] as String?;
    final onboarding = id != null && _hub.kitPendingFor(id);
    final kit = recoveryKitFromJson(decodeObject(await guard(rs_prof.vaultRegenerateRecoveryKit)));
    // Settings → "New Recovery Kit" shows the kit in a dialog; onboarding
    // (still pending) runs the 3-word check. The core's pending copy (and
    // its persisted flag) is dropped unless onboarding still needs it.
    if (id != null && !onboarding) {
      try {
        await guard(rs_prof.vaultAcknowledgeRecoveryKit);
      } on AppException {
        // Nothing pending.
      }
    }
    return kit;
  }

  @override
  Future<void> unlockWithPassphrase(SecretText passphrase) async {
    if (_joining) {
      final vaultId = _hub.remoteVaults.first['vault_id']! as String;
      await withSecret(
        passphrase,
        (p) => guard(() => rs_prof.vaultJoinWithPassphrase(vaultId: vaultId, passphrase: p)),
      );
      _hub.remoteVaults = const [];
    } else {
      await withSecret(passphrase, (p) => guard(() => rs_prof.vaultUnlockWithPassphrase(passphrase: p)));
    }
    await _afterUnlock();
  }

  Future<void> _afterUnlock() async {
    _hub
      ..awaitingApproval = false
      ..stopApprovalPoll();
    await _hub.refreshState();
    await _hub.reloadAll();
  }

  @override
  Future<void> unlockWithDevice({required String reason}) async {
    // The OS prompt runs in the UI process (LocalAuthentication via the
    // `consolecrypt/local_auth` channel); on success the core gets a
    // single-use grant and opens the device envelope itself.
    if (!await LocalAuthChannel.instance.authenticate(reason)) {
      throw const AppException(AppErrorCode.osAuthFailed, 'OS authentication failed or was cancelled');
    }
    await guard(rs_prof.vaultUnlockWithDeviceAttested);
    await _afterUnlock();
  }

  @override
  Future<void> refreshDeviceUnlockAvailability() async {
    await LocalAuthChannel.instance.reportToCore();
    await _hub.refreshState();
  }

  @override
  Future<void> setDeviceUnlockEnabled(bool enabled, {required String reason}) async {
    final profileId = _hub.activeProfile?.id;
    if (enabled) {
      await LocalAuthChannel.instance.reportToCore();
      if (!await LocalAuthChannel.instance.authenticate(reason)) {
        throw const AppException(AppErrorCode.osAuthFailed, 'OS authentication failed or was cancelled');
      }
    }
    if (profileId != _hub.activeProfile?.id) {
      throw const AppException(AppErrorCode.cancelled, 'Profile changed during authentication');
    }
    await guard(() => rs_prof.vaultSetDeviceUnlockEnabledAttested(enabled: enabled));
    await _hub.refreshState();
  }

  @override
  Future<void> lock() async {
    await guard(rs_prof.vaultLock);
    await _hub.refreshState();
  }

  @override
  Future<void> requestDeviceApproval() async {
    final active = _hub.activeInfo;
    if (active == null || active['kind'] != 'synced') {
      throw const AppException(
        AppErrorCode.unsupported,
        'Device approval needs a synced profile', // l10n-ignore: diagnostic (UI renders code/reason)
        reason: AppErrorReason.syncedProfileRequired,
      );
    }
    await guard(() => rs_sync.devicesRequestApproval(vaultId: _hub.activeVaultId?.value));
    _hub
      ..awaitingApproval = true
      ..startApprovalPoll();
    await _hub.refreshState();
  }

  @override
  Future<void> cancelDeviceApproval() async {
    _hub
      ..awaitingApproval = false
      ..stopApprovalPoll();
    await _hub.refreshState();
  }

  @override
  Future<void> recoverWithRecoveryKey({required SecretText recoveryInput, required SecretText newPassphrase}) async {
    if (RecoveryInput.parse(recoveryInput.expose()) == null) {
      recoveryInput.wipe();
      newPassphrase.wipe();
      throw const AppException(
        AppErrorCode.invalidRecoveryKey,
        'Enter all 24 words of the Recovery Key or the QR text', // l10n-ignore: diagnostic (UI renders code/reason)
      );
    }
    _checkNewPassphrase(newPassphrase);
    if (_joining) {
      final vaultId = _hub.remoteVaults.first['vault_id']! as String;
      await withSecrets(
        recoveryInput,
        newPassphrase,
        (r, p) => guard(() => rs_prof.vaultJoinWithRecoveryKey(vaultId: vaultId, recoveryInput: r, newPassphrase: p)),
      );
      _hub.remoteVaults = const [];
    } else {
      await withSecrets(
        recoveryInput,
        newPassphrase,
        (r, p) => guard(() => rs_prof.vaultResetPassphraseWithRecoveryKey(recoveryInput: r, next: p)),
      );
    }
    await _afterUnlock();
  }

  @override
  Future<void> resetPassphraseWithTrustedDevice({required SecretText newPassphrase}) async {
    _checkNewPassphrase(newPassphrase);
    // TODO(l10n): the OS prompt reason is English — VaultService has no
    // `reason` parameter here; next: add one (like unlockWithDevice).
    const reason = 'Reset the ConsoleCrypt vault passphrase'; // l10n-ignore: OS prompt reason
    if (!await LocalAuthChannel.instance.authenticate(reason)) {
      newPassphrase.wipe();
      throw const AppException(AppErrorCode.osAuthFailed, 'OS authentication failed or was cancelled');
    }
    await withSecret(newPassphrase, (p) => guard(() => rs_prof.vaultResetPassphraseWithDeviceAttested(next: p)));
    await _afterUnlock();
  }

  @override
  Future<void> changePassphrase({required SecretText current, required SecretText next}) async {
    _checkNewPassphrase(next);
    try {
      await withSecrets(current, next, (c, n) => guard(() => rs_prof.vaultChangePassphrase(current: c, next: n)));
    } on AppException catch (e) {
      if (e.code == AppErrorCode.wrongPassphrase) {
        throw AppException(e.code, e.message, reason: AppErrorReason.currentPassphraseWrong);
      }
      rethrow;
    }
  }
}

// ---- devices ----------------------------------------------------------------------------

final class RustDevicesService implements DevicesService {
  RustDevicesService(this._hub);

  final RustBackend _hub;

  @override
  Stream<DevicesSnapshot> watchDevices() {
    unawaited(_quietRefresh());
    return _hub.devices.stream;
  }

  Future<void> _quietRefresh() async {
    try {
      await _hub.refreshDevices();
    } on AppException {
      // Offline / signed out: the screen shows the last snapshot.
    }
  }

  @override
  Future<void> refresh() => _hub.refreshDevices();

  @override
  Future<VerificationCode> verificationCodeFor(DeviceRequestId requestId) async {
    final pending = decodeObject(await guard(() => rs_sync.devicesStartApproval(requestId: requestId.value)));
    return verificationCodeFrom(pending['verification_code']! as String);
  }

  @override
  Future<VerificationCode> currentDeviceVerificationCode() async =>
      verificationCodeFrom(await guard(rs_sync.devicesOwnCode));

  @override
  Future<void> approve(DeviceRequestId requestId, {required VerificationCode confirmedCode}) async {
    await guard(() => rs_sync.devicesConfirmApproval(requestId: requestId.value, confirmedCode: confirmedCode.display));
    await _quietRefresh();
  }

  @override
  Future<void> reject(DeviceRequestId requestId) async {
    await guard(() => rs_sync.devicesReject(requestId: requestId.value));
    await _quietRefresh();
  }

  @override
  Future<void> revoke(DeviceId deviceId, {String? reason}) async {
    await guard(() => rs_sync.devicesRevoke(deviceId: deviceId.value, reason: reason));
    await _hub.refreshState();
    await _quietRefresh();
  }

  @override
  Future<void> rename(DeviceId deviceId, String name) async {
    await guard(() => rs_sync.devicesRename(deviceId: deviceId.value, name: name));
    await _quietRefresh();
  }
}

// ---- sync ---------------------------------------------------------------------------------

final class RustSyncService implements SyncService {
  RustSyncService(this._hub);

  final RustBackend _hub;

  @override
  Stream<SyncStatus> watchStatus() => _hub.syncStatus.stream;

  @override
  SyncStatus get currentStatus => _hub.syncStatus.value;

  @override
  Future<void> syncNow() async {
    if (_hub.activeInfo?['kind'] != 'synced') return;
    await guard(rs_sync.syncNow);
    await _hub.refreshSyncStatus();
  }

  @override
  Stream<EnableSyncProgress> enableSync({
    required Uri serverUrl,
    required String email,
    required SecretText password,
    required String deviceName,
    required bool createAccount,
  }) {
    final out = StreamController<EnableSyncProgress>();
    void onEvent(Json e) {
      if (e['type'] != 'enable_sync_progress' || out.isClosed) return;
      final step = switch (e['step']) {
        'creating_remote_vault' => EnableSyncStep.creatingRemoteVault,
        'reconnecting' => EnableSyncStep.reconnecting,
        'uploading' => EnableSyncStep.uploading,
        'finishing' => EnableSyncStep.finishing,
        'done' => null, // reported after the profile state is refreshed
        _ => EnableSyncStep.authenticating,
      };
      if (step == null) return;
      out.add(
        EnableSyncProgress(
          step,
          uploaded: (e['uploaded'] as num?)?.toInt() ?? 0,
          total: (e['total'] as num?)?.toInt() ?? 0,
        ),
      );
    }

    Future<void> run() async {
      out.add(const EnableSyncProgress(EnableSyncStep.authenticating));
      _hub.eventListeners.add(onEvent);
      try {
        await withSecret(
          password,
          (pw) => guard(
            () => rs_sync.syncEnable(
              serverUrl: serverUrl.toString(),
              email: email.trim(),
              password: pw,
              register: createAccount,
            ),
          ),
        );
        final id = _hub.activeInfo?['id'] as String?;
        if (id != null) _hub.signedOut.remove(id);
        await _hub.refreshState();
        out.add(const EnableSyncProgress(EnableSyncStep.done));
      } on AppException catch (e) {
        out.add(EnableSyncProgress(EnableSyncStep.failed, message: e.message));
      } finally {
        _hub.eventListeners.remove(onEvent);
        await out.close();
      }
    }

    out.onListen = () => unawaited(run());
    return out.stream;
  }

  @override
  Future<void> disconnect({required bool revokeThisDevice}) async {
    await guard(() => rs_sync.syncDisconnect(revokeDevice: revokeThisDevice));
    await _hub.refreshState();
  }
}
