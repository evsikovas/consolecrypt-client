// macOS-only visual QA with OS fonts (never copied into the repository).
// flutter test tool/design_preview.dart
// Safe website guide: flutter test --dart-define=CC_GUIDE_CAPTURE=true tool/design_preview.dart
// Add --dart-define=CC_GUIDE_LOCALE=en for English. Guide PNGs default to the
// ignored .local/verification directory; CC_GUIDE_OUTPUT can override it.
import 'dart:io';
import 'dart:ui' as ui;

import 'package:consolecrypt/ai/ai_chat_controller.dart';
import 'package:consolecrypt/ai/command_palette.dart';
import 'package:consolecrypt/app/about.dart';
import 'package:consolecrypt/app/app.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/models/sftp_edit.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/sharing/enrollment_owner.dart';
import 'package:consolecrypt/sharing/sharing_dialogs.dart';
import 'package:consolecrypt/sharing/sharing_models.dart';
import 'package:consolecrypt/sharing/sharing_providers.dart';
import 'package:consolecrypt/sharing/sharing_secrets.dart';
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
import 'guide_capture_support.dart';

const _guide = bool.fromEnvironment('CC_GUIDE_CAPTURE');
const _guideLanguage = String.fromEnvironment('CC_GUIDE_LOCALE', defaultValue: 'ru');
const _guideOutput = String.fromEnvironment('CC_GUIDE_OUTPUT', defaultValue: '../../.local/verification');

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
    for (final family in ['monospace', 'SF Mono']) {
      final loader = FontLoader(family)
        ..addFont(File('/System/Library/Fonts/SFNSMono.ttf').readAsBytes().then(ByteData.sublistView));
      await loader.load();
    }
    final icons = FontLoader('MaterialIcons')..addFont(rootBundle.load('fonts/MaterialIcons-Regular.otf'));
    await icons.load();
    await Directory(_guide ? _guideOutput : 'build/design-preview').create(recursive: true);
  });
  for (final theme in _guide ? [AppThemeMode.dark] : [AppThemeMode.dark, AppThemeMode.light]) {
    for (final screen
        in _guide
            ? guideScreens
            : [
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
      const locale = _guide && _guideLanguage == 'en' ? AppLocale.en : AppLocale.ru;
      final l10n = lookupAppLocalizations(Locale(locale.wireName));
      final glass = screen.startsWith('glass-');
      final personalized = glass || ['appearance', 'terminal', 'palette', 'spectrum'].contains(screen);
      testWidgets('${theme.name} $screen', (tester) async {
        tester.view.physicalSize = screen.endsWith('-compact')
            ? const Size(1024, 720)
            : _guide
            ? const Size(1440, 900)
            : const Size(1440, 960);
        tester.view.devicePixelRatio = 1;
        addTearDown(tester.view.reset);
        final backend = MockBackend(config: MockConfig.test(seedDemoData: !welcome));
        addTearDown(backend.dispose);
        if (!welcome) await backend.debugSignInDemoAndUnlock();
        if (_guide && !welcome) await prepareGuideInventory(backend, screen);
        if (screen.startsWith('snippet-')) {
          for (final package in starterSnippetPackages(l10n)) {
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
            appLocale: locale,
            uiAccentColor: _guide
                ? 0x4B89FF
                : screen.startsWith('terminal-menu')
                ? 0x4B89FF
                : personalized
                ? 0x7C5CFC
                : null,
            uiBackgroundColor: !_guide && personalized ? 0x16324F : null,
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
        final sharing = GuideSharingService();
        await tester.pumpWidget(
          RepaintBoundary(
            key: key,
            child: ProviderScope(
              overrides: [
                appServicesProvider.overrideWithValue(backend.services),
                if (_guide) developerControlsProvider.overrideWithValue(null),
                if (_guide) sharingServiceProvider.overrideWithValue(sharing),
                if (_guide) enrollmentServiceProvider.overrideWithValue(GuideEnrollmentService()),
              ],
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
          if (_guide) expect(find.byKey(const ValueKey('terminal-menu-copy')), findsOneWidget);
          if (screen == 'terminal-menu-ai') {
            await tapKey(tester, 'terminal-menu-ask-ai');
            await tester.enterText(
              find.byKey(const ValueKey('ai-input')),
              locale == AppLocale.en ? 'Is there enough free space?' : 'Хватает ли свободного места?',
            );
            await settle(tester);
          }
          if (screen == 'terminal-menu-snippet') {
            await tapKey(tester, 'terminal-menu-snippet');
            await tester.enterText(find.byKey(const ValueKey('snippet-name')), 'Проверка места на диске');
            await tester.enterText(find.byKey(const ValueKey('snippet-template')), 'df -h');
            await settle(tester);
          }
        }
        if (screen == 'terminal-selection-scroll' || screen == 'terminal-tabs-overflow') {
          final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
          final tabs = container.read(terminalTabsProvider.notifier);
          final host = backend.inventory.currentHosts.firstWhere((h) => h.name == 'raspberry-pi');
          final tab = await tabs.open(host);
          if (screen == 'terminal-tabs-overflow') {
            for (var i = 0; i < 7; i++) {
              await tabs.open(host);
            }
            tabs.activate(3);
          }
          container.read(routerProvider).go(AppRoutes.terminal);
          await settle(tester);
          if (screen == 'terminal-selection-scroll') {
            tab.terminal.write('\x1b[2J\x1b[H');
            tab.terminal.write(
              [
                for (var i = 0; i < 100; i++)
                  '[${i.toString().padLeft(3, '0')}] web-01.example.test  service=nginx  status=ready  request=/healthz\r\n',
              ].join(),
            );
            await settle(tester);
            final view = tester.widget<TerminalView>(find.byType(TerminalView).first);
            final position = view.scrollController!.position;
            view.scrollController!.jumpTo(position.maxScrollExtent / 2);
            await settle(tester);
            tab.controller.setSelection(
              tab.terminal.buffer.createAnchor(0, 32),
              tab.terminal.buffer.createAnchor(84, 67),
            );
            await settle(tester);
            expect(tab.controller.selection, isNotNull);
            expect(position.extentBefore, greaterThan(0));
            expect(position.extentAfter, greaterThan(0));
          } else {
            expect(find.text('(8)'), findsOneWidget);
            expect(find.byKey(const ValueKey('glass-tabs-count')), findsOneWidget);
          }
        }
        if (screen == 'host-palette') {
          showCommandPalette(tester.element(find.byType(Navigator).first)).ignore();
          await settle(tester);
          await tester.enterText(find.byKey(const ValueKey('palette-input')), 'prod');
          await settle(tester);
          expect(find.text('prod-web-1'), findsWidgets);
          expect(find.text('prod-db-1'), findsWidgets);
        }
        if (screen == 'about-license') {
          showAboutConsoleCrypt(tester.element(find.byType(Navigator).first)).ignore();
          await settle(tester);
          expect(find.text('AGPL-3.0'), findsNWidgets(2));
          expect(find.text('AGPL-3.0-only'), findsNothing);
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
          await tester.tap(find.byKey(const ValueKey('inventory-nav-groups')));
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
          if (_guide && screen.contains('ai')) {
            final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
            final provider = backend.cloud.activeData!.providers.firstWhere((p) => p.isRemote);
            container.read(aiChatControllerProvider.notifier).selectProvider(provider.id);
          }
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
            await tester.enterText(
              find.byKey(const ValueKey('ai-input')),
              locale == AppLocale.en ? 'Is there enough free space?' : 'Хватает ли свободного места?',
            );
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
            await tester.tap(find.text('${starterSnippetPackages(l10n).first.name} (5)').last);
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
        if (_guide && ['credentials', 'devices', 'tunnels', 'backups', 'sync', 'sharing-main'].contains(screen)) {
          await tapKey(tester, 'nav-${screen == 'sharing-main' ? 'sharing' : screen}');
          if (screen == 'credentials') {
            await tapKey(tester, 'credential-home-lab password');
          }
        }
        if (_guide && screen == 'updates') {
          await tapKey(tester, 'nav-settings');
          await Scrollable.ensureVisible(tester.element(find.byKey(const ValueKey('settings-updates'))), alignment: .3);
          await settle(tester);
        }
        if (_guide && screen == 'security-settings') {
          await tapKey(tester, 'nav-settings');
          await Scrollable.ensureVisible(
            tester.element(find.byKey(const ValueKey('screen-capture-macos-info'))),
            alignment: .4,
          );
          await settle(tester);
        }
        if (_guide && screen.startsWith('sharing-') && screen != 'sharing-main') {
          final context = tester.element(find.byType(Navigator).first);
          if (screen == 'sharing-secret') {
            showSharingSecretPublish(context, credential: backend.cloud.activeData!.credentials.first).ignore();
          } else {
            showSharingPublish(
              context,
              kind: SharingKind.host,
              objectId: backend.inventory.currentHosts.first.id.value,
            ).ignore();
          }
          await settle(tester);
          if (screen == 'sharing-recipient' || screen == 'sharing-roles') {
            await tester.enterText(find.byKey(const ValueKey('sharing-email')), 'colleague@example.test');
            await tapKey(tester, 'sharing-find');
            await tapKey(tester, 'sharing-verify-${guideRecipient.deviceId}');
            await Scrollable.ensureVisible(tester.element(find.text(guideRecipient.code)), alignment: .15);
            await settle(tester);
            if (screen == 'sharing-roles') {
              final role = find.byType(GlassSelect<SharingRole>);
              await tester.tap(role.last);
              await settle(tester);
              expect(find.text(l10n.sharingEditor), findsWidgets);
            }
          }
        }
        if (_guide && screen == 'enrollment-owner') {
          final context = tester.element(find.byType(Navigator).first);
          showEnrollmentOwner(context, sharing.ownedHost).ignore();
          await settle(tester);
          await tapKey(tester, 'enrollment-new-grant');
          final anchor = tester.widget<GlassSelect<String?>>(find.byType(GlassSelect<String?>).last);
          anchor.onChanged!(guideRecipient.deviceId);
          await settle(tester);
          expect(
            tester.widget<CheckboxListTile>(find.byKey(const ValueKey('enrollment-grant-confirmed'))).value,
            isFalse,
          );
          expect(tester.widget<GlassButton>(find.byKey(const ValueKey('enrollment-grant-create'))).onPressed, isNull);
        }
        expect(tester.takeException(), isNull);
        if (_guide) _assertGuideScreen(tester, screen, l10n);
        final boundary = key.currentContext!.findRenderObject()! as RenderRepaintBoundary;
        await tester.runAsync(() async {
          final image = await boundary.toImage();
          final bytes = (await image.toByteData(format: ui.ImageByteFormat.png))!.buffer.asUint8List();
          final path = _guide
              ? '$_guideOutput/guide-$screen-${locale.wireName}.png'
              : 'build/design-preview/$screen-${theme.name}.png';
          await File(path).writeAsBytes(bytes);
          image.dispose();
        });
      }, variant: const TargetPlatformVariant({TargetPlatform.macOS}));
    }
  }
}

void _assertGuideScreen(WidgetTester tester, String screen, AppLocalizations l10n) {
  final texts = [
    ...tester.widgetList<Text>(find.byType(Text)).map((w) => w.data ?? w.textSpan?.toPlainText() ?? ''),
    ...tester
        .widgetList<SelectableText>(find.byType(SelectableText))
        .map((w) => w.data ?? w.textSpan?.toPlainText() ?? ''),
  ].join('\n');
  expect(texts, isNot(contains('PRIVATE KEY')));
  expect(texts, isNot(contains(guideUnexpectedMetadata)));
  expect(texts, isNot(contains('correct horse battery staple')));
  expect(texts, isNot(contains('demo-password-1')));
  expect(find.byType(ConsoleCryptApp), findsOneWidget);
  switch (screen) {
    case 'hosts':
      expect(find.byKey(const ValueKey('add-host')), findsOneWidget);
      expect(texts, contains('bastion-a.example.test'));
    case 'inventory-groups':
      expect(find.byKey(const ValueKey('inventory-group-Production')), findsWidgets);
    case 'inventory-production':
      expect(texts, contains('Production'));
    case 'terminal-menu':
      expect(find.byKey(const ValueKey('terminal-menu-ask-ai')), findsOneWidget);
      expect(find.byKey(const ValueKey('terminal-menu-snippet')), findsOneWidget);
    case 'terminal-selection-scroll':
      expect(find.byType(TerminalView), findsWidgets);
    case 'terminal-tabs-overflow':
      expect(find.text('(8)'), findsOneWidget);
    case 'host-palette':
      expect(find.byKey(const ValueKey('command-palette')), findsOneWidget);
    case 'about-license':
      expect(find.text('AGPL-3.0'), findsNWidgets(2));
    case 'sftp-preview':
      expect(find.byKey(const ValueKey('sftp-preview-line-numbers')), findsOneWidget);
      expect(find.byKey(const ValueKey('sftp-quicklook-text')), findsOneWidget);
    case 'sftp-settings':
      expect(find.text('Visual Studio Code'), findsWidgets);
    case 'workspace-ai-selection':
      expect(tester.widget<CheckboxListTile>(find.byKey(const ValueKey('ai-attach-selection'))).value, isTrue);
      expect(texts, contains(l10n.aiChatRemoteProviderNotice('x', 'x').split('x').first));
    case 'appearance':
      expect(find.byKey(const ValueKey('ui-font-scale')), findsOneWidget);
      expect(find.byKey(const ValueKey('glass-mode')), findsOneWidget);
    case 'panel-mode-settings':
      expect(find.byKey(const ValueKey('workspace-panel-expanded')), findsOneWidget);
    case 'vault-settings':
      expect(find.byKey(const ValueKey('device-unlock-switch')), findsOneWidget);
      expect(find.byKey(const ValueKey('auto-lock')), findsOneWidget);
    case 'security-settings':
      expect(find.byKey(const ValueKey('screen-capture-macos-info')), findsOneWidget);
    case 'backups':
      expect(find.byKey(const ValueKey('export-backup')), findsOneWidget);
      expect(texts, contains('Workspace-2026-10-01.ccbackup'));
    case 'updates':
      expect(find.byKey(const ValueKey('updates-check')), findsOneWidget);
      expect(find.byKey(const ValueKey('updates-automatic')), findsOneWidget);
    case 'sync':
      expect(find.byKey(const ValueKey('sync-now')), findsOneWidget);
    case 'credentials':
      expect(texts, contains('home-lab password'));
      expect(texts, contains('•'));
    case 'devices':
      expect(find.byKey(const ValueKey('review-request')), findsOneWidget);
    case 'tunnels':
      expect(find.byKey(const ValueKey('add-tunnel')), findsOneWidget);
      expect(texts, contains('service.example.test'));
    case 'sharing-main':
      expect(texts, contains('Disk health'));
      expect(texts, contains('Deployment credential'));
    case 'sharing-recipient' || 'sharing-roles':
      expect(find.text(guideRecipient.code), findsOneWidget);
      expect(
        tester.widget<CheckboxListTile>(find.byKey(ValueKey('sharing-verify-${guideRecipient.deviceId}'))).value,
        isTrue,
      );
    case 'sharing-secret':
      expect(find.text('••••••••'), findsOneWidget);
      expect(tester.widget<GlassButton>(find.byKey(const ValueKey('sharing-secret-publish'))).onPressed, isNull);
    case 'enrollment-owner':
      expect(find.text(guideRecipient.code), findsOneWidget);
      expect(find.text(l10n.sharingReader), findsWidgets);
    default:
      expect(texts.trim(), isNotEmpty);
  }
}
