// Real app widgets, synthetic sessions only. No native connection is opened.
import 'dart:io';
import 'dart:ui' as ui;

import 'package:consolecrypt/app/app.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/terminal/terminal_minimal_mode.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../test/helpers/test_app.dart';
import '../test/widget/terminal_minimal_mode_test.dart' show minimalKeys;

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  testWidgets('capture minimal terminal and command palette', (tester) async {
    for (final family in ['Segoe UI', 'Segoe UI Variable Text', 'Roboto']) {
      await (FontLoader(family)..addFont(rootBundle.load('assets/fonts/Roboto-Regular.ttf'))).load();
    }
    final mono = ByteData.sublistView(File('C:/Windows/Fonts/consola.ttf').readAsBytesSync());
    await (FontLoader('Consolas')..addFont(Future.value(mono))).load();
    await (FontLoader('monospace')..addFont(Future.value(mono))).load();
    await (FontLoader('MaterialIcons')..addFont(rootBundle.load('fonts/MaterialIcons-Regular.otf'))).load();
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    setTestLocale(backend, AppLocale.ru);
    await backend.services.settings.updateLocal(
      backend.services.settings.currentLocal.copyWith(
        themeMode: AppThemeMode.dark,
        terminalTabPlacement: TerminalTabPlacement.top,
      ),
    );
    final capture = GlobalKey();
    tester.view.physicalSize = const Size(1440, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(
      RepaintBoundary(
        key: capture,
        child: ProviderScope(
          overrides: [appServicesProvider.overrideWithValue(backend.services)],
          child: const ConsoleCryptApp(),
        ),
      ),
    );
    await settle(tester);
    final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    final controller = container.read(terminalTabsProvider.notifier);
    for (final name in ['raspberry-pi', 'prod-web-1', 'build-runner']) {
      final host = backend.inventory.currentHosts.where((h) => h.name == name).firstOrNull;
      if (host != null) await controller.open(host);
    }
    container.read(routerProvider).go(AppRoutes.terminal);
    await settle(tester);
    final tab = container.read(terminalTabsProvider).tabs.first;
    tab.hostKey = null;
    tab.passwordPrompt = null;
    tab.state = SessionConnectionState.connected;
    tab.terminal.write(
      '\x1b[2J\x1b[H'
      '\x1b[38;5;75mConsoleCrypt\x1b[0m  •  SSH / raspberry-pi\r\n\r\n'
      '\x1b[38;5;114mpi@raspberry-pi\x1b[0m:\x1b[38;5;75m~/projects\x1b[0m \$ ls -lah\r\n'
      'total 48K\r\n'
      'drwxr-xr-x  8 pi pi 4.0K Nov  8 17:08 \x1b[38;5;75m.\x1b[0m\r\n'
      'drwxr-xr-x  5 pi pi 4.0K Nov  8 16:40 \x1b[38;5;75m..\x1b[0m\r\n'
      'drwxr-xr-x  4 pi pi 4.0K Nov  8 17:00 \x1b[38;5;75mconsolecrypt\x1b[0m\r\n'
      'drwxr-xr-x  6 pi pi 4.0K Nov  7 12:30 \x1b[38;5;75mhomelab\x1b[0m\r\n'
      '-rw-r--r--  1 pi pi  842 Nov  8 16:52 docker-compose.yml\r\n'
      '-rw-r--r--  1 pi pi 1.2K Nov  8 16:44 README.md\r\n\r\n'
      '\x1b[38;5;114mpi@raspberry-pi\x1b[0m:\x1b[38;5;75m~/projects\x1b[0m \$ docker compose ps\r\n'
      'NAME             IMAGE                  STATUS\r\n'
      'web              nginx:alpine           \x1b[38;5;114mUp 2 hours\x1b[0m\r\n'
      'database         postgres:17            \x1b[38;5;114mUp 2 hours (healthy)\x1b[0m\r\n'
      'redis            redis:7                \x1b[38;5;114mUp 2 hours\x1b[0m\r\n\r\n'
      '\x1b[38;5;114mpi@raspberry-pi\x1b[0m:\x1b[38;5;75m~/projects\x1b[0m \$ ',
    );
    controller.activate(0);
    await settle(tester);
    final output = Directory(
      const String.fromEnvironment('CC_MINIMAL_OUTPUT', defaultValue: '../../../Tasks/terminal-minimal-preview'),
    );
    await tester.runAsync(() => output.create(recursive: true));
    Future<void> shot(String name) async {
      await tester.pump(const Duration(milliseconds: 100));
      final boundary = capture.currentContext!.findRenderObject()! as RenderRepaintBoundary;
      await tester.runAsync(() async {
        final image = await boundary.toImage(pixelRatio: 1);
        final bytes = (await image.toByteData(format: ui.ImageByteFormat.png))!.buffer.asUint8List();
        await File('${output.path}/$name.png').writeAsBytes(bytes);
        image.dispose();
      });
    }

    await shot('normal');
    container.read(terminalMinimalModeProvider.notifier).toggle();
    await settle(tester);
    await shot('minimal-top');
    await minimalKeys(tester, LogicalKeyboardKey.keyK);
    await shot('minimal-palette');
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await settle(tester);
    final settings = backend.services.settings;
    await settings.updateLocal(settings.currentLocal.copyWith(terminalTabPlacement: TerminalTabPlacement.left));
    await settle(tester);
    await shot('minimal-left');
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
}
