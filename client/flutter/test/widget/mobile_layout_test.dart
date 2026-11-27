import 'dart:ui' show PointerDeviceKind;

import 'package:consolecrypt/ai/terminal_attachment.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/widgets/workspace_draggable.dart';
import 'package:consolecrypt/sftp/dialogs/quick_look_dialog.dart';
import 'package:consolecrypt/sftp/sftp_controller.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

void phoneTest(String name, WidgetTesterCallback body) =>
    testWidgets(name, body, variant: TargetPlatformVariant.only(TargetPlatform.android));

void main() {
  phoneTest('tablet rows scroll with a finger and drag with a mouse', (tester) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(800, 600);
    addTearDown(tester.view.reset);
    final scroll = ScrollController();
    addTearDown(scroll.dispose);
    int? accepted;
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: ListView.builder(
            controller: scroll,
            itemExtent: 60,
            itemCount: 40,
            itemBuilder: (_, index) => DragTarget<int>(
              onAcceptWithDetails: (details) => accepted = details.data,
              builder: (_, candidates, rejected) => WorkspaceDraggable<int>(
                data: index,
                feedback: const SizedBox.square(dimension: 30),
                child: ColoredBox(color: Colors.grey, child: Text('row $index')),
              ),
            ),
          ),
        ),
      ),
    );
    await tester.dragFrom(const Offset(100, 200), const Offset(0, -120));
    await settle(tester);
    expect(scroll.offset, greaterThan(0));
    expect(accepted, isNull);
    scroll.jumpTo(0);
    await tester.pump();
    final mouse = await tester.startGesture(const Offset(100, 30), kind: PointerDeviceKind.mouse);
    await mouse.moveBy(const Offset(0, 30));
    await tester.pump();
    await mouse.moveTo(const Offset(100, 150));
    await tester.pump();
    await mouse.up();
    await tester.pump();
    expect(accepted, 0);
  });

  phoneTest('Android: resizing a connected terminal switches chrome and keeps the session', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend, size: const Size(390, 844));
    final c = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    final session = await c.read(terminalTabsProvider.notifier).open(backend.inventory.currentHosts.first);
    final router = c.read(routerProvider);
    router.go(AppRoutes.terminal);
    await settle(tester);
    final phoneNav = find.byKey(const ValueKey('mobile-navigation'));
    final toolRail = find.byKey(const ValueKey('workspace-tools-rail'));
    final keyboard = find.byKey(const ValueKey('terminal-keyboard'));
    expect(phoneNav, findsOneWidget);
    session.terminal.write('resize-marker');
    expect(session.terminal.buffer.getText(), contains('resize-marker'));
    for (final size in [const Size(600, 900), const Size(1440, 900), const Size(390, 844)]) {
      tester.view.physicalSize = size * tester.view.devicePixelRatio;
      await settle(tester);
      final compact = size.width == 390;
      expect(phoneNav, compact ? findsOneWidget : findsNothing);
      expect(toolRail, compact ? findsNothing : findsOneWidget);
      expect(keyboard, findsOneWidget);
      expect(c.read(terminalTabsProvider).active, same(session));
      expect(session.terminal.buffer.getText(), contains('resize-marker'));
      expect(tester.takeException(), isNull);
    }
  });

  phoneTest('Android: a draft stays editable across tablet and phone layouts', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend, size: const Size(768, 1024));
    final c = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    c.read(routerProvider).go(AppRoutes.newHost);
    await settle(tester);
    await enterKey(tester, 'host-name', 'unsaved-host');
    for (final size in [const Size(460, 800), const Size(1280, 800)]) {
      tester.view.physicalSize = size;
      await settle(tester);
      final field = find.descendant(of: find.byKey(const ValueKey('host-name')), matching: find.byType(EditableText));
      expect(tester.widget<EditableText>(field).controller.text, 'unsaved-host');
      expect(tester.takeException(), isNull);
    }
  });

  phoneTest('Android: selected text opens the terminal menu and an AI attachment fits above the keyboard', (
    tester,
  ) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend, locale: AppLocale.ru, size: const Size(360, 800));
    final c = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    final tab = await c
        .read(terminalTabsProvider.notifier)
        .open(backend.inventory.currentHosts.firstWhere((h) => h.name == 'raspberry-pi'));
    c.read(routerProvider).go(AppRoutes.terminal);
    await settle(tester);
    tab.terminal.write('\x1b[2J\x1b[Hfirst line\r\nsecond line\r\nthird line');
    final buffer = tab.terminal.buffer;
    tab.controller.setSelection(
      buffer.createAnchor(0, buffer.absoluteCursorY - 2),
      buffer.createAnchor(10, buffer.absoluteCursorY),
    );
    await tapKey(tester, 'terminal-context-menu');
    expect(tester.takeException(), isNull, reason: 'menu before opening AI');
    await tapKey(tester, 'terminal-menu-ask-ai');
    expect(c.read(terminalAttachmentProvider)?.text, contains('first line'));
    expect(tester.takeException(), isNull, reason: 'AI before keyboard');
    await enterKey(tester, 'ai-input', 'Объясни вывод');
    tester.view.viewInsets = const FakeViewPadding(bottom: 320);
    await settle(tester);
    expect(find.byKey(const ValueKey('ai-terminal-attachment')), findsOneWidget);
  });

  phoneTest('Android: numbered SFTP preview fits 360px and desktop editor preferences stay hidden', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend, locale: AppLocale.ru, size: const Size(360, 800));
    final context = tester.element(find.byType(Navigator).first);
    final c = ProviderScope.containerOf(context);
    await c
        .read(sftpControllerProvider.notifier)
        .connect(backend.inventory.currentHosts.firstWhere((h) => h.name == 'prod-web-1'));
    c.read(routerProvider).go(AppRoutes.sftp);
    await settle(tester);
    await tester.tap(find.text('notes.txt'));
    await settle(tester);
    final preview = find.byType(QuickLookDialog);
    expect(preview, findsOneWidget);
    final bounds = tester.getRect(find.byKey(const ValueKey('sftp-preview-vertical')));
    expect(bounds.left, greaterThanOrEqualTo(0));
    expect(bounds.right, lessThanOrEqualTo(360));
    expect(find.byKey(const ValueKey('sftp-preview-line-numbers')), findsOneWidget);
    await tapKey(tester, 'sftp-quicklook-edit');
    expect(find.byKey(const ValueKey('/home/deploy/notes.txt')), findsOneWidget);
    c.read(routerProvider).go(AppRoutes.settings);
    await settle(tester);
    expect(find.byKey(const ValueKey('sftp-default-editor-choose')), findsNothing);
    expect(tester.takeException(), isNull);
  });

  for (final width in [360.0, 412.0]) {
    testWidgets('Phone $width: navigation, tools, editor and settings fit phone', (tester) async {
      final backend = testBackend();
      addTearDown(backend.dispose);
      await backend.debugSignInDemoAndUnlock();
      await pumpApp(tester, backend, locale: AppLocale.ru, size: Size(width, 915));
      expect(find.byKey(const ValueKey('mobile-navigation')), findsOneWidget);
      expect(find.byKey(const ValueKey('workspace-tools-rail')), findsNothing);
      expect(tester.takeException(), isNull, reason: 'hosts');
      final c = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
      final layoutErrors = <String>[];
      for (final route in [
        AppRoutes.groups,
        AppRoutes.newHost,
        AppRoutes.credentials,
        AppRoutes.knownHosts,
        AppRoutes.terminal,
        AppRoutes.sftp,
        AppRoutes.devices,
        AppRoutes.sharing,
        AppRoutes.rdp,
        AppRoutes.kubernetes,
        AppRoutes.databases,
        AppRoutes.docker,
        AppRoutes.tcpdump,
        AppRoutes.portCheck,
        AppRoutes.passwords,
        AppRoutes.externalVault,
        AppRoutes.automation,
        AppRoutes.tunnels,
        AppRoutes.settings,
        AppRoutes.backups,
        AppRoutes.sync,
      ]) {
        c.read(routerProvider).go(route);
        await settle(tester);
        if (tester.takeException() case final error?) layoutErrors.add('$route: $error');
      }
      expect(layoutErrors, isEmpty);
      await tapKey(tester, 'mobile-ai');
      expect(tester.takeException(), isNull, reason: 'AI');
      await enterKey(tester, 'ai-input', 'Черновик');
      tester.view.viewInsets = const FakeViewPadding(bottom: 320);
      await settle(tester);
      expect(tester.takeException(), isNull, reason: 'AI with keyboard');
      tester.view.viewInsets = FakeViewPadding.zero;
      await tapKey(tester, 'nav-snippets');
      expect(tester.takeException(), isNull, reason: 'snippets');
      await tapKey(tester, 'nav-ai');
      expect(tester.widget<TextField>(find.byKey(const ValueKey('ai-input'))).controller!.text, 'Черновик');
      await tester.binding.handlePopRoute();
      await settle(tester);
      expect(find.byKey(const ValueKey('workspace-tools-panel')), findsNothing);
      expect(tester.takeException(), isNull);
    }, variant: const TargetPlatformVariant({TargetPlatform.android, TargetPlatform.iOS}));
  }

  phoneTest('Android: terminal control keys and navigation keep the session', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend, size: const Size(412, 915));
    await enterKey(tester, 'hosts-search', 'staging-web');
    await tapKey(tester, 'connect-staging-web');
    await tapKey(tester, 'accept-host-key');
    expect(tester.takeException(), isNull, reason: 'terminal');
    final c = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    final tab = c.read(terminalTabsProvider).active!;
    final outputs = <String>[];
    tab.terminal.onOutput = outputs.add;
    await tapKey(tester, 'terminal-key-Ctrl+C');
    expect(outputs, contains('\x03'));
    tester.view.viewInsets = const FakeViewPadding(bottom: 320);
    await settle(tester);
    expect(find.byKey(const ValueKey('mobile-navigation')), findsNothing);
    expect(tester.takeException(), isNull, reason: 'terminal with keyboard');
    tester.view.viewInsets = FakeViewPadding.zero;
    await tapKey(tester, 'mobile-ai');
    await tapKey(tester, 'workspace-tools-close');
    expect(c.read(terminalTabsProvider).active, same(tab));
    expect(tester.takeException(), isNull);
  });

  phoneTest('Android: connected SFTP uses tappable rows and phone toolbar', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend, locale: AppLocale.ru, size: const Size(360, 800));
    final c = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    await c
        .read(sftpControllerProvider.notifier)
        .connect(backend.inventory.currentHosts.firstWhere((h) => h.name == 'prod-web-1'));
    c.read(routerProvider).go(AppRoutes.sftp);
    await settle(tester);
    expect(find.byKey(const ValueKey('sftp-mobile-files')), findsOneWidget);
    expect(find.byKey(const ValueKey('sftp-toolbar-local-pane')), findsNothing);
    expect(tester.takeException(), isNull);
    await tapKey(tester, 'sftp-toolbar-actions');
    expect(find.byKey(const ValueKey('sftp-action-openWith')), findsNothing);
    expect(tester.takeException(), isNull);
  });

  phoneTest('Android: welcome and unlock fit the phone and keyboard', (tester) async {
    final backend = testBackend(seed: false);
    addTearDown(backend.dispose);
    await pumpApp(tester, backend, locale: AppLocale.ru, size: const Size(360, 800));
    expect(tester.takeException(), isNull, reason: 'welcome');
    await backend.debugCreateUnlockedLocalProfile();
    await settle(tester);
    await backend.services.vault.lock();
    await settle(tester);
    tester.view.viewInsets = const FakeViewPadding(bottom: 320);
    await settle(tester);
    expect(tester.takeException(), isNull, reason: 'unlock');
  });
}
