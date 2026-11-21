import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/keepass/external_vault.dart';
import 'package:consolecrypt/keepass/external_vault_repository.dart';
import 'package:consolecrypt/keepass/password_manager.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  late MockBackend backend;
  late ProviderContainer container;
  late PasswordManager manager;
  late String vaultId;
  Future<void> deliverSettings() async {
    // MockSettingsService publishes its snapshots asynchronously; let both
    // streams deliver their events before pumping Riverpod's dependent reads.
    await Future<void>.delayed(Duration.zero);
    await container.pump();
  }

  setUp(() async {
    backend = MockBackend(config: const MockConfig.test());
    await backend.debugSignInDemoAndUnlock();
    container = ProviderContainer(overrides: [appServicesProvider.overrideWithValue(backend.services)]);
    addTearDown(backend.dispose);
    addTearDown(container.dispose);
    container.listen(profilesProvider, (_, _) {});
    container.listen(vaultStatusProvider, (_, _) {});
    container.listen(vaultSettingsProvider, (_, _) {});
    await container.read(profilesProvider.future);
    vaultId = (await container.read(vaultStatusProvider.future)).vaultId!.value;
    manager = container.read(passwordManagerProvider);
  });

  ExternalVaultConnection connection(
    String id, {
    String? name,
    String? profile,
    String? vault,
    String endpoint = 'https://bao.example',
    ExternalVaultAuth auth = ExternalVaultAuth.token,
  }) => ExternalVaultConnection(
    id: id,
    profile: profile ?? manager.profile,
    vault: vault ?? vaultId,
    name: name ?? id,
    endpoint: endpoint,
    auth: auth,
    username: auth == ExternalVaultAuth.token ? '' : 'tester',
    authMount: auth == ExternalVaultAuth.token ? '' : 'userpass',
    mount: 'kv',
    namespace: '',
    versioned: true,
  );

  test('five most recent sources share one order across local, WebDAV, Vault and OpenBao', () async {
    final settings = container.read(settingsServiceProvider);
    final profile = manager.profile;
    final files = [for (var i = 0; i < 3; i++) KdbxSource.local('fixture$i.kdbx')];
    const dav = PasswordVaultConnection(id: 'dav', name: 'Shared', url: 'https://dav.example/p.kdbx');
    final remote = KdbxSource.dav(dav);
    final vaultSettings = (await container.read(vaultSettingsProvider.future))!;
    await settings.updateVault(vaultSettings.copyWith(passwordVaults: [dav]));
    await container.pump();
    await settings.updateLocal(
      settings.currentLocal.copyWith(
        passwordVaultFiles: [
          for (final file in files) {'profile': profile, 'path': file.path},
        ],
        passwordVaultRecentByProfile: {
          profile: [remote.id],
          'other-profile': ['other-id'],
        },
      ),
    );
    final vault = connection('vault');
    final bao = connection('bao');
    final vaultKey = RecentExternalVaultSource(vault).id;
    final baoKey = RecentExternalVaultSource(bao).id;
    List<String> visible() => manager.recentPasswordSources([vault, bao]).map((source) => source.id).toList();
    expect(visible(), [remote.id, ...files.reversed.map((source) => source.id)]);

    await manager.persistExternalVault(vault, wrapped: 'private-wrapper', opened: true);
    await manager.persistExternalVault(bao, opened: true);
    expect(visible(), [baoKey, vaultKey, remote.id, files[2].id, files[1].id]);
    await manager.rememberExternalVault(vault);
    final restored = localSettingsFromJson(localSettingsToJson(settings.currentLocal));
    expect(restored.passwordVaultRecentByProfile[profile], [vaultKey, baoKey, remote.id, files[2].id, files[1].id]);
    expect(restored.passwordVaultRecentByProfile['other-profile'], ['other-id']);
    expect(vaultKey, matches(RegExp(r'^vault:[0-9a-f]{64}$')));
    expect(restored.externalVaultConnections.singleWhere((row) => row['id'] == 'vault')['wrapped'], 'private-wrapper');
    await settings.updateLocal(restored);
    expect(visible(), restored.passwordVaultRecentByProfile[profile]);

    final renamed = connection('vault', name: 'New name');
    await manager.persistExternalVault(renamed, wrapped: 'new-wrapper', opened: true);
    expect(RecentExternalVaultSource(renamed).id, vaultKey);
    expect(settings.currentLocal.externalVaultConnections.single['wrapped'], 'new-wrapper');
    await manager.persistExternalVault(renamed, remove: true);
    await manager.rememberExternalVault(vault); // A picker borrowing the old session cannot restore a removed row.
    expect(visible(), [baoKey, remote.id, files[2].id, files[1].id, files[0].id]);
    expect(settings.currentLocal.passwordVaultRecentByProfile[profile], isNot(contains(vaultKey)));
    await manager.forgetSource(remote);
    expect(settings.currentLocal.passwordVaultRecentByProfile[profile], isNot(contains(remote.id)));
    await manager.forgetSource(files[2]);
    expect(settings.currentLocal.passwordVaultRecentByProfile[profile], isNot(contains(files[2].id)));
    expect(visible(), [baoKey, files[1].id, files[0].id]);
  });

  test(
    'device credential capacity is checked before remote writes and concurrent saves preserve all device keys',
    () async {
      final settings = backend.settings;
      final rows = [
        for (var i = 0; i < 256; i++)
          connection('other$i', profile: 'other').toStorage(cipherText: i.isEven ? 'key$i' : null, synced: i.isEven),
      ];
      final login = SecretText('example-login');
      addTearDown(login.wipe);
      await settings.updateLocal(settings.currentLocal.copyWith(externalVaultConnections: rows));
      final before = (await backend.inventory.watchCredentials().first).length;
      final a = connection('a');
      final b = connection('b');
      Future<void> register(ExternalVaultConnection c) =>
          manager.persistExternalVault(c, wrapped: 'wrapper-${c.id}', opened: true, syncCredential: login);
      await expectLater(register(a), throwsA(isA<ExternalVaultFailure>().having((e) => e.code, 'code', 'store')));
      expect(settings.currentLocal.externalVaultConnections, rows);
      expect((await backend.settings.watchVault().first)!.externalVaults, isEmpty);
      expect((await backend.inventory.watchCredentials().first).length, before);

      await manager.persistExternalVault(connection('metadata-only'), opened: true);
      expect(settings.currentLocal.externalVaultConnections, rows);
      // One existing metadata-only shadow is expendable. Only one concurrent
      // registration can use that slot, even though both start from 256 rows.
      final reusable = [...rows.take(255), connection('shadow', profile: 'other').toStorage(synced: true)];
      await settings.updateLocal(settings.currentLocal.copyWith(externalVaultConnections: reusable));
      final first = register(a);
      final second = expectLater(
        register(b),
        throwsA(isA<ExternalVaultFailure>().having((e) => e.code, 'code', 'store')),
      );
      await Future.wait([first, second]);
      expect(settings.currentLocal.externalVaultConnections, [
        ...rows.take(255),
        a.toStorage(cipherText: 'wrapper-a', synced: true),
      ]);
      expect((await backend.settings.watchVault().first)!.externalVaults.map((c) => c.id), ['metadata-only', 'a']);
      expect((await backend.inventory.watchCredentials().first).length, before + 1);
      await manager.persistExternalVault(a, wrapped: 'replacement-wrapper', opened: true);
      expect(settings.currentLocal.externalVaultConnections.last['wrapped'], 'replacement-wrapper');
    },
  );

  test('saved but unverified connections do not enter history and profile isolation survives stale IDs', () async {
    final saved = connection('saved');
    final foreignProfile = connection('saved', profile: 'other');
    final foreignVault = connection('saved', vault: 'other');
    final settings = container.read(settingsServiceProvider);
    await manager.persistExternalVault(saved);
    expect(manager.recentPasswordSources([saved]), isEmpty);
    await manager.rememberExternalVault(saved);
    expect(manager.recentPasswordSources([saved]).single.id, RecentExternalVaultSource(saved).id);
    final all = [foreignVault, foreignProfile, saved];
    await settings.updateLocal(
      settings.currentLocal.copyWith(
        passwordVaultRecentByProfile: {
          manager.profile: ['stale', ...all.map((c) => RecentExternalVaultSource(c).id)],
        },
      ),
    );
    expect(manager.recentPasswordSources(all).single.id, RecentExternalVaultSource(saved).id);
    for (final foreign in [foreignVault, foreignProfile]) {
      await expectLater(manager.persistExternalVault(foreign, opened: true), throwsA(isA<ExternalVaultFailure>()));
    }
    manager.dispose();
    expect(manager.recentPasswordSources(all), isEmpty);
    await expectLater(manager.rememberExternalVault(saved), throwsA(isA<ExternalVaultFailure>()));
  });

  test(
    'legacy connection sync copies only metadata, preserves biometric binding, and respects remote deletion',
    () async {
      final config = connection('legacy');
      final settings = backend.settings;
      await settings.updateLocal(
        settings.currentLocal.copyWith(
          externalVaultConnections: [
            config.toStorage(cipherText: 'device-only-key-wrapper'),
            connection('foreign', profile: 'another').toStorage(),
          ],
        ),
      );
      container.listen(externalVaultConnectionsProvider, (_, _) {});
      await settings.watchLocal().firstWhere((local) => local.externalVaultConnections.first['synced'] == '1');
      final catalog = (await settings.watchVault().first)!;
      final row = catalog.externalVaults.single;
      expect(row.id, config.id);
      expect(row.credentialId, isNull);
      expect(row.toJson().keys, isNot(contains('wrapped')));
      expect(row.toJson().keys, isNot(contains('profile')));
      await deliverSettings();
      expect(container.read(externalVaultConnectionsProvider).single.wrapped, 'device-only-key-wrapper');

      await settings.updateVault(
        catalog.copyWith(externalVaults: [connection(config.id, endpoint: 'https://other.example').portable()]),
      );
      await deliverSettings();
      expect(container.read(externalVaultConnectionsProvider).single.wrapped, isNull);
      await settings.updateVault(catalog.copyWith(externalVaults: []));
      await deliverSettings();
      expect(container.read(externalVaultConnectionsProvider), isEmpty);
      expect((await settings.watchVault().first)!.externalVaults, isEmpty);
    },
  );

  for (final auth in ExternalVaultAuth.values) {
    test(
      'opt-in ${auth.name} login survives on a fresh device, rotates and revokes without sharing biometrics',
      () async {
        final config = connection('connection', auth: auth);
        final first = SecretText('test-login');
        final second = SecretText('replacement');
        addTearDown(first.wipe);
        addTearDown(second.wipe);
        final settings = backend.settings;
        container.listen(externalVaultConnectionsProvider, (_, _) {});
        final before = (await backend.inventory.watchCredentials().first).length;
        await expectLater(
          manager.persistExternalVault(config, syncCredential: first),
          throwsA(isA<ExternalVaultFailure>()),
        );
        await manager.persistExternalVault(config, opened: true, wrapped: 'local-wrapper', syncCredential: first);
        await deliverSettings();
        final saved = container.read(externalVaultConnectionsProvider).single;
        final originalId = saved.credentialId;
        expect(originalId, isNotNull);
        expect(saved.wrapped, 'local-wrapper');
        final shared = (await settings.watchVault().first)!.externalVaults.single;
        expect(shared.toJson().values, isNot(contains('test-login')));
        expect(shared.toJson().values, isNot(contains('local-wrapper')));
        expect((await backend.inventory.watchCredentials().first).length, before + 1);

        await manager.persistExternalVault(config, opened: true, expected: saved, syncCredential: first);
        await settings.updateLocal(const LocalSettings());
        await deliverSettings();
        final onOtherDevice = container.read(externalVaultConnectionsProvider).single;
        expect(onOtherDevice.wrapped, isNull);
        expect(onOtherDevice.credentialId, originalId);
        final recovered = await manager.unlockExternalVault(onOtherDevice);
        expect(recovered.constantTimeEquals(first), isTrue);
        recovered.wipe();
        await manager.persistExternalVault(config, opened: true, expected: onOtherDevice);
        await manager.persistExternalVault(config, opened: true, expected: onOtherDevice, syncCredential: second);
        await deliverSettings();
        final rotated = container.read(externalVaultConnectionsProvider).single;
        expect(rotated.credentialId, isNot(originalId));
        expect((await backend.inventory.watchCredentials().first).length, before + 1);
        await expectLater(manager.unlockExternalVault(onOtherDevice), throwsA(isA<ExternalVaultFailure>()));

        final revocation = await manager.forgetExternalVaultLogin(rotated);
        expect(revocation.credentialId, isNull);
        expect((await settings.watchVault().first)!.externalVaults.single.credentialId, isNull);
        expect((await backend.inventory.watchCredentials().first).length, before);
        await expectLater(manager.unlockExternalVault(rotated), throwsA(isA<ExternalVaultFailure>()));
        await manager.persistExternalVault(revocation, remove: true);
        await deliverSettings();
        expect(container.read(externalVaultConnectionsProvider), isEmpty);
        await expectLater(
          manager.persistExternalVault(config, opened: true, expected: revocation),
          throwsA(isA<ExternalVaultFailure>()),
        );
      },
    );
  }

  test(
    'a changed endpoint cannot reuse another binding and an explicitly reused credential survives removal',
    () async {
      final config = connection('source');
      final login = SecretText('fixture');
      addTearDown(login.wipe);
      final settings = backend.settings;
      await manager.persistExternalVault(config, opened: true, syncCredential: login);
      final shared = (await settings.watchVault().first)!;
      final id = shared.externalVaults.single.credentialId!;
      final redirected = connection(config.id, endpoint: 'https://changed.example').portable(credentialId: id);
      await settings.updateVault(shared.copyWith(externalVaults: [redirected]));
      final mismatched = container.read(externalVaultRepositoryProvider).resolve(redirected);
      await expectLater(manager.unlockExternalVault(mismatched), throwsA(isA<ExternalVaultFailure>()));
      await settings.updateVault(
        shared.copyWith(
          passwordVaults: [
            PasswordVaultConnection(id: 'dav', name: 'Example', url: 'https://dav.example/test.kdbx', credentialId: id),
          ],
        ),
      );
      final valid = container.read(externalVaultRepositoryProvider).resolve(shared.externalVaults.single);
      await manager.persistExternalVault(valid, remove: true);
      expect((await backend.inventory.watchCredentials().first).any((c) => c.id.value == id), isTrue);
    },
  );
}
