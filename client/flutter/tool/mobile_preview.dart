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
import 'package:xterm/xterm.dart';

import 'guide_capture_support.dart';

const _guide = bool.fromEnvironment('CC_GUIDE_CAPTURE');
const _guideOutput = String.fromEnvironment('CC_GUIDE_OUTPUT', defaultValue: '../../.local/verification');

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  setUpAll(() async {
    final fonts = Platform.environment['CC_PREVIEW_FONTS'] ?? '/private/tmp/cc-android-tools';
    for (final font in [('Roboto', 'Roboto'), ('monospace', 'RobotoMono')]) {
      final path = _guide
          ? font.$1 == 'Roboto'
                ? '/opt/homebrew/share/flutter/bin/cache/artifacts/material_fonts/Roboto-Regular.ttf'
                : '/System/Library/Fonts/SFNSMono.ttf'
          : '$fonts/${font.$2}.ttf';
      await (FontLoader(font.$1)..addFont(File(path).readAsBytes().then(ByteData.sublistView))).load();
    }
    await (FontLoader('MaterialIcons')..addFont(rootBundle.load('fonts/MaterialIcons-Regular.otf'))).load();
    Directory(_guide ? _guideOutput : 'build/mobile-preview').createSync(recursive: true);
  });
  for (final locale in _guide ? [AppLocale.ru, AppLocale.en] : [AppLocale.ru]) {
    for (final screen
        in _guide
            ? ['terminal', 'screen-capture']
            : ['hosts', 'terminal', 'ai', 'snippets', 'sftp', 'welcome', 'screen-capture']) {
      testWidgets('phone ${locale.wireName} $screen', (tester) async {
        var allowed = true;
        if (screen == 'screen-capture') {
          const captureChannel = MethodChannel('consolecrypt/screen_capture');
          final messenger = TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
          messenger.setMockMethodCallHandler(captureChannel, (call) async {
            if (call.method == 'setAllowed') allowed = (call.arguments as Map)['allowed'] as bool;
            return allowed;
          });
          addTearDown(() => messenger.setMockMethodCallHandler(captureChannel, null));
        }
        tester.view.physicalSize = const Size(412, 915);
        tester.view.devicePixelRatio = 1;
        tester.view.padding = const FakeViewPadding(top: 28, bottom: 24);
        addTearDown(tester.view.reset);
        final backend = MockBackend(config: MockConfig.test(seedDemoData: screen != 'welcome'));
        addTearDown(backend.dispose);
        if (screen != 'welcome') await backend.debugSignInDemoAndUnlock();
        if (_guide && screen != 'welcome') await prepareGuideInventory(backend, screen);
        await backend.services.settings.updateLocal(
          backend.services.settings.currentLocal.copyWith(
            appLocale: locale,
            themeMode: AppThemeMode.dark,
            terminalColorScheme: TerminalColorScheme.ocean,
          ),
        );
        final key = GlobalKey();
        await tester.pumpWidget(
          RepaintBoundary(
            key: key,
            child: ProviderScope(
              overrides: [
                appServicesProvider.overrideWithValue(backend.services),
                if (_guide) developerControlsProvider.overrideWithValue(null),
              ],
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
          if (_guide) expect(tab.isConnected, isTrue);
          tab.terminal.write(
            '\r\n\x1b[32mdeploy@staging\x1b[0m:~\$ uptime\r\n 12:48 up 21 days, load: 0.12, 0.08, 0.05\r\n\r\n\x1b[32mdeploy@staging\x1b[0m:~\$ ',
          );
        } else if (screen == 'ai' || screen == 'snippets') {
          c.read(workspaceToolsProvider.notifier).open(screen == 'ai' ? WorkspaceTool.ai : WorkspaceTool.snippets);
        } else if (screen == 'screen-capture') {
          c.read(routerProvider).go(AppRoutes.settings);
          await settle();
          await tester.ensureVisible(find.byKey(const ValueKey('screen-capture-settings')));
          if (_guide) {
            await settle();
            final toggle = find.byKey(const ValueKey('screen-capture-switch'));
            expect(tester.widget<SwitchListTile>(toggle).value, isTrue);
            await tester.tap(toggle);
            await settle();
            expect(allowed, isFalse);
            expect(tester.widget<SwitchListTile>(toggle).value, isFalse);
            await tester.tap(toggle);
            await settle();
            expect(allowed, isTrue);
          }
        } else if (screen == 'sftp') {
          await c
              .read(sftpControllerProvider.notifier)
              .connect(backend.inventory.currentHosts.firstWhere((h) => h.name == 'prod-web-1'));
          c.read(routerProvider).go(AppRoutes.sftp);
        }
        await settle();
        expect(tester.takeException(), isNull);
        if (_guide) {
          final text = tester.widgetList<Text>(find.byType(Text)).map((widget) => widget.data ?? '').join('\n');
          expect(text, isNot(contains('PRIVATE KEY')));
          expect(text, isNot(contains('correct horse battery staple')));
          expect(text, isNot(contains('demo-password-1')));
          if (screen == 'terminal') {
            expect(find.byType(TerminalView), findsOneWidget);
            expect(c.read(terminalTabsProvider).active!.terminal.buffer.getText(), contains('load: 0.12, 0.08, 0.05'));
          } else {
            expect(find.byKey(const ValueKey('screen-capture-macos-info')), findsNothing);
            expect(find.byKey(const ValueKey('screen-capture-error')), findsNothing);
            expect(tester.widget<SwitchListTile>(find.byKey(const ValueKey('screen-capture-switch'))).value, isTrue);
          }
        }
        final boundary = key.currentContext!.findRenderObject()! as RenderRepaintBoundary;
        await tester.runAsync(() async {
          final img = await boundary.toImage(pixelRatio: 2);
          final bytes = await img.toByteData(format: ui.ImageByteFormat.png);
          final path = _guide
              ? '$_guideOutput/guide-mobile-$screen-${locale.wireName}.png'
              : 'build/mobile-preview/$screen.png';
          await File(path).writeAsBytes(bytes!.buffer.asUint8List());
          img.dispose();
        });
      }, variant: TargetPlatformVariant.only(TargetPlatform.android));
    }
  }
}
