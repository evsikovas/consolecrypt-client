import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

void main() {
  testWidgets('group browser shows nested hosts, scopes search, and preserves actions in both views', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);
    await tapKey(tester, 'nav-groups');
    expect(find.byKey(const ValueKey('inventory-group-Production')), findsOneWidget);
    expect(find.byKey(const ValueKey('host-prod-db-1')), findsNothing);
    await tapKey(tester, 'inventory-group-Production');
    expect(find.byKey(const ValueKey('inventory-group-Databases')), findsOneWidget);
    expect(find.byKey(const ValueKey('host-prod-db-1')), findsOneWidget);
    expect(find.byKey(const ValueKey('host-staging-web')), findsNothing);
    expect(find.text('not set'), findsNothing, reason: 'settings no longer displace the actual hosts');
    await enterKey(tester, 'hosts-search', 'primary');
    expect(find.byKey(const ValueKey('host-prod-db-1')), findsOneWidget);
    expect(find.byKey(const ValueKey('host-prod-db-2')), findsNothing);
    await tapKey(tester, 'inventory-view-list');
    expect(tester.widget(find.byKey(const ValueKey('host-prod-db-1'))), isA<ListTile>());
    await tapKey(tester, 'host-menu-prod-db-1');
    expect(find.text('Open SFTP'), findsOneWidget);
    expect(find.byKey(const ValueKey('host-group-menu-prod-db-1')), findsOneWidget);
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await settle(tester);
    await tapKey(tester, 'inventory-view-cards');
    await tapKey(tester, 'host-prod-db-1');
    final c = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    expect(c.read(terminalTabsProvider).tabs, hasLength(1));
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('new host inherits the open group and new subgroup keeps its parent', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);
    final db = backend.cloud.activeData!.groups.firstWhere((g) => g.name == 'Databases');
    await tapKey(tester, 'group-Databases');
    await tapKey(tester, 'add-host');
    await enterKey(tester, 'host-name', 'new-database');
    await enterKey(tester, 'host-address', 'database.example.test');
    expect(find.byKey(const ValueKey('host-password')), findsNothing, reason: 'uses inherited credential');
    await tapKey(tester, 'save-host');
    final saved = backend.inventory.currentHosts.firstWhere((h) => h.name == 'new-database');
    expect(saved.groupId, db.id);
    expect(saved.promptsForPassword, isFalse);
    expect(saved.credentialId, isNull, reason: 'inheritance remains linked to the group');
    await tapKey(tester, 'add-group');
    await enterKey(tester, 'group-name', 'Analytics');
    await tapKey(tester, 'save-group');
    expect(backend.cloud.activeData!.groups.firstWhere((g) => g.name == 'Analytics').parentId, db.id);
    expect(find.byKey(const ValueKey('inventory-group-Analytics')), findsOneWidget);
    await tapKey(tester, 'inventory-group-settings');
    expect(find.byKey(const ValueKey('group-dialog')), findsOneWidget);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('tree collapse, ungrouped scope, group search and deletion keep hosts reachable', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);
    await tapKey(tester, 'inventory-expand-Production');
    expect(find.byKey(const ValueKey('group-Databases')), findsNothing);
    await tapKey(tester, 'inventory-nav-ungrouped');
    expect(find.byKey(const ValueKey('host-bastion-a')), findsOneWidget);
    expect(find.byKey(const ValueKey('host-prod-db-1')), findsNothing);
    await tapKey(tester, 'inventory-nav-groups');
    await enterKey(tester, 'hosts-search', 'Production Databases');
    expect(find.byKey(const ValueKey('inventory-group-Databases')), findsOneWidget);
    await tapKey(tester, 'inventory-group-Databases');
    final before = backend.inventory.currentHosts.length;
    await tapKey(tester, 'inventory-delete-group');
    await tapKey(tester, 'confirm-ok');
    expect(backend.inventory.currentHosts.length, before);
    expect(find.byKey(const ValueKey('host-prod-db-1')), findsOneWidget);
    expect(backend.cloud.activeData!.groups.any((g) => g.name == 'Databases'), isFalse);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('Russian 140% uses compact group selector without overflow and opens settings', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await backend.services.settings.updateLocal(backend.services.settings.currentLocal.copyWith(uiFontScale: 1.4));
    await pumpApp(tester, backend, locale: AppLocale.ru, size: const Size(1024, 720));
    expect(find.byKey(const ValueKey('inventory-location')), findsOneWidget);
    await tapKey(tester, 'inventory-location');
    await tapKey(tester, 'inventory-location-Production');
    await enterKey(tester, 'hosts-search', 'prod-db-1');
    expect(find.byKey(const ValueKey('host-prod-db-1')), findsOneWidget);
    expect(tester.takeException(), isNull);
    await tapKey(tester, 'inventory-view-list');
    expect(tester.takeException(), isNull);
    await tapKey(tester, 'inventory-group-settings');
    expect(find.byKey(const ValueKey('group-dialog')), findsOneWidget);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('empty group provides creation actions and jump profiles remain available', (tester) async {
    final backend = testBackend(seed: false);
    addTearDown(backend.dispose);
    await backend.debugCreateUnlockedLocalProfile();
    await backend.inventory.saveGroup(Group.create('Empty'));
    await pumpApp(tester, backend);
    await tapKey(tester, 'inventory-group-Empty');
    expect(find.byKey(const ValueKey('add-host')), findsOneWidget);
    await tapKey(tester, 'inventory-jump-profiles');
    expect(find.text('Reusable ordered chains of jump hosts.'), findsWidgets);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
}
