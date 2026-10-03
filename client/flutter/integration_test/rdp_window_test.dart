// Run on a desktop with a window manager (macOS, Windows or a Linux desktop).
// Uses the real runner/window channel; no network, vault or RDP credentials.
import 'dart:io';

import 'package:consolecrypt/rdp/rdp_window.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:material_ui/material_ui.dart';

Future<void> _until(WidgetTester tester, Future<bool> Function() condition) async {
  for (var attempt = 0; attempt < 100; attempt++) {
    if (await condition()) return;
    await tester.pump(const Duration(milliseconds: 100));
  }
  fail('Native window did not settle in the expected mode.');
}

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();
  testWidgets('native fullscreen fills the monitor and restores window placement', (tester) async {
    await tester.pumpWidget(
      const MaterialApp(
        home: Scaffold(body: Center(child: Text('ConsoleCrypt window test'))),
      ),
    );
    await tester.pumpAndSettle();
    final window = RdpWindow();
    final originalFullscreen = await window.isFullscreen();
    final originalSize = tester.view.physicalSize;
    final lease = window.acquire()!;
    try {
      await lease.enter();
      await tester.pumpAndSettle();
      // Windows checks actual borderless monitor bounds; AppKit/GTK report
      // their acknowledged native fullscreen state, not a Dart layout flag.
      expect(await window.isFullscreen(), isTrue);
      expect(window.acquire(), isNull);
      expect(tester.view.physicalSize.width, greaterThanOrEqualTo(originalSize.width - 2));
      expect(tester.view.physicalSize.height, greaterThanOrEqualTo(originalSize.height - 2));
    } finally {
      await lease.restore();
    }
    await _until(tester, () async => await window.isFullscreen() == originalFullscreen);
    await _until(
      tester,
      () async =>
          (tester.view.physicalSize.width - originalSize.width).abs() < 3 &&
          (tester.view.physicalSize.height - originalSize.height).abs() < 3,
    );
    final next = window.acquire();
    expect(next, isNotNull);
    await next!.restore();
  }, skip: !(Platform.isMacOS || Platform.isWindows || Platform.isLinux));
}
