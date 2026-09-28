// Drives the real Rust core (client/rust/bridge → app-core) through
// RustAppServices — the same code path the app uses, with an isolated data
// directory and keychain service.
//
//   flutter test integration_test -d macos
//
// `--dart-define=CC_IT_MEMORY_STORE=true` keeps keys in memory (machines
// without a usable login keychain); the restart step is skipped then.
import 'dart:io';

import 'package:consolecrypt/core/bridge/rust_app_services.dart';
import 'package:consolecrypt/core/bridge/rust_backup.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';

const _memoryStore = bool.fromEnvironment('CC_IT_MEMORY_STORE');

// Generated per run; never a real credential.
final _passphrase = 'it-${DateTime.now().microsecondsSinceEpoch}-Violet-Anchor-Muffin-Glacier';
final _hostPassword = 'host-${DateTime.now().microsecondsSinceEpoch}';

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  late Directory root;
  late RustCoreOptions options;
  RustAppServices? rust;

  setUpAll(() {
    root = Directory.systemTemp.createTempSync('cc-bridge-it-');
    options = RustCoreOptions(
      dataDir: '${root.path}/data',
      keychainService: 'io.consolecrypt.ConsoleCrypt.it-${DateTime.now().millisecondsSinceEpoch}',
      inMemorySecureStore: _memoryStore,
      fastKdfForTests: true,
      backgroundSync: false,
      autoStartTunnels: false,
      hookAppExit: false,
    );
  });

  tearDownAll(() async {
    // Removing the profiles also deletes their keychain items.
    final s = rust?.services ?? (await RustAppServices.open(options)).services;
    for (final p in s.profiles.currentProfiles.profiles) {
      await s.profiles.delete(p.id);
    }
    await s.dispose();
    root.deleteSync(recursive: true);
  });

  if (Platform.isAndroid) {
    testWidgets('Android initializes private storage and rejects exports outside it', (tester) async {
      const channel = MethodChannel('consolecrypt/android');
      final directory = await channel.invokeMethod<String>('dataDirectory');
      expect(directory, contains('/no_backup/consolecrypt'));
      expect(Directory(directory!).existsSync(), isTrue);
      const dialogs = NativeFileDialogService();
      final staging = await dialogs.chooseSaveFile(suggestedName: '../example.txt');
      expect(staging, startsWith('$directory/exports/'));
      expect(File(staging!).parent.existsSync(), isTrue);
      expect(File(staging).uri.pathSegments.last, '.._example.txt');
      await File(staging).parent.delete();
      await expectLater(
        channel.invokeMethod<bool>('exportFile', {'path': '${root.path}/outside.txt'}),
        throwsA(isA<PlatformException>().having((e) => e.code, 'code', 'export_path')),
      );
    });
  }

  testWidgets('local profile → host with inline password → lock/unlock → restart → backup', (tester) async {
    rust = await RustAppServices.open(options);
    var s = rust!.services;
    expect(s.isMock, isFalse);
    expect(s.profiles.currentProfiles.profiles, isEmpty);
    expect(s.vault.currentStatus.phase, VaultPhase.none);

    // Onboarding: profile draft → vault → Recovery Kit → confirmation.
    final draft = await s.profiles.createLocalProfile(name: 'IT Personal');
    expect(s.profiles.currentProfiles.activeId, draft.id);
    expect(s.vault.currentStatus.phase, VaultPhase.none);
    final kit = await s.vault.createVault(name: 'IT Vault', passphrase: SecretText(_passphrase));
    expect(kit.exposeWords(), hasLength(RecoveryKit.wordCount));
    expect(s.vault.currentStatus.phase, VaultPhase.unlocked);
    expect(s.vault.currentStatus.recoveryKitConfirmed, isFalse);
    await s.vault.confirmRecoveryKitSaved();
    expect(s.vault.currentStatus.recoveryKitConfirmed, isTrue);
    final profile = s.profiles.currentProfiles.active!;
    expect(profile.isLocal, isTrue);
    expect(profile.name, 'IT Personal');
    expect(s.vault.currentStatus.vaultName, 'IT Vault');

    // Host with its own (inline) password credential.
    final saved = await s.inventory.saveHostWithAuth(
      Host.create(name: 'db-it', address: '10.0.0.5'),
      HostAuthInlinePassword(password: SecretText(_hostPassword)),
    );
    expect(saved.credentialId, isNotNull);
    expect(saved.inlineCredentialId, saved.credentialId);
    expect((await s.inventory.watchHosts().first).map((h) => h.name), contains('db-it'));
    final inline = (await s.inventory.watchCredentials().first).singleWhere((c) => c.id == saved.credentialId);
    expect(inline.kind, CredentialKind.password);
    expect(inline.secretId, isNotNull, reason: 'the secret is stored (reported as a flag only)');

    // Lock → nothing readable; wrong passphrase refused; unlock → intact.
    await s.vault.lock();
    expect(s.vault.currentStatus.phase, VaultPhase.locked);
    expect(await s.inventory.watchHosts().first, isEmpty);
    await expectLater(
      s.vault.unlockWithPassphrase(SecretText('definitely-not-the-passphrase')),
      throwsA(isA<AppException>().having((e) => e.code, 'code', AppErrorCode.wrongPassphrase)),
    );
    await s.vault.unlockWithPassphrase(SecretText(_passphrase));
    expect(s.vault.currentStatus.phase, VaultPhase.unlocked);
    expect((await s.inventory.watchHosts().first).single.id, saved.id);

    // Device-local settings persist in the core's data dir.
    await s.settings.updateLocal(s.settings.currentLocal.copyWith(appLocale: AppLocale.ru, reopenLastProfile: true));

    if (!_memoryStore) {
      // Restart: the profile reopens locked (Vault Unlock, not onboarding)
      // and the data is intact (SQLCipher key from the OS keychain).
      await rust!.close();
      rust = await RustAppServices.open(options);
      s = rust!.services;
      expect(s.profiles.currentProfiles.activeId, profile.id);
      expect(s.vault.currentStatus.phase, VaultPhase.locked);
      expect(s.settings.currentLocal.appLocale, AppLocale.ru);
      await s.vault.unlockWithPassphrase(SecretText(_passphrase));
      expect((await s.inventory.watchHosts().first).single.name, 'db-it');
      expect(s.vault.currentStatus.recoveryKitConfirmed, isTrue);
    }

    // Encrypted backup → inspect → restore into a new local profile.
    final path = '${root.path}/it-backup.$backupFileExtension';
    final exported = await s.backups.exportBackup(path: path);
    expect(File(path).existsSync(), isTrue);
    expect(exported.objectCount, greaterThanOrEqualTo(3), reason: 'host + credential + secret (+ settings)');
    final header = await s.backups.inspectBackup(path);
    expect(header.vaultId, exported.vaultId);
    expect((await s.backups.watchRecentBackups().first).map((b) => b.path), contains(path));
    final restored = await s.backups.restoreBackup(
      path: path,
      unlock: BackupUnlockWithPassphrase(SecretText(_passphrase)),
      profileName: 'IT Restored',
    );
    expect(s.profiles.currentProfiles.activeId, restored.id);
    expect(s.vault.currentStatus.phase, VaultPhase.unlocked);
    expect((await s.inventory.watchHosts().first).single.name, 'db-it');
    expect(s.profiles.currentProfiles.profiles.map((p) => p.name), containsAll(['IT Personal', 'IT Restored']));

    // Switching back locks the restored profile and opens the original.
    await s.profiles.switchTo(profile.id);
    expect(s.vault.currentStatus.phase, VaultPhase.locked);
  });

  testWidgets('core-gaps: reveal, planner preview, snippets, errors, SFTP browser, prompts, backups', (tester) async {
    rust ??= await RustAppServices.open(options);
    final s = rust!.services;
    if (s.vault.currentStatus.phase != VaultPhase.unlocked) {
      await s.vault.unlockWithPassphrase(SecretText(_passphrase));
    }
    final host = (await s.inventory.watchHosts().first).firstWhere((h) => h.name == 'db-it');

    // Explicit reveal of the inline password (returned once, wiped by us).
    final secret = await s.inventory.revealCredentialSecret(host.credentialId!);
    expect(secret.expose(), _hostPassword);
    secret.wipe();

    // Planner preview from the core, with diagnostic codes.
    final effective = await s.inventory.resolveEffective(host);
    expect(effective.port.value, 22);
    expect(effective.port.source, ValueSource.appDefault);
    expect(effective.credentialId.value, host.credentialId);
    final orphan = await s.inventory.resolveEffective(Host.create(name: 'draft', address: '10.0.0.6'));
    expect(orphan.diagnostics.map((d) => d.code), contains('no_credential'));

    // Snippets: search + render in the core (no AI provider configured).
    final now = DateTime.now().toUtc();
    final snippet = await s.snippets.saveSnippet(
      Snippet(
        id: ObjectId.generate(),
        name: 'show log',
        snippetType: SnippetType.bash,
        template: 'tail -n {{lines}} /var/log/app.log',
        createdAt: now,
        updatedAt: now,
      ),
    );
    final hits = await s.snippets.search('app.log');
    expect(hits.map((h) => h.snippet.id), contains(snippet.id));
    expect(await s.snippets.render(snippet, {'lines': '20'}), 'tail -n 20 /var/log/app.log');
    await expectLater(
      s.snippets.render(snippet, const {}),
      throwsA(isA<AppException>().having((e) => e.reason, 'reason', AppErrorReason.missingVariables)),
    );

    // Reasons come from the core.
    await expectLater(
      s.vault.changePassphrase(current: SecretText('wrong current passphrase'), next: SecretText('$_passphrase-2')),
      throwsA(isA<AppException>().having((e) => e.reason, 'reason', AppErrorReason.currentPassphraseWrong)),
    );
    await expectLater(
      s.auth.logout(),
      throwsA(isA<AppException>().having((e) => e.reason, 'reason', AppErrorReason.syncedProfileRequired)),
    );

    // SFTP browser + prompts are wired to the core.
    final browser = s.sftpBrowser!;
    expect(await browser.watchEditSessions().first, isEmpty);
    expect(await browser.listEditLeftovers(), isEmpty);
    final prompts = s.prompts!;
    final detach = prompts.attachPresenter();
    expect(prompts.pending, isEmpty);
    detach();

    // Backup schedule / scheduler state lives in the core.
    final folder = Directory('${root.path}/auto')..createSync();
    await s.backups.updateSchedule(const BackupSchedule().copyWith(enabled: true, folder: folder.path, keepLast: 2));
    final schedule = await s.backups.watchSchedule().first;
    expect(schedule.nextRunAt, isNotNull);
    final auto = await s.backups.backupNow();
    expect(auto.automatic, isTrue);
    expect(File(auto.path).existsSync(), isTrue);
    expect((await s.backups.watchRecentBackups().first).map((b) => b.path), contains(auto.path));
  });
}
