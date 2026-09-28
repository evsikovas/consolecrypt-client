// Run from client/flutter: flutter test tool/render_brand.dart
// Generates app icons directly from the original vector painter, never from
// resized raster artwork. No fonts or external design dependencies are needed.
import 'dart:io';
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:consolecrypt/app/brand.dart';
import 'package:consolecrypt/app/theme/glass_palette.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

Future<Uint8List> iconPng(int size, {required bool mac}) async {
  final recorder = ui.PictureRecorder();
  final canvas = Canvas(recorder);
  canvas.scale(size / 1024);
  final bounds = mac ? const Rect.fromLTWH(100, 100, 824, 824) : const Rect.fromLTWH(24, 24, 976, 976);
  final shape = RRect.fromRectAndRadius(bounds, Radius.circular(bounds.width * .224));
  canvas.drawRRect(
    shape.shift(const Offset(0, 14)),
    Paint()
      ..color = const Color(0x33000000)
      ..maskFilter = const MaskFilter.blur(BlurStyle.normal, 18),
  );
  canvas.drawRRect(
    shape,
    Paint()
      ..shader = const LinearGradient(
        begin: Alignment.topLeft,
        end: Alignment.bottomRight,
        colors: [Color(0xff30343b), Color(0xff17191e), Color(0xff0b0c0f)],
      ).createShader(bounds),
  );
  canvas.drawRRect(
    shape.deflate(1.5),
    Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = 3
      ..shader = const LinearGradient(
        begin: Alignment.topLeft,
        end: Alignment.bottomRight,
        colors: [Color(0x667b808b), Color(0x00262a32), Color(0x444b5059)],
      ).createShader(bounds),
  );
  final glyphSize = bounds.width * .68;
  canvas.translate((1024 - glyphSize) / 2, (1024 - glyphSize) / 2);
  const BrandSymbolPainter(color: GlassPalette.brandBlue).paint(canvas, Size.square(glyphSize));
  final picture = recorder.endRecording();
  final image = await picture.toImage(size, size);
  final data = (await image.toByteData(format: ui.ImageByteFormat.png))!.buffer.asUint8List();
  image.dispose();
  picture.dispose();
  return data;
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  test('render app-icon assets from the shared vector', () async {
    for (final size in [16, 32, 64, 128, 256, 512, 1024]) {
      await File('macos/Runner/Assets.xcassets/AppIcon.appiconset/app_icon_$size.png')
          .writeAsBytes(await iconPng(size, mac: true));
    }
    for (final entry in {'mdpi': 48, 'hdpi': 72, 'xhdpi': 96, 'xxhdpi': 144, 'xxxhdpi': 192}.entries) {
      await File('android/app/src/main/res/mipmap-${entry.key}/ic_launcher.png')
          .writeAsBytes(await iconPng(entry.value, mac: false));
    }
    const sizes = [16, 24, 32, 48, 64, 128, 256];
    final images = <Uint8List>[];
    for (final size in sizes) {
      images.add(await iconPng(size, mac: false));
    }
    final header = ByteData(6 + 16 * sizes.length)
      ..setUint16(2, 1, Endian.little)
      ..setUint16(4, sizes.length, Endian.little);
    var offset = header.lengthInBytes;
    for (var i = 0; i < sizes.length; i++) {
      final at = 6 + 16 * i;
      header
        ..setUint8(at, sizes[i] % 256)
        ..setUint8(at + 1, sizes[i] % 256)
        ..setUint16(at + 4, 1, Endian.little)
        ..setUint16(at + 6, 32, Endian.little)
        ..setUint32(at + 8, images[i].length, Endian.little)
        ..setUint32(at + 12, offset, Endian.little);
      offset += images[i].length;
    }
    final ico = BytesBuilder()..add(header.buffer.asUint8List());
    for (final image in images) {
      ico.add(image);
    }
    await File('windows/runner/resources/app_icon.ico').writeAsBytes(ico.takeBytes());
  });
}
