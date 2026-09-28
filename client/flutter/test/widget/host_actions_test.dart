import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

void main() {
  for (final action in ['row', 'icon', 'menu']) {
    testWidgets('$action connects to the host exactly once without opening its editor', (tester) async {
      final backend = testBackend();
      addTearDown(backend.dispose);
      await backend.debugSignInDemoAndUnlock();
      await pumpApp(tester, backend);
      await enterKey(tester, 'hosts-search', 'staging-web');
      final button = find.byKey(const ValueKey('connect-staging-web'));
      expect(tester.widget(button), isA<GlassIconButton>());
      expect(find.descendant(of: button, matching: find.byType(Text)), findsNothing);
      expect(tester.widget<GlassIconButton>(button).tooltip, 'Connect');
      final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
      if (action == 'row') {
        await tapKey(tester, 'host-staging-web');
      } else if (action == 'icon') {
        await tapKey(tester, 'connect-staging-web');
      } else {
        await tapKey(tester, 'host-menu-staging-web');
        expect(container.read(terminalTabsProvider).tabs, isEmpty, reason: 'menu does not trigger the row');
        expect(
          tester.getTopLeft(find.byKey(const ValueKey('host-connect-menu-staging-web'))).dy,
          lessThan(tester.getTopLeft(find.text('Open SFTP')).dy),
        );
        await tapKey(tester, 'host-connect-menu-staging-web');
      }
      expect(container.read(routerProvider).state.uri.path, AppRoutes.terminal);
      expect(container.read(terminalTabsProvider).tabs, hasLength(1));
      expect(find.byKey(const ValueKey('host-key-prompt')), findsOneWidget);
      expect(find.byKey(const ValueKey('save-host')), findsNothing);
      await tapKey(tester, 'accept-host-key');
      expect(container.read(terminalTabsProvider).tabs, hasLength(1));
      expect(tester.takeException(), isNull);
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
  }

  testWidgets('group picker searches nested paths, changes membership and can remove it at Russian 140%', (
    tester,
  ) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await backend.services.settings.updateLocal(backend.services.settings.currentLocal.copyWith(uiFontScale: 1.4));
    await pumpApp(tester, backend, locale: AppLocale.ru, size: const Size(1024, 720));
    final before = backend.inventory.currentHosts.firstWhere((h) => h.name == 'bastion-a');
    final target = backend.cloud.activeData!.groups.firstWhere((g) => g.parentId != null);
    final parent = backend.cloud.activeData!.groups.firstWhere((g) => g.id == target.parentId);
    await enterKey(tester, 'hosts-search', before.name);
    await tapKey(tester, 'host-menu-${before.name}');
    await tapKey(tester, 'host-group-menu-${before.name}');
    expect(tester.takeException(), isNull);
    expect(tester.widget<ListTile>(find.byKey(const ValueKey('host-group-choice-none'))).selected, isTrue);
    await enterKey(tester, 'host-group-search', '${parent.name} › ${target.name}');
    expect(find.byKey(const ValueKey('host-group-choice-none')), findsNothing);
    await tapKey(tester, 'host-group-choice-${target.id.value}');
    final saved = backend.inventory.currentHosts.firstWhere((h) => h.id == before.id);
    expect(saved.groupId, target.id);
    final originalFields = hostToJson(before)
      ..remove('group_id')
      ..remove('updated_at_ms');
    final savedFields = hostToJson(saved)
      ..remove('group_id')
      ..remove('updated_at_ms');
    expect(savedFields, originalFields, reason: 'credentials, overrides and metadata must survive the move');
    expect(find.byKey(const ValueKey('host-group-picker')), findsNothing);

    await tapKey(tester, 'host-menu-${before.name}');
    await tapKey(tester, 'host-group-menu-${before.name}');
    await enterKey(tester, 'host-group-search', target.name);
    expect(tester.widget<ListTile>(find.byKey(ValueKey('host-group-choice-${target.id.value}'))).selected, isTrue);
    await enterKey(tester, 'host-group-search', 'no matching group');
    expect(find.text('Группы не найдены'), findsOneWidget);
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await settle(tester);
    expect(backend.inventory.currentHosts.firstWhere((h) => h.id == before.id).groupId, target.id);

    await tapKey(tester, 'host-menu-${before.name}');
    await tapKey(tester, 'host-group-menu-${before.name}');
    await tapKey(tester, 'host-group-choice-none');
    expect(backend.inventory.currentHosts.firstWhere((h) => h.id == before.id).groupId, isNull);
    final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    expect(container.read(terminalTabsProvider).tabs, isEmpty, reason: 'group actions do not connect');
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('host menu hides grouping when there are no groups and editing remains available', (tester) async {
    final backend = testBackend(seed: false);
    addTearDown(backend.dispose);
    await backend.debugCreateUnlockedLocalProfile();
    await backend.inventory.saveHost(Host.create(name: 'new-host', address: 'example.test'));
    await pumpApp(tester, backend);
    await tapKey(tester, 'host-menu-new-host');
    expect(find.byKey(const ValueKey('host-group-menu-new-host')), findsNothing);
    expect(find.byKey(const ValueKey('host-connect-menu-new-host')), findsOneWidget);
    expect(find.text('Open SFTP'), findsOneWidget);
    expect(find.text('Delete'), findsOneWidget);
    await tapKey(tester, 'host-edit-menu-new-host');
    expect(find.byKey(const ValueKey('save-host')), findsOneWidget);
    final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    expect(container.read(terminalTabsProvider).tabs, isEmpty);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
}
