import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/mock/mock_cloud.dart';
import 'package:consolecrypt/core/mock/mock_risk_rules.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:flutter_test/flutter_test.dart';

MockBackend newBackend() {
  final backend = MockBackend(config: const MockConfig.test());
  addTearDown(backend.dispose);
  return backend;
}

Matcher throwsAppError(AppErrorCode code) => throwsA(isA<AppException>().having((e) => e.code, 'code', code));

void main() {
  group('profiles & vault (ADR-0106)', () {
    test('local profile: create vault, lock, unlock, wrong passphrase', () async {
      final b = newBackend();
      final profile = await b.profiles.createLocalProfile(name: 'Personal');
      expect(b.profiles.currentProfiles.active?.id, profile.id);
      expect(b.vault.currentStatus.phase, VaultPhase.none);

      final kit = await b.vault.createVault(
        name: 'Personal',
        passphrase: SecretText('violet-anchor-muffin-glacier-42'),
      );
      expect(kit.serverUrl, isNull, reason: 'local vaults have no server');
      expect(b.vault.currentStatus.recoveryKitConfirmed, isFalse);
      await b.vault.confirmRecoveryKitSaved();
      await b.vault.setDeviceUnlockEnabled(true, reason: 'test');

      await b.vault.lock();
      expect(b.vault.currentStatus.phase, VaultPhase.locked);
      expect(() => b.vault.unlockWithPassphrase(SecretText('nope')), throwsAppError(AppErrorCode.wrongPassphrase));
      await b.vault.unlockWithDevice(reason: 'test');
      expect(b.vault.currentStatus.isUnlocked, isTrue);
    });

    test('recovery with the Recovery Key sets a new passphrase', () async {
      final b = newBackend();
      await b.profiles.createLocalProfile(name: 'P');
      final kit = await b.vault.createVault(name: 'P', passphrase: SecretText('violet-anchor-muffin-glacier-42'));
      final words = kit.exposeWords().join(' ');
      await b.vault.confirmRecoveryKitSaved();
      await b.vault.lock();
      expect(
        () => b.vault.recoverWithRecoveryKey(
          recoveryInput: SecretText(List.filled(24, 'abandon').join(' ')),
          newPassphrase: SecretText('seven brisk otters juggle neon'),
        ),
        throwsAppError(AppErrorCode.invalidRecoveryKey),
      );
      await b.vault.recoverWithRecoveryKey(
        recoveryInput: SecretText(words),
        newPassphrase: SecretText('seven brisk otters juggle neon'),
      );
      await b.vault.lock();
      await b.vault.unlockWithPassphrase(SecretText('seven brisk otters juggle neon'));
      expect(b.vault.currentStatus.isUnlocked, isTrue);
    });

    test('switching profiles locks the previous vault', () async {
      final b = newBackend();
      await b.debugSignInDemoAndUnlock();
      final work = b.profiles.currentProfiles.active!;
      await b.debugCreateUnlockedLocalProfile();
      expect(b.profiles.currentProfiles.profiles, hasLength(2));
      await b.profiles.switchTo(work.id);
      expect(b.vault.currentStatus.phase, VaultPhase.locked);
    });

    test('new device must be approved or attest with the passphrase', () async {
      final b = newBackend();
      await b.profiles.createSyncedProfile(
        serverUrl: Uri.parse('https://sync.example.org'),
        email: MockCloud.newDeviceEmail,
        password: SecretText(MockCloud.demoPassword),
        deviceName: 'New Mac',
        createAccount: false,
      );
      expect(b.vault.currentStatus.deviceTrusted, isFalse);
      expect(() => b.vault.unlockWithDevice(reason: 't'), throwsAppError(AppErrorCode.deviceNotTrusted));
      await b.vault.requestDeviceApproval();
      expect(b.vault.currentStatus.phase, VaultPhase.awaitingApproval);
      await b.simulateApprovalFromOtherDevice();
      expect(b.vault.currentStatus.isUnlocked, isTrue);
      expect(b.vault.currentStatus.deviceTrusted, isTrue);
    });

    test('login errors', () async {
      final b = newBackend();
      expect(
        () => b.profiles.createSyncedProfile(
          serverUrl: Uri.parse('https://sync.example.org'),
          email: MockCloud.demoEmail,
          password: SecretText('wrong-password'),
          deviceName: 'x',
          createAccount: false,
        ),
        throwsAppError(AppErrorCode.invalidCredentials),
      );
      expect(
        () => b.profiles.createSyncedProfile(
          serverUrl: Uri.parse('https://sync.example.org'),
          email: MockCloud.demoEmail,
          password: SecretText('long-enough-password'),
          deviceName: 'x',
          createAccount: true,
        ),
        throwsAppError(AppErrorCode.emailTaken),
      );
    });
  });

  group('inventory planning preview', () {
    test('group inheritance and jump profile resolve with provenance', () async {
      final b = newBackend();
      await b.debugSignInDemoAndUnlock();
      final hosts = await b.inventory.watchHosts().first;
      final db = hosts.firstWhere((h) => h.name == 'prod-db-1');
      final eff = await b.inventory.resolveEffective(db);
      expect(eff.username.value, 'dba');
      expect(eff.username.source, ValueSource.group);
      expect(eff.username.sourceName, 'Databases');
      expect(eff.credentialName, 'prod-deploy');
      expect(eff.credentialId.sourceName, 'Production');
      expect(eff.port.value, 22);
      expect(eff.route.map((h) => h.label.split(' ').first), ['bastion-a', 'bastion-b']);
      expect(eff.groupPath, ['Production', 'Databases']);

      final web = hosts.firstWhere((h) => h.name == 'prod-web-1');
      expect((await b.inventory.resolveEffective(web)).port.value, 2222);
    });

    test('secrets are only returned on explicit reveal, and imported keys keep encryption', () async {
      final b = newBackend();
      await b.debugCreateUnlockedLocalProfile();
      final cred = await b.inventory.createPasswordCredential(name: 'db', password: SecretText('s3cret-value'));
      expect(cred.toString(), isNot(contains('s3cret')));
      expect((await b.inventory.revealCredentialSecret(cred.id)).expose(), 's3cret-value');

      final generated = await b.inventory.generateKeyCredential(name: 'k', algorithm: KeyAlgorithm.ed25519);
      expect(generated.publicKey, startsWith('ssh-ed25519 '));
      expect(
        () => b.inventory.generateKeyCredential(name: 'k', algorithm: KeyAlgorithm.rsa2048),
        throwsAppError(AppErrorCode.unsupported),
      );
    });
  });

  group('host authentication (saveHostWithAuth)', () {
    test('inline password lifecycle: create → update → prompt deletes it', () async {
      final b = newBackend();
      await b.debugCreateUnlockedLocalProfile();
      final draft = Host.create(name: 'web', address: 'web.example.org');
      expect(
        () => b.inventory.saveHostWithAuth(draft, const HostAuthInlinePassword()),
        throwsAppError(AppErrorCode.validation),
      );

      var host = await b.inventory.saveHostWithAuth(draft, HostAuthInlinePassword(password: SecretText('one')));
      final credId = host.credentialId!;
      expect(host.inlineCredentialId, credId);
      expect((await b.inventory.revealCredentialSecret(credId)).expose(), 'one');

      host = await b.inventory.saveHostWithAuth(host, const HostAuthInlinePassword());
      expect(host.credentialId, credId, reason: 'keep existing secret');
      expect((await b.inventory.revealCredentialSecret(credId)).expose(), 'one');

      host = await b.inventory.saveHostWithAuth(host, HostAuthInlinePassword(password: SecretText('two')));
      expect(host.credentialId, credId);
      expect((await b.inventory.revealCredentialSecret(credId)).expose(), 'two');

      host = await b.inventory.saveHostWithAuth(host, const HostAuthPasswordPrompt());
      expect(host.credentialId, isNull);
      expect(host.promptsForPassword, isTrue);
      expect((await b.inventory.watchCredentials().first).any((c) => c.id == credId), isFalse);
      final eff = await b.inventory.resolveEffective(host);
      expect(eff.credentialName, contains('asked when connecting'));
    });

    test('shared credentials are linked, never deleted; prompt stops group inheritance', () async {
      final b = newBackend();
      await b.debugSignInDemoAndUnlock();
      final hosts = await b.inventory.watchHosts().first;
      final creds = await b.inventory.watchCredentials().first;
      final shared = creds.firstWhere((c) => c.name == 'home-lab password');
      final db = hosts.firstWhere((h) => h.name == 'prod-db-1'); // inherits prod-deploy

      var host = await b.inventory.saveHostWithAuth(db, HostAuthCredential(shared.id));
      expect(host.credentialId, shared.id);
      host = await b.inventory.saveHostWithAuth(host, const HostAuthInherit());
      expect(host.credentialId, isNull);
      expect((await b.inventory.watchCredentials().first).any((c) => c.id == shared.id), isTrue);
      expect((await b.inventory.resolveEffective(host)).credentialName, 'prod-deploy');

      host = await b.inventory.saveHostWithAuth(host, const HostAuthPasswordPrompt());
      expect((await b.inventory.resolveEffective(host)).credentialId.value, isNull);
    });

    test('agent auth reuses a matching agent credential or creates one', () async {
      final b = newBackend();
      await b.debugSignInDemoAndUnlock();
      final before = (await b.inventory.watchCredentials().first).length;
      final draft = Host.create(name: 'a', address: 'a.example.org');
      final os = await b.inventory.saveHostWithAuth(draft, const HostAuthAgent(kind: CredentialKind.osSshAgent));
      expect((await b.inventory.watchCredentials().first).length, before, reason: 'reused System ssh-agent');
      final ext = await b.inventory.saveHostWithAuth(
        os,
        const HostAuthAgent(kind: CredentialKind.externalAgent, agentPath: '/tmp/agent.sock'),
      );
      final creds = await b.inventory.watchCredentials().first;
      expect(creds.length, before + 1);
      expect(creds.firstWhere((c) => c.id == ext.credentialId).agentPath, '/tmp/agent.sock');
    });

    test('deleting a host deletes its inline password credential', () async {
      final b = newBackend();
      await b.debugCreateUnlockedLocalProfile();
      final host = await b.inventory.saveHostWithAuth(
        Host.create(name: 'x', address: 'x.example.org'),
        HostAuthInlinePassword(password: SecretText('pw')),
      );
      await b.inventory.deleteHost(host.id);
      expect(await b.inventory.watchCredentials().first, isEmpty);
    });

    test('remember / forget key passphrase', () async {
      final b = newBackend();
      await b.debugCreateUnlockedLocalProfile();
      final plain = await b.inventory.generateKeyCredential(name: 'plain', algorithm: KeyAlgorithm.ed25519);
      expect(
        () => b.inventory.rememberKeyPassphrase(plain.id, SecretText('x')),
        throwsAppError(AppErrorCode.validation),
      );
      final enc = await b.inventory.generateKeyCredential(
        name: 'enc',
        algorithm: KeyAlgorithm.ed25519,
        passphrase: SecretText('key-pass'),
      );
      expect(enc.remembersPassphrase, isFalse);
      expect((await b.inventory.rememberKeyPassphrase(enc.id, SecretText('key-pass'))).remembersPassphrase, isTrue);
      expect((await b.inventory.forgetKeyPassphrase(enc.id)).remembersPassphrase, isFalse);
    });
  });

  group('devices (ADR-0004)', () {
    test('approval requires the matching verification code', () async {
      final b = newBackend();
      await b.debugSignInDemoAndUnlock();
      final snapshot = await b.devices.watchDevices().first;
      final request = snapshot.pendingRequests.single;
      final code = await b.devices.verificationCodeFor(request.requestId);
      final wrong = VerificationCode.tryParse('000000000000000000000000000000')!;
      expect(
        () => b.devices.approve(request.requestId, confirmedCode: wrong),
        throwsAppError(AppErrorCode.verificationMismatch),
      );
      await b.devices.approve(request.requestId, confirmedCode: code);
      final after = await b.devices.watchDevices().first;
      expect(after.pendingRequests, isEmpty);
      expect(after.devices.firstWhere((d) => d.name == 'MacBook Air').trustedVaults, isNotEmpty);
    });

    test('the current device cannot revoke itself; others can be revoked', () async {
      final b = newBackend();
      await b.debugSignInDemoAndUnlock();
      final snapshot = await b.devices.watchDevices().first;
      final current = snapshot.devices.firstWhere((d) => d.isCurrent);
      expect(() => b.devices.revoke(current.deviceId), throwsAppError(AppErrorCode.validation));
      final office = snapshot.devices.firstWhere((d) => d.name == 'Office PC');
      await b.devices.revoke(office.deviceId);
      final after = await b.devices.watchDevices().first;
      expect(after.devices.firstWhere((d) => d.name == 'Office PC').isRevoked, isTrue);
    });
  });

  group('sync', () {
    test('local profiles report localOnly and queue nothing', () async {
      final b = newBackend();
      await b.debugCreateUnlockedLocalProfile();
      await b.inventory.saveHost(Host.create(name: 'x', address: 'x.example.org'));
      await pumpEventQueue();
      expect(b.sync.currentStatus.state, SyncState.localOnly);
      expect(b.sync.currentStatus.pendingChanges, 0);
    });

    test('synced profiles push changes; offline queues them', () async {
      final b = newBackend();
      await b.debugSignInDemoAndUnlock();
      await pumpEventQueue();
      expect(b.sync.currentStatus.state, SyncState.idle);
      b.setSimulatedOffline(true);
      await b.inventory.saveHost(Host.create(name: 'x', address: 'x.example.org'));
      await pumpEventQueue();
      expect(b.sync.currentStatus.state, SyncState.offline);
      expect(b.sync.currentStatus.pendingChanges, 1);
      b.setSimulatedOffline(false);
      await pumpEventQueue();
      expect(b.sync.currentStatus.pendingChanges, 0);
      expect(b.sync.currentStatus.state, SyncState.idle);
    });

    test('enable sync converts a local profile, disconnect reverts it', () async {
      final b = newBackend();
      await b.debugCreateUnlockedLocalProfile();
      final vaultId = b.vault.currentStatus.vaultId;
      final steps = await b.sync
          .enableSync(
            serverUrl: Uri.parse('https://sync.example.org'),
            email: 'me@example.org',
            password: SecretText('long-enough-password'),
            deviceName: 'Mac',
            createAccount: true,
          )
          .map((p) => p.step)
          .toList();
      expect(steps.first, EnableSyncStep.authenticating);
      expect(steps, contains(EnableSyncStep.creatingRemoteVault));
      expect(steps, contains(EnableSyncStep.uploading));
      expect(steps.last, EnableSyncStep.done);
      final profile = b.profiles.currentProfiles.active!;
      expect(profile.isSynced, isTrue);
      expect(profile.vaultId, vaultId, reason: 'vault keeps its id');
      expect(b.auth.currentAuthState.isSignedIn, isTrue);

      await b.sync.disconnect(revokeThisDevice: false);
      expect(b.profiles.currentProfiles.active!.isLocal, isTrue);
      expect(b.vault.currentStatus.isUnlocked, isTrue, reason: 'data stays available locally');
    });
  });

  group('backups (ADR-0106)', () {
    test('export then restore into a new local profile with the same data', () async {
      final b = newBackend();
      await b.debugCreateUnlockedLocalProfile();
      await b.inventory.saveHost(Host.create(name: 'kept', address: 'kept.example.org'));
      final info = await b.backups.exportBackup(path: '/tmp/test.ccbackup');
      expect(info.objectCount, greaterThan(0));
      expect((await b.backups.inspectBackup('/tmp/test.ccbackup')).vaultId, info.vaultId);

      expect(
        () => b.backups.restoreBackup(
          path: '/tmp/test.ccbackup',
          unlock: BackupUnlockWithPassphrase(SecretText('wrong')),
        ),
        throwsAppError(AppErrorCode.wrongPassphrase),
      );
      final restored = await b.backups.restoreBackup(
        path: '/tmp/test.ccbackup',
        unlock: BackupUnlockWithPassphrase(SecretText('violet-anchor-muffin-glacier-42')),
      );
      expect(restored.isLocal, isTrue);
      expect(b.profiles.currentProfiles.profiles, hasLength(2));
      final hosts = await b.inventory.watchHosts().first;
      expect(hosts.map((h) => h.name), contains('kept'));
    });

    test('schedule requires a folder and applies retention', () async {
      final b = newBackend();
      await b.debugCreateUnlockedLocalProfile();
      expect(
        () => b.backups.updateSchedule(const BackupSchedule(enabled: true)),
        throwsAppError(AppErrorCode.validation),
      );
      await b.backups.updateSchedule(const BackupSchedule(enabled: true, folder: '/backups', keepLast: 2));
      for (var i = 0; i < 3; i++) {
        await b.backups.backupNow();
      }
      final recent = await b.backups.watchRecentBackups().first;
      expect(recent.where((r) => r.automatic), hasLength(2));
    });
  });

  group('local risk rules (mock of ai-core)', () {
    RiskLevel local(String cmd) =>
        MockRiskRules.assess(cmd, declared: RiskLevel.unknown, source: SnippetSource.user).local;

    test('classifies common commands', () {
      expect(local('df -h'), RiskLevel.readOnly);
      expect(local('kubectl get pods -A | grep api'), RiskLevel.readOnly);
      expect(local('sudo systemctl restart nginx'), RiskLevel.modifying);
      expect(local('helm upgrade api ./chart'), RiskLevel.modifying);
      expect(local('rm -rf /var/tmp/x'), RiskLevel.destructive);
      expect(local("find /var/log -name '*.gz' -delete"), RiskLevel.destructive);
      expect(local('DROP TABLE users;'), RiskLevel.destructive);
      expect(local('frobnicate --all'), RiskLevel.unknown);
    });

    test('AI read-only hint cannot downgrade a destructive command', () {
      final a = MockRiskRules.assess('find / -name x -delete', declared: RiskLevel.readOnly, source: SnippetSource.ai);
      expect(a.effective, RiskLevel.destructive);
    });
  });
}
