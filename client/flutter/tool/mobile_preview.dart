// Headless Android layout previews. Uses mock data, never the user's vault.
import 'dart:io';
import 'dart:ui' as ui;

import 'package:consolecrypt/app/app.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/app/workspace_tools_controller.dart';
import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/sftp/sftp_controller.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  setUpAll(() async {
    final fonts = Platform.environment['CC_PREVIEW_FONTS'] ?? '/private/tmp/cc-android-tools';
    for (final font in [('Roboto', 'Roboto'), ('monospace', 'RobotoMono')]) {
      await (FontLoader(
        font.$1,
      )..addFont(File('$fonts/${font.$2}.ttf').readAsBytes().then(ByteData.sublistView))).load();
    }
    await (FontLoader('MaterialIcons')..addFont(rootBundle.load('fonts/MaterialIcons-Regular.otf'))).load();
    Directory('build/mobile-preview').createSync(recursive: true);
  });
  for (final screen in ['hosts', 'terminal', 'ai', 'snippets', 'sftp', 'welcome', 'screen-capture']) {
    testWidgets('phone $screen', (tester) async {
      if (screen == 'screen-capture') {
        const captureChannel = MethodChannel('consolecrypt/screen_capture');
        final messenger = TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
        messenger.setMockMethodCallHandler(captureChannel, (_) async => true);
        addTearDown(() => messenger.setMockMethodCallHandler(captureChannel, null));
      }
      tester.view.physicalSize = const Size(412, 915);
      tester.view.devicePixelRatio = 1;
      tester.view.padding = const FakeViewPadding(top: 28, bottom: 24);
      addTearDown(tester.view.reset);
      final backend = MockBackend(config: MockConfig.test(seedDemoData: screen != 'welcome'));
      addTearDown(backend.dispose);
      if (screen != 'welcome') await backend.debugSignInDemoAndUnlock();
      await backend.services.settings.updateLocal(
        backend.services.settings.currentLocal.copyWith(
          appLocale: AppLocale.ru,
          themeMode: AppThemeMode.dark,
          terminalColorScheme: TerminalColorScheme.ocean,
        ),
      );
      final key = GlobalKey();
      await tester.pumpWidget(
        RepaintBoundary(
          key: key,
          child: ProviderScope(
            overrides: [appServicesProvider.overrideWithValue(backend.services)],
            retry: (_, _) => null,
            child: const ConsoleCryptApp(),
          ),
        ),
      );
      Future<void> settle() async {
        for (var i = 0; i < 14; i++) {
          await tester.pump(const Duration(milliseconds: 100));
        }
      }

      await settle();
      final c = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
      if (screen == 'terminal') {
        final host = backend.inventory.currentHosts.firstWhere((h) => h.name == 'staging-web');
        final tab = await c.read(terminalTabsProvider.notifier).open(host);
        c.read(routerProvider).go(AppRoutes.terminal);
        await settle();
        await c.read(terminalTabsProvider.notifier).answerHostKey(tab, HostKeyDecision.acceptAndSave);
        await settle();
        tab.terminal.write(
          '\r\n\x1b[32mdeploy@staging\x1b[0m:~\$ uptime\r\n 12:48 up 21 days, load: 0.12, 0.08, 0.05\r\n\r\n\x1b[32mdeploy@staging\x1b[0m:~\$ ',
        );
      } else if (screen == 'ai' || screen == 'snippets') {
        c.read(workspaceToolsProvider.notifier).open(screen == 'ai' ? WorkspaceTool.ai : WorkspaceTool.snippets);
      } else if (screen == 'screen-capture') {
        c.read(routerProvider).go(AppRoutes.settings);
        await settle();
        await tester.ensureVisible(find.byKey(const ValueKey('screen-capture-settings')));
      } else if (screen == 'sftp') {
        await c
            .read(sftpControllerProvider.notifier)
            .connect(backend.inventory.currentHosts.firstWhere((h) => h.name == 'prod-web-1'));
        c.read(routerProvider).go(AppRoutes.sftp);
      }
      await settle();
      expect(tester.takeException(), isNull);
      final boundary = key.currentContext!.findRenderObject()! as RenderRepaintBoundary;
      await tester.runAsync(() async {
        final img = await boundary.toImage(pixelRatio: 2);
        final bytes = await img.toByteData(format: ui.ImageByteFormat.png);
        await File('build/mobile-preview/$screen.png').writeAsBytes(bytes!.buffer.asUint8List());
        img.dispose();
      });
    }, variant: TargetPlatformVariant.only(TargetPlatform.android));
  }
}
