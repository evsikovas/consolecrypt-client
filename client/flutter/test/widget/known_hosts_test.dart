import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/hosts/known_hosts_screen.dart';
import 'package:consolecrypt/settings/settings_screen.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

void main() {
  testWidgets('known hosts open from sidebar, leave Settings and retain confirmed removal', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    final before = List<KnownHost>.of(backend.cloud.activeData!.knownHosts);
    final host = before.first;
    await pumpApp(tester, backend);
    await tapKey(tester, 'nav-knownHosts');
    final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    expect(container.read(routerProvider).state.uri.path, AppRoutes.knownHosts);
    expect(tester.widget<GlassSidebarItem>(find.byKey(const ValueKey('nav-knownHosts'))).selected, isTrue);
    expect(find.byType(KnownHostsScreen), findsOneWidget);
    expect(find.text(host.hostPattern), findsOneWidget);
    expect(find.text('${host.keyType} ${host.fingerprintSha256}'), findsOneWidget);
    expect(find.textContaining('Accepted on first connect'), findsWidgets);

    await tapKey(tester, 'remove-known-host-${host.id.value}');
    expect(find.text('The next connection will ask you to verify the host key again.'), findsOneWidget);
    await tapKey(tester, 'confirm-cancel');
    expect(backend.cloud.activeData!.knownHosts, hasLength(before.length));
    await tapKey(tester, 'remove-known-host-${host.id.value}');
    await tapKey(tester, 'confirm-ok');
    final after = backend.cloud.activeData!.knownHosts;
    expect(after, hasLength(before.length - 1));
    expect(after.any((k) => k.id == host.id), isFalse);
    expect(find.byKey(ValueKey('known-host-${host.id.value}')), findsNothing);

    await tapKey(tester, 'nav-settings');
    expect(find.byType(SettingsScreen), findsOneWidget);
    expect(find.descendant(of: find.byType(SettingsScreen), matching: find.text('Known hosts')), findsNothing);
    expect(
      find.descendant(
        of: find.byType(SettingsScreen),
        matching: find.text('${host.keyType} ${host.fingerprintSha256}'),
      ),
      findsNothing,
    );
    await tapKey(tester, 'nav-knownHosts');
    expect(find.byType(KnownHostsScreen), findsOneWidget);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('local profile has the new page and empty state at Russian 140%, guarded when locked', (tester) async {
    final backend = testBackend(seed: false);
    addTearDown(backend.dispose);
    await backend.debugCreateUnlockedLocalProfile();
    await backend.services.settings.updateLocal(backend.services.settings.currentLocal.copyWith(uiFontScale: 1.4));
    await pumpApp(tester, backend, locale: AppLocale.ru, size: const Size(1024, 720));
    await tapKey(tester, 'nav-knownHosts');
    expect(find.text('Известных хостов пока нет.'), findsOneWidget);
    expect(find.byType(KnownHostsScreen), findsOneWidget);
    expect(tester.takeException(), isNull);
    final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    await backend.services.vault.lock();
    await settle(tester);
    container.read(routerProvider).go(AppRoutes.knownHosts);
    await settle(tester);
    expect(container.read(routerProvider).state.uri.path, AppRoutes.unlock);
    expect(find.byType(KnownHostsScreen), findsNothing);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
}
