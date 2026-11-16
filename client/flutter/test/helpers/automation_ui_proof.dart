import 'dart:io' as io;
import 'dart:ui' as ui;

import 'package:flutter/rendering.dart' show RenderRepaintBoundary;
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

bool _proofFontsLoaded = false;
Future<void> loadAutomationProofFonts(WidgetTester tester) async {
  if (_proofFontsLoaded || io.Platform.environment['CONSOLECRYPT_UI_PROOF_DIR'] == null) return;
  await tester.runAsync(() async {
    const text = 'C:/Windows/Fonts/segoeui.ttf';
    const variable = 'C:/Windows/Fonts/SegUIVar.ttf';
    final uiFont = io.File(variable).existsSync() ? variable : text;
    for (final font in {
      'Segoe UI Variable Text': uiFont,
      'Segoe UI Variable Display': uiFont,
      'Segoe UI': text,
      'Roboto': text,
      'Ahem': text,
      'Consolas': 'C:/Windows/Fonts/consola.ttf',
      'monospace': 'C:/Windows/Fonts/consola.ttf',
      'MaterialIcons': 'D:/dev/flutter/bin/cache/artifacts/material_fonts/MaterialIcons-Regular.otf',
    }.entries) {
      final loader = FontLoader(font.key)..addFont(io.File(font.value).readAsBytes().then(ByteData.sublistView));
      await loader.load();
    }
    _proofFontsLoaded = true;
  });
}

// Optional visual proof of this synthetic UI; ordinary tests write nothing.
Future<void> captureAutomationDialog(WidgetTester tester, String name) async {
  final directory = io.Platform.environment['CONSOLECRYPT_UI_PROOF_DIR'];
  if (directory == null || directory.isEmpty) return;
  await tester.pump();
  final boundary = tester.renderObject<RenderRepaintBoundary>(
    find.ancestor(of: find.byType(AlertDialog).last, matching: find.byType(RepaintBoundary)).first,
  );
  await tester.runAsync(() async {
    final image = await boundary.toImage(pixelRatio: 1);
    try {
      final bytes = await image.toByteData(format: ui.ImageByteFormat.png);
      await io.Directory(directory).create(recursive: true);
      await io.File('$directory/$name.png').writeAsBytes(bytes!.buffer.asUint8List(), flush: true);
    } finally {
      image.dispose();
    }
  });
}

Future<void> captureAutomationWorkspace(WidgetTester tester, String name) async {
  final directory = io.Platform.environment['CONSOLECRYPT_UI_PROOF_DIR'];
  if (directory == null || directory.isEmpty) return;
  await tester.pump();
  final boundary = tester.renderObject<RenderRepaintBoundary>(find.byKey(const ValueKey('automation-proof-root')));
  await tester.runAsync(() async {
    final image = await boundary.toImage(pixelRatio: 1);
    try {
      final bytes = await image.toByteData(format: ui.ImageByteFormat.png);
      await io.Directory(directory).create(recursive: true);
      await io.File('$directory/$name.png').writeAsBytes(bytes!.buffer.asUint8List(), flush: true);
    } finally {
      image.dispose();
    }
  });
}
