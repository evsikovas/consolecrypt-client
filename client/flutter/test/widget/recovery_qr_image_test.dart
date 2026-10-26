import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/file_dialog_service.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:image/image.dart' as img;
import 'package:material_ui/material_ui.dart';
import 'package:zxing2/qrcode.dart';

import '../helpers/test_app.dart';

class _Files implements FileDialogService {
  _Files(this.path);
  final String path;
  @override
  Future<String?> chooseOpenFile({List<String> extensions = const []}) async => path;
  @override
  dynamic noSuchMethod(Invocation invocation) => throw UnimplementedError();
}

/// A PNG of [text] as a QR code.
Uint8List _png(String text) {
  final matrix = Encoder.encode(text, ErrorCorrectionLevel.m).matrix!;
  const scale = 8;
  final size = (matrix.width + 8) * scale;
  final picture = img.Image(width: size, height: size)..clear(img.ColorRgb8(255, 255, 255));
  for (var y = 0; y < matrix.height; y++) {
    for (var x = 0; x < matrix.width; x++) {
      if (matrix.get(x, y) != 1) continue;
      img.fillRect(
        picture,
        x1: (x + 4) * scale,
        y1: (y + 4) * scale,
        x2: (x + 5) * scale - 1,
        y2: (y + 5) * scale - 1,
        color: img.ColorRgb8(0, 0, 0),
      );
    }
  }
  return Uint8List.fromList(img.encodePng(picture));
}

void main() {
  testWidgets('the Recovery Kit QR is read from a picture into the recovery field', (tester) async {
    final payload =
        '${RecoveryInput.qrPrefix}0b8f3c2e-1d4a-4f6b-9c7e-2a5d8e1f3b6c:${base64Url.encode(List.filled(32, 7)).replaceAll('=', '')}';
    final dir = Directory.systemTemp.createTempSync('cc-recovery-qr');
    addTearDown(() => dir.deleteSync(recursive: true));
    final file = File('${dir.path}/kit.png')..writeAsBytesSync(_png(payload));
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await backend.vault.lock();
    await pumpApp(tester, backend, overrides: [fileDialogServiceProvider.overrideWithValue(_Files(file.path))]);
    await tapKey(tester, 'forgot-passphrase');
    await tester.tap(find.text('Recovery Key'));
    await settle(tester);
    expect(find.byKey(const ValueKey('recovery-scan-qr')), findsNothing, reason: 'the camera is for phones');
    await tester.runAsync(() async {
      await tester.tap(find.byKey(const ValueKey('recovery-qr-image')));
      for (var i = 0; i < 50 && !find.text('QR text recognised').evaluate().isNotEmpty; i++) {
        await Future<void>.delayed(const Duration(milliseconds: 50));
        await tester.pump();
      }
    });
    await settle(tester);
    final field = tester.widget<TextField>(find.byKey(const ValueKey('recovery-input')));
    expect(field.controller!.text, payload);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
}
