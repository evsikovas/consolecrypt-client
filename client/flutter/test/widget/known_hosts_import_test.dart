import 'dart:async';

import 'package:consolecrypt/app/app.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/hosts/known_hosts_import.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

class _Import extends KnownHostsImportService {
  final pending = Completer<KnownHostsScan>();
  int reads = 0;
  @override
  Future<KnownHostsScan> scan() {
    reads++;
    return pending.future;
  }
}

void main() {
  testWidgets('new host previews files, skips existing endpoints, imports only selected addresses', (tester) async {
    final backend = testBackend(seed: false);
    addTearDown(backend.dispose);
    await backend.debugCreateUnlockedLocalProfile();
    setTestLocale(backend, AppLocale.en);
    final now = DateTime.now().toUtc();
    await backend.inventory.saveHost(
      Host(
        id: ObjectId.generate(),
        name: 'Already saved',
        address: 'existing.test',
        port: 22,
        createdAt: now,
        updatedAt: now,
      ),
    );
    final source = _Import();
    tester.view.physicalSize = const Size(1200, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          appServicesProvider.overrideWithValue(backend.services),
          knownHostsImportServiceProvider.overrideWithValue(source),
        ],
        child: const ConsoleCryptApp(),
      ),
    );
    await settle(tester);
    final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    container.read(routerProvider).go(AppRoutes.newHost);
    await settle(tester);
    expect(source.reads, 0, reason: 'opening the editor does not read SSH files');
    await tapKey(tester, 'host-editor-import');
    expect(source.reads, 1);
    source.pending.complete(
      const KnownHostsScan(
        hosts: [
          KnownHostCandidate('EXISTING.test', 22),
          KnownHostCandidate('existing.test', 2222),
          KnownHostCandidate('unselected.test', 22),
          KnownHostCandidate('2001:db8::1', 2200),
        ],
        sources: ['/demo/.ssh/known_hosts', '/etc/ssh/ssh_known_hosts'],
        skipped: 2,
      ),
    );
    await settle(tester);
    final duplicate = tester.widget<CheckboxListTile>(
      find.byKey(ValueKey('import-host-${const KnownHostCandidate('EXISTING.test', 22).identity}')),
    );
    expect(duplicate.onChanged, isNull);
    expect(duplicate.value, isFalse);
    await tapKey(tester, 'import-host-${const KnownHostCandidate('unselected.test', 22).identity}');
    expect(container.read(hostsProvider).requireValue, hasLength(1));
    await tapKey(tester, 'known-hosts-import-save');
    final hosts = container.read(hostsProvider).requireValue;
    expect(hosts, hasLength(3));
    final imported = hosts.where((h) => h.name != 'Already saved');
    expect(imported.map((h) => (h.address, h.port)), containsAll([('existing.test', 2222), ('2001:db8::1', 2200)]));
    for (final host in imported) {
      expect(host.protocol, HostProtocol.ssh);
      expect(host.username, isNull);
      expect(host.credentialId, isNull);
      expect(host.hostKeyPolicy, HostKeyPolicy.ask);
    }
    expect(await tester.runAsync(() => backend.inventory.watchKnownHosts().first), isEmpty);
    expect(find.byKey(const ValueKey('known-hosts-import')), findsNothing);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('locking during file reading dismisses import and cannot save into another vault', (tester) async {
    final backend = testBackend(seed: false);
    addTearDown(backend.dispose);
    await backend.debugCreateUnlockedLocalProfile();
    final source = _Import();
    tester.view.physicalSize = const Size(1200, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          appServicesProvider.overrideWithValue(backend.services),
          knownHostsImportServiceProvider.overrideWithValue(source),
        ],
        child: const ConsoleCryptApp(),
      ),
    );
    await settle(tester);
    final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    container.read(routerProvider).go(AppRoutes.newHost);
    await settle(tester);
    await tapKey(tester, 'host-editor-import');
    await backend.vault.lock();
    await settle(tester);
    source.pending.complete(const KnownHostsScan(hosts: [KnownHostCandidate('new.test', 22)]));
    await settle(tester);
    expect(find.byKey(const ValueKey('known-hosts-import')), findsNothing);
    expect(container.read(hostsProvider).requireValue, isEmpty);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));
}
