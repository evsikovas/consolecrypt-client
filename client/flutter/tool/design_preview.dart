// macOS-only visual QA with OS fonts (never copied into the repository).
// flutter test tool/design_preview.dart
import 'dart:io';
import 'dart:ui' as ui;

import 'package:consolecrypt/app/app.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/models/sftp_edit.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/snippets/starter_catalog.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:xterm/xterm.dart';

import '../test/helpers/test_app.dart' show tapKey, settle, pickHost;

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  setUpAll(() async {
    for (final family in [
      '.SF Pro Text',
      '.SF Pro Display',
      '.AppleSystemUIFont',
      'CupertinoSystemText',
      'CupertinoSystemDisplay',
      'Roboto',
    ]) {
      final loader = FontLoader(family)
        ..addFont(File('/System/Library/Fonts/SFNS.ttf').readAsBytes().then(ByteData.sublistView));
      await loader.load();
    }
    final mono = FontLoader('Menlo')
      ..addFont(File('/System/Library/Fonts/Menlo.ttc').readAsBytes().then(ByteData.sublistView));
    await mono.load();
    final icons = FontLoader('MaterialIcons')..addFont(rootBundle.load('fonts/MaterialIcons-Regular.otf'));
    await icons.load();
    await Directory('build/design-preview').create(recursive: true);
  });
  for (final theme in [AppThemeMode.dark, AppThemeMode.light]) {
    for (final screen in [
      'hosts',
      'sftp-settings',
      'sftp-preview',
      'terminal-menu',
      'terminal-menu-compact',
      'terminal-menu-ai',
      'terminal-menu-snippet',
      'inventory-groups',
      'inventory-production',
      'inventory-list',
      'inventory-compact',
      'known-hosts',
      'host-menu',
      'host-groups',
      'workspace-snippets',
      'snippet-packages',
      'snippet-catalog',
      'workspace-ai',
      'workspace-ai-expanded',
      'workspace-snippets-expanded',
      'workspace-ai-expanded-compact',
      'panel-mode-settings',
      'workspace-ai-selection',
      'welcome',
      'appearance',
      'vault-settings',
      'terminal',
      'palette',
      'spectrum',
      'resume',
      'glass-clear',
      'glass-default',
      'glass-tinted',
      'glass-solid',
    ]) {
      final welcome = screen == 'welcome';
      final glass = screen.startsWith('glass-');
      final personalized = glass || ['appearance', 'terminal', 'palette', 'spectrum'].contains(screen);
      testWidgets('${theme.name} $screen', (tester) async {
        tester.view.physicalSize = screen.endsWith('-compact') ? const Size(1024, 720) : const Size(1440, 960);
        tester.view.devicePixelRatio = 1;
        addTearDown(tester.view.reset);
        final backend = MockBackend(config: MockConfig.test(seedDemoData: !welcome));
        addTearDown(backend.dispose);
        if (!welcome) await backend.debugSignInDemoAndUnlock();
        if (screen.startsWith('snippet-')) {
          for (final package in starterSnippetPackages(lookupAppLocalizations(const Locale('ru')))) {
            for (final snippet in package.snippets) {
              await backend.snippets.saveSnippet(snippet);
            }
          }
        }
        if (screen == 'resume') {
          backend.cloud.lockActive();
          backend.cloud.publishProfiles(clearActive: true);
        }
        final settings = backend.services.settings;
        await settings.updateLocal(
          settings.currentLocal.copyWith(
            themeMode: theme,
            appLocale: AppLocale.ru,
            uiAccentColor: screen.startsWith('terminal-menu')
                ? 0x4B89FF
                : personalized
                ? 0x7C5CFC
                : null,
            uiBackgroundColor: personalized ? 0x16324F : null,
            uiFontScale: screen.endsWith('-compact') ? 1.4 : 1,
            workspacePanelStyle: screen.contains('expanded') || screen == 'panel-mode-settings'
                ? WorkspacePanelStyle.expanded
                : WorkspacePanelStyle.floating,
            terminalColorScheme: TerminalColorScheme.ocean,
            sftpDefaultEditor: screen == 'sftp-settings'
                ? const AppRef(AppRefKind.path, '/Applications/Visual Studio Code.app')
                : null,
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
        for (var i = 0; i < 20; i++) {
          await tester.pump(const Duration(milliseconds: 100));
        }
        if (screen.startsWith('terminal-menu')) {
          final c = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
          final tabs = c.read(terminalTabsProvider.notifier);
          final tab = await tabs.open(backend.inventory.currentHosts.firstWhere((h) => h.name == 'raspberry-pi'));
          c.read(routerProvider).go(AppRoutes.terminal);
          await settle(tester);
          tab.terminal.write(
            '\x1b[2J\x1b[HFilesystem      Size  Used Avail Use% Mounted on\r\n/dev/sda1        80G   22G   54G  29% /\r\n',
          );
          final b = tab.terminal.buffer;
          tab.controller.setSelection(
            b.createAnchor(0, b.absoluteCursorY - 2),
            b.createAnchor(46, b.absoluteCursorY - 1),
          );
          await settle(tester);
          final point = tester.getTopLeft(find.byType(TerminalView).first) + const Offset(220, 90);
          await tester.tapAt(point, buttons: kSecondaryMouseButton, kind: PointerDeviceKind.mouse);
          await settle(tester);
          if (screen == 'terminal-menu-ai') {
            await tapKey(tester, 'terminal-menu-ask-ai');
            await tester.enterText(find.byKey(const ValueKey('ai-input')), 'Хватает ли свободного места?');
            await settle(tester);
          }
          if (screen == 'terminal-menu-snippet') {
            await tapKey(tester, 'terminal-menu-snippet');
            await tester.enterText(find.byKey(const ValueKey('snippet-name')), 'Проверка места на диске');
            await tester.enterText(find.byKey(const ValueKey('snippet-template')), 'df -h');
            await settle(tester);
          }
        }
        if (screen == 'sftp-settings') {
          await tapKey(tester, 'nav-settings');
          await tester.ensureVisible(find.byKey(const ValueKey('settings-sftp')));
          await settle(tester);
        }
        if (screen == 'sftp-preview') {
          await tapKey(tester, 'nav-sftp');
          await tapKey(tester, 'sftp-connect');
          await pickHost(tester, 'prod-web-1');
          await tapKey(tester, 'sftp-row-/home/deploy/deploy.sh');
          await tapKey(tester, 'sftp-toolbar-quicklook');
        }
        if (screen.startsWith('inventory-')) {
          await tester.tap(find.byKey(const ValueKey('nav-groups')));
          for (var i = 0; i < 12; i++) {
            await tester.pump(const Duration(milliseconds: 100));
          }
          if (screen != 'inventory-groups') {
            await tester.tap(find.byKey(const ValueKey('inventory-group-Production')));
            for (var i = 0; i < 12; i++) {
              await tester.pump(const Duration(milliseconds: 100));
            }
          }
          if (screen == 'inventory-list') {
            await tester.tap(find.byKey(const ValueKey('inventory-view-list')));
            for (var i = 0; i < 12; i++) {
              await tester.pump(const Duration(milliseconds: 100));
            }
          }
        }
        if (screen == 'panel-mode-settings') {
          await tester.tap(find.byKey(const ValueKey('nav-settings')));
          for (var i = 0; i < 12; i++) {
            await tester.pump(const Duration(milliseconds: 100));
          }
          await Scrollable.ensureVisible(
            tester.element(find.byKey(const ValueKey('workspace-panel-style'))),
            alignment: .5,
          );
          for (var i = 0; i < 12; i++) {
            await tester.pump(const Duration(milliseconds: 100));
          }
        }
        if (screen == 'known-hosts') {
          await tester.tap(find.byKey(const ValueKey('nav-knownHosts')));
          for (var i = 0; i < 12; i++) {
            await tester.pump(const Duration(milliseconds: 100));
          }
        }
        if (screen == 'host-menu' || screen == 'host-groups') {
          await tester.enterText(find.byKey(const ValueKey('hosts-search')), 'bastion-a');
          await tester.pump();
          await tester.tap(find.byKey(const ValueKey('host-menu-bastion-a')));
          for (var i = 0; i < 12; i++) {
            await tester.pump(const Duration(milliseconds: 100));
          }
          if (screen == 'host-groups') {
            await tester.tap(find.byKey(const ValueKey('host-group-menu-bastion-a')));
            for (var i = 0; i < 12; i++) {
              await tester.pump(const Duration(milliseconds: 100));
            }
          }
        }
        if (screen.startsWith('workspace-') || screen.startsWith('snippet-')) {
          await tester.enterText(find.byKey(const ValueKey('hosts-search')), 'staging-web');
          await tester.pump();
          await tester.tap(find.byKey(const ValueKey('connect-staging-web')));
          for (var i = 0; i < 12; i++) {
            await tester.pump(const Duration(milliseconds: 100));
          }
          await tester.tap(find.byKey(const ValueKey('accept-host-key')));
          for (var i = 0; i < 12; i++) {
            await tester.pump(const Duration(milliseconds: 100));
          }
          final tool = screen.startsWith('snippet-') || screen.startsWith('workspace-snippets') ? 'snippets' : 'ai';
          await tester.tap(find.byKey(ValueKey('nav-$tool')));
          for (var i = 0; i < 12; i++) {
            await tester.pump(const Duration(milliseconds: 100));
          }
          if (screen == 'workspace-ai') {
            await tester.enterText(
              find.byKey(const ValueKey('ai-input')),
              'Как посмотреть свободное место на сервере?',
            );
          }
          if (screen == 'workspace-ai-selection') {
            final c = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
            final tabs = c.read(terminalTabsProvider.notifier);
            final host = backend.inventory.currentHosts.firstWhere((h) => h.name == 'raspberry-pi');
            for (var i = 0; i < 6; i++) {
              await tabs.open(host);
            }
            for (var i = 0; i < 12; i++) {
              await tester.pump(const Duration(milliseconds: 100));
            }
            tabs.activate(3);
            for (var i = 0; i < 12; i++) {
              await tester.pump(const Duration(milliseconds: 100));
            }
            final tab = c.read(terminalTabsProvider).active!;
            tab.terminal.write(
              '\r\nFilesystem      Size  Used Avail Use% Mounted on\r\n/dev/sda1        80G   22G   54G  29% /\r\n',
            );
            final b = tab.terminal.buffer;
            tab.controller.setSelection(
              b.createAnchor(0, b.absoluteCursorY - 2),
              b.createAnchor(45, b.absoluteCursorY - 1),
            );
            await tester.pump();
            await tester.tap(find.byKey(const ValueKey('ai-attach-selection')));
            await tester.enterText(find.byKey(const ValueKey('ai-input')), 'Хватает ли свободного места?');
            for (var i = 0; i < 6; i++) {
              await tester.pump(const Duration(milliseconds: 100));
            }
            expect(tester.widget<CheckboxListTile>(find.byKey(const ValueKey('ai-attach-selection'))).value, isTrue);
          }
          if (screen == 'snippet-packages') {
            await tester.tap(find.byKey(const ValueKey('snippet-package-filter')));
            for (var i = 0; i < 12; i++) {
              await tester.pump(const Duration(milliseconds: 100));
            }
            await tester.tap(find.text('Диагностика Linux (5)').last);
            for (var i = 0; i < 12; i++) {
              await tester.pump(const Duration(milliseconds: 100));
            }
          }
          if (screen == 'snippet-catalog') {
            await tester.tap(find.byKey(const ValueKey('snippet-starters')));
            for (var i = 0; i < 12; i++) {
              await tester.pump(const Duration(milliseconds: 100));
            }
          }
        }
        if (screen == 'vault-settings') {
          await tester.tap(find.byKey(const ValueKey('nav-settings')));
          for (var i = 0; i < 12; i++) {
            await tester.pump(const Duration(milliseconds: 100));
          }
          await tester.ensureVisible(find.byKey(const ValueKey('device-unlock-switch')));
        }
        if (personalized) {
          await tester.tap(find.byKey(const ValueKey('nav-settings')));
          for (var i = 0; i < 12; i++) {
            await tester.pump(const Duration(milliseconds: 100));
          }
          await Scrollable.ensureVisible(
            tester.element(
              find.byKey(ValueKey('settings-${screen == 'appearance' || glass ? 'appearance' : 'terminal'}-section')),
            ),
            alignment: 0,
          );
          for (var i = 0; i < 12; i++) {
            await tester.pump(const Duration(milliseconds: 100));
          }
        }
        if (glass) {
          final segment = find.byKey(ValueKey('glass-mode-${screen.substring(6)}'));
          await tester.ensureVisible(segment);
          await tester.tap(segment);
          for (var i = 0; i < 12; i++) {
            await tester.pump(const Duration(milliseconds: 100));
          }
        }
        if (screen == 'palette' || screen == 'spectrum') {
          await tester.ensureVisible(find.byKey(const ValueKey('terminal-theme-edit')));
          await tester.tap(find.byKey(const ValueKey('terminal-theme-edit')));
          for (var i = 0; i < 12; i++) {
            await tester.pump(const Duration(milliseconds: 100));
          }
          if (screen == 'spectrum') {
            await tester.tap(find.byKey(const ValueKey('terminal-color-background')));
            for (var i = 0; i < 12; i++) {
              await tester.pump(const Duration(milliseconds: 100));
            }
          }
        }
        expect(tester.takeException(), isNull);
        final boundary = key.currentContext!.findRenderObject()! as RenderRepaintBoundary;
        await tester.runAsync(() async {
          final image = await boundary.toImage();
          final bytes = (await image.toByteData(format: ui.ImageByteFormat.png))!.buffer.asUint8List();
          await File('build/design-preview/$screen-${theme.name}.png').writeAsBytes(bytes);
          image.dispose();
        });
      }, variant: const TargetPlatformVariant({TargetPlatform.macOS}));
    }
  }
}
