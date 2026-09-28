import 'dart:async';
import 'dart:convert';
import 'dart:math';

import 'package:consolecrypt/core/mock/fake_data.dart';
import 'package:consolecrypt/core/mock/mock_config.dart';
import 'package:consolecrypt/core/mock/mock_wordlist.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/util/value_stream.dart';

// MOCK ONLY. This file simulates the self-hosted server, local profile
// databases and vault key material entirely in memory. It keeps passphrases
// and Recovery Keys in plain Dart strings to *simulate* envelope checks —
// the real implementation never stores either (VRK lives in zeroised Rust
// memory; only envelopes are persisted). Never reuse this for real data.

/// Simulated key material of one vault.
final class MockVaultKeys {
  MockVaultKeys({
    required this.vaultId,
    required this.name,
    required this.passphrase,
    required this.words,
    required this.qrPayload,
  });

  final VaultId vaultId;
  String name;
  String passphrase;
  List<String> words;
  String qrPayload;

  MockVaultKeys copy() =>
      MockVaultKeys(vaultId: vaultId, name: name, passphrase: passphrase, words: List.of(words), qrPayload: qrPayload);

  bool matchesRecovery(String input) {
    final text = input.trim();
    if (text.startsWith(RecoveryInput.qrPrefix)) return text == qrPayload;
    final typed = text
        .toLowerCase()
        .split(RegExp(r'[\s,;]+'))
        .map((w) => w.replaceFirst(RegExp(r'^\d+[.)]'), ''))
        .where((w) => w.isNotEmpty)
        .toList();
    return typed.join(' ') == words.join(' ');
  }
}

/// Server-side account.
final class MockAccount {
  MockAccount({required this.email, required this.password, this.trustedOnLogin = false});

  final String email;
  String password;
  final String userId = generateUuidV4();

  /// Vault uploaded to the server (envelopes + objects).
  MockVaultKeys? vault;

  /// Demo shortcut: whether this installation is already a trusted device
  /// when signing into this account (simulates a previous session).
  final bool trustedOnLogin;
}

/// Decrypted working set of one vault (what app-core holds while unlocked).
final class MockVaultData {
  MockVaultData.empty(this.vaultId, String vaultName)
    : groups = [],
      hosts = [],
      credentials = [],
      jumpProfiles = [],
      knownHosts = [],
      changedKeyPatterns = {},
      tunnels = [],
      snippets = [],
      providers = [],
      secrets = {},
      passphrases = {},
      settings = _settings(vaultName);

  factory MockVaultData.demo(VaultId vaultId, String vaultName) {
    final inv = FakeInventory.build();
    final data = MockVaultData.empty(vaultId, vaultName)
      ..groups.addAll(inv.groups)
      ..hosts.addAll(inv.hosts)
      ..credentials.addAll(inv.credentials)
      ..jumpProfiles.addAll(inv.jumpProfiles)
      ..knownHosts.addAll(inv.knownHosts)
      ..changedKeyPatterns.addAll(inv.changedKeyPatterns)
      ..tunnels.addAll(inv.tunnels)
      ..snippets.addAll(buildFakeSnippets())
      ..providers.addAll(buildFakeProviders());
    for (final c in inv.credentials) {
      if (c.secretId != null) {
        // Generated at runtime: no secrets in git (AGENTS.md).
        data.secrets[c.id] = SecretText(
          c.kind == CredentialKind.password ? 'mock-${fakeBase64(9)}' : fakePrivateKeyPem(),
        );
      }
    }
    return data;
  }

  final VaultId vaultId;
  final List<Group> groups;
  final List<Host> hosts;
  final List<Credential> credentials;
  final List<JumpProfile> jumpProfiles;
  final List<KnownHost> knownHosts;
  final Set<String> changedKeyPatterns;
  final List<Tunnel> tunnels;
  final List<Snippet> snippets;
  final List<AiProviderConfig> providers;

  /// credential id → secret material (mock of the Secret objects).
  final Map<ObjectId, SecretText> secrets;

  /// credential id → remembered SSH key passphrase (separate Secret).
  final Map<ObjectId, SecretText> passphrases;
  VaultSettings settings;

  int get objectCount =>
      groups.length +
      hosts.length +
      credentials.length +
      secrets.length +
      jumpProfiles.length +
      knownHosts.length +
      tunnels.length +
      snippets.length +
      providers.length +
      1;

  static VaultSettings _settings(String name) {
    final now = DateTime.now().toUtc();
    return VaultSettings(id: ObjectId.generate(), vaultName: name, createdAt: now, updatedAt: now);
  }
}

/// Local state of one profile on this installation.
final class MockProfileRecord {
  MockProfileRecord(this.profile);

  Profile profile;
  MockVaultKeys? vault;
  VaultPhase phase = VaultPhase.none;
  bool deviceTrusted = false;
  bool deviceUnlockEnabled = false;
  bool recoveryKitConfirmed = true;
  bool sessionActive = false;
  String deviceName = 'This device';

  // Sync engine (synced profiles).
  int pendingChanges = 0;
  DateTime? lastSyncAt;
  int serverSequence = 0;
  bool syncing = false;
  final List<SyncIssue> issues = [];

  // Backups.
  BackupSchedule schedule = const BackupSchedule();
  final List<BackupInfo> backups = [];
}

/// A backup file "on disk" (mock file system).
final class MockBackupFile {
  MockBackupFile({required this.info, required this.keys});

  final BackupInfo info;
  final MockVaultKeys keys;
}

final class MockCloud {
  MockCloud(this.config) {
    if (config.seedDemoData) _seed();
  }

  final MockConfig config;
  final Random _rng = Random.secure();

  final Map<String, MockAccount> accounts = {};
  final Map<ProfileId, MockProfileRecord> records = {};
  final Map<VaultId, MockVaultData> _vaultData = {};
  final Set<VaultId> _demoVaults = {};
  final Map<String, MockBackupFile> backupFiles = {};

  final ValueStreamController<ProfilesState> profiles = ValueStreamController(ProfilesState.empty);
  final StreamController<void> _changed = StreamController<void>.broadcast(sync: true);
  final StreamController<void> _mutations = StreamController<void>.broadcast();

  bool offline = false;

  static const demoEmail = 'demo@consolecrypt.test';
  static const newDeviceEmail = 'newdevice@consolecrypt.test';
  static const demoPassword = 'demo-password-1';
  static const demoPassphrase = 'correct horse battery staple';
  static const demoBackupPath = '/Users/demo/Backups/Personal-2026-09-20.ccbackup';

  /// Fixed so demos and tests can exercise Recovery Key restore.
  static const demoRecoveryWords = [
    'anchor',
    'bamboo',
    'canvas',
    'august',
    'breeze',
    'amazing',
    'bicycle',
    'castle',
    'alpha',
    'brave',
    'autumn',
    'bridge',
    'cactus',
    'april',
    'blossom',
    'artist',
    'bronze',
    'cabin',
    'arena',
    'buddy',
    'athlete',
    'broccoli',
    'actress',
    'balcony',
  ];

  /// Any state change (profile switch, lock, sign-in, sync state …).
  /// Synchronous so every mock service observes a consistent world.
  Stream<void> get changed => _changed.stream;

  void notifyChanged() {
    if (!_changed.isClosed) _changed.add(null);
  }

  /// Local vault data changed (goes to the outbox of synced profiles).
  Stream<void> get mutations => _mutations.stream;

  void recordMutation() {
    if (!_mutations.isClosed) _mutations.add(null);
  }

  MockProfileRecord? get activeRecord {
    final id = profiles.value.activeId;
    return id == null ? null : records[id];
  }

  /// Decrypted working set, only while the active vault is unlocked.
  MockVaultData? get activeData {
    final record = activeRecord;
    final keys = record?.vault;
    if (record == null || keys == null || record.phase != VaultPhase.unlocked) return null;
    return dataFor(keys.vaultId, keys.name);
  }

  MockVaultData dataFor(VaultId id, String name) => _vaultData.putIfAbsent(
    id,
    () => _demoVaults.contains(id) ? MockVaultData.demo(id, name) : MockVaultData.empty(id, name),
  );

  void publishProfiles({ProfileId? activeId, bool clearActive = false}) {
    profiles.value = ProfilesState(
      profiles: [for (final r in records.values) r.profile],
      activeId: clearActive ? null : (activeId ?? profiles.value.activeId),
    );
    notifyChanged();
  }

  /// Locks the active vault (profile switch, sign-out).
  void lockActive() {
    final record = activeRecord;
    if (record != null && record.phase == VaultPhase.unlocked) {
      record.phase = VaultPhase.locked;
    }
  }

  MockVaultKeys newVaultKeys(String name, String passphrase, {VaultId? id}) {
    final vaultId = id ?? VaultId.generate();
    return MockVaultKeys(
      vaultId: vaultId,
      name: name,
      passphrase: passphrase,
      words: List.generate(RecoveryKit.wordCount, (_) => mockRecoveryWords[_rng.nextInt(mockRecoveryWords.length)]),
      qrPayload: generateQrPayload(vaultId),
    );
  }

  String generateQrPayload(VaultId vaultId) {
    final key = List<int>.generate(32, (_) => _rng.nextInt(256));
    final encoded = base64Url.encode(key).replaceAll('=', '');
    return '${RecoveryInput.qrPrefix}${vaultId.value}:$encoded';
  }

  RecoveryKit kitFor(MockVaultKeys keys, Uri? serverUrl) => RecoveryKit(
    vaultId: keys.vaultId,
    words: keys.words,
    qrPayload: keys.qrPayload,
    serverUrl: serverUrl,
    createdAt: DateTime.now().toUtc(),
  );

  void _seed() {
    final demoVaultId = VaultId.generate();
    final demoVault = MockVaultKeys(
      vaultId: demoVaultId,
      name: 'Personal',
      passphrase: demoPassphrase,
      words: demoRecoveryWords,
      qrPayload: generateQrPayload(demoVaultId),
    );
    _demoVaults.add(demoVault.vaultId);
    accounts[demoEmail] = MockAccount(email: demoEmail, password: demoPassword, trustedOnLogin: true)
      ..vault = demoVault;
    accounts[newDeviceEmail] = MockAccount(email: newDeviceEmail, password: demoPassword)..vault = demoVault;
    final now = DateTime.now().toUtc();
    backupFiles[demoBackupPath] = MockBackupFile(
      keys: demoVault.copy(),
      info: BackupInfo(
        path: demoBackupPath,
        vaultId: demoVault.vaultId,
        createdAt: now.subtract(const Duration(days: 6)),
        objectCount: 64,
        sizeBytes: 96 * 1024,
        formatVersion: 1,
        appVersion: '0.1.0',
      ),
    );
  }

  Future<void> dispose() async {
    await _changed.close();
    await _mutations.close();
    await profiles.close();
  }
}

String fakePrivateKeyPem() =>
    '-----BEGIN OPENSSH PRIVATE KEY-----\n${fakeBase64(180)}\n-----END OPENSSH PRIVATE KEY-----\n';
