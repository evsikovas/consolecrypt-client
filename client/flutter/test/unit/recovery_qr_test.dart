import 'dart:convert';
import 'dart:typed_data';

import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/vault/recovery_qr.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:image/image.dart' as img;
import 'package:zxing2/qrcode.dart';

final _payload =
    '${RecoveryInput.qrPrefix}0b8f3c2e-1d4a-4f6b-9c7e-2a5d8e1f3b6c:${base64Url.encode(List.generate(32, (i) => i * 7 % 256)).replaceAll('=', '')}';

/// [text] as a QR of [scale]-pixel modules with a 4-module quiet zone:
/// luminance rows of [stride] bytes (0 dark, 255 light).
({Uint8List luma, int size, int stride}) _qr(String text, {int scale = 4, int pad = 3, bool invert = false}) {
  final matrix = Encoder.encode(text, ErrorCorrectionLevel.m).matrix!;
  final size = (matrix.width + 8) * scale;
  final stride = size + pad;
  final luma = Uint8List(stride * size)..fillRange(0, stride * size, invert ? 0 : 255);
  for (var y = 0; y < matrix.height; y++) {
    for (var x = 0; x < matrix.width; x++) {
      if (matrix.get(x, y) != 1) continue;
      for (var dy = 0; dy < scale; dy++) {
        final row = ((y + 4) * scale + dy) * stride;
        luma.fillRange(row + (x + 4) * scale, row + (x + 5) * scale, invert ? 255 : 0);
      }
    }
  }
  return (luma: luma, size: size, stride: stride);
}

void main() {
  test('the Recovery Kit QR is read from a camera-like luminance frame', () {
    expect(RecoveryInput.parse(_payload), isA<RecoveryQrInput>());
    final frame = _qr(_payload);
    expect(decodeRecoveryQr(frame.luma, frame.size, frame.size, stride: frame.stride), _payload);
    // Light modules on dark (an inverted screen) read too.
    final inverted = _qr(_payload, invert: true);
    expect(decodeRecoveryQr(inverted.luma, inverted.size, inverted.size, stride: inverted.stride), _payload);
  });

  test('other QR codes and empty frames give nothing', () {
    final other = _qr('https://example.test/not-a-recovery-kit');
    expect(decodeRecoveryQr(other.luma, other.size, other.size, stride: other.stride), isNull);
    final malformed = _qr('${RecoveryInput.qrPrefix}not-a-vault:short');
    expect(decodeRecoveryQr(malformed.luma, malformed.size, malformed.size, stride: malformed.stride), isNull);
    expect(decodeRecoveryQr(Uint8List(100 * 100)..fillRange(0, 10000, 255), 100, 100), isNull);
    expect(decodeRecoveryQr(Uint8List(10), 100, 100), isNull, reason: 'a short buffer is refused');
  });

  test('a picture or screenshot of the QR is read, large ones scaled down', () {
    final frame = _qr(_payload, scale: 12, pad: 0);
    final picture = img.Image(width: frame.size, height: frame.size);
    for (var y = 0; y < frame.size; y++) {
      for (var x = 0; x < frame.size; x++) {
        final v = frame.luma[y * frame.stride + x];
        picture.setPixelRgb(x, y, v, v, v);
      }
    }
    expect(decodeRecoveryQrImage(Uint8List.fromList(img.encodePng(picture))), _payload);
    final large = img.copyResize(picture, width: frame.size * 3, interpolation: img.Interpolation.nearest);
    expect(decodeRecoveryQrImage(Uint8List.fromList(img.encodeJpg(large, quality: 90))), _payload);
    expect(decodeRecoveryQrImage(Uint8List.fromList(utf8.encode('not an image'))), isNull);
  });
}
