import 'dart:async';
import 'dart:typed_data';

import 'package:camera/camera.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:flutter/foundation.dart';
import 'package:image/image.dart' as img;
import 'package:material_ui/material_ui.dart';
import 'package:zxing2/qrcode.dart';

/// The Recovery Kit QR text in 8-bit luminance [luma] of [width]×[height]
/// (rows [stride] bytes apart), or null when there is no recovery QR.
/// Pure Dart (zxing2): nothing leaves the process.
String? decodeRecoveryQr(Uint8List luma, int width, int height, {int? stride}) {
  final rowBytes = stride ?? width;
  if (width <= 0 || height <= 0 || rowBytes < width || luma.length < rowBytes * (height - 1) + width) return null;
  final compact = Int8List(width * height);
  for (var y = 0; y < height; y++) {
    compact.setRange(y * width, (y + 1) * width, Int8List.sublistView(luma, y * rowBytes, y * rowBytes + width));
  }
  final source = RGBLuminanceSource.crop(compact, width, height, 0, 0, width, height);
  for (final candidate in [source, InvertedLuminanceSource(source)]) {
    try {
      final text = QRCodeReader().decode(BinaryBitmap(HybridBinarizer(candidate))).text;
      return _recovery(text);
    } on ReaderException {
      continue;
    }
  }
  return null;
}

String? _recovery(String text) {
  final trimmed = text.trim();
  return trimmed.startsWith(RecoveryInput.qrPrefix) && RecoveryInput.parse(trimmed) != null ? trimmed : null;
}

/// The Recovery Kit QR in a picture or screenshot (PNG, JPEG, …), or null.
/// Large pictures are scaled down first.
String? decodeRecoveryQrImage(Uint8List bytes) {
  var picture = img.decodeImage(bytes);
  if (picture == null) return null;
  const limit = 1600;
  if (picture.width > limit || picture.height > limit) {
    picture = picture.width >= picture.height
        ? img.copyResize(picture, width: limit)
        : img.copyResize(picture, height: limit);
  }
  final luma = Uint8List(picture.width * picture.height);
  var i = 0;
  for (final pixel in picture) {
    luma[i++] = img.getLuminanceRgb(pixel.r, pixel.g, pixel.b).toInt().clamp(0, 255);
  }
  return decodeRecoveryQr(luma, picture.width, picture.height);
}

/// One camera frame as luminance: the Y plane (YUV 4:2:0) or BGRA
/// converted at every second pixel (iOS).
({Uint8List luma, int width, int height, int stride})? _frameLuma(CameraImage frame) {
  if (frame.planes.isEmpty) return null;
  final plane = frame.planes.first;
  if (frame.format.group == ImageFormatGroup.bgra8888) {
    final width = frame.width ~/ 2, height = frame.height ~/ 2;
    final luma = Uint8List(width * height);
    final bytes = plane.bytes, row = plane.bytesPerRow;
    for (var y = 0; y < height; y++) {
      for (var x = 0; x < width; x++) {
        final o = (y * 2) * row + (x * 2) * 4;
        luma[y * width + x] = ((bytes[o] + 2 * bytes[o + 1] + bytes[o + 2]) >> 2);
      }
    }
    return (luma: luma, width: width, height: height, stride: width);
  }
  return (luma: plane.bytes, width: frame.width, height: frame.height, stride: plane.bytesPerRow);
}

String? _decodeFrame(({Uint8List luma, int width, int height, int stride}) frame) =>
    decodeRecoveryQr(frame.luma, frame.width, frame.height, stride: frame.stride);

/// Scans the Recovery Kit QR with the back camera (phones). Frames are
/// decoded in memory, a few per second, and never stored; the dialog
/// closes with the recovery text.
Future<String?> scanRecoveryQr(BuildContext context) =>
    showDialog<String>(context: context, builder: (_) => const RecoveryQrScanner());

class RecoveryQrScanner extends StatefulWidget {
  const RecoveryQrScanner({super.key});
  @override
  State<RecoveryQrScanner> createState() => _RecoveryQrScannerState();
}

class _RecoveryQrScannerState extends State<RecoveryQrScanner> {
  CameraController? _camera;
  bool _failed = false, _busy = false, _done = false;
  DateTime _last = DateTime.fromMillisecondsSinceEpoch(0);

  @override
  void initState() {
    super.initState();
    unawaited(_start());
  }

  Future<void> _start() async {
    try {
      final cameras = await availableCameras();
      final back = cameras.where((c) => c.lensDirection == CameraLensDirection.back).firstOrNull ?? cameras.first;
      final camera = CameraController(
        back,
        ResolutionPreset.high,
        enableAudio: false,
        imageFormatGroup: defaultTargetPlatform == TargetPlatform.iOS
            ? ImageFormatGroup.bgra8888
            : ImageFormatGroup.yuv420,
      );
      await camera.initialize();
      if (!mounted) {
        await camera.dispose();
        return;
      }
      setState(() => _camera = camera);
      await camera.startImageStream(_frame);
    } catch (_) {
      if (mounted) setState(() => _failed = true);
    }
  }

  void _frame(CameraImage frame) {
    final now = DateTime.now();
    if (_busy || _done || now.difference(_last) < const Duration(milliseconds: 300)) return;
    _last = now;
    final luma = _frameLuma(frame);
    if (luma == null) return;
    _busy = true;
    unawaited(
      compute(_decodeFrame, luma)
          .then((text) {
            if (text == null || _done || !mounted) return;
            _done = true;
            Navigator.of(context).pop(text);
          })
          .whenComplete(() => _busy = false),
    );
  }

  @override
  void dispose() {
    final camera = _camera;
    if (camera != null) {
      unawaited(
        camera.value.isStreamingImages ? camera.stopImageStream().whenComplete(camera.dispose) : camera.dispose(),
      );
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final camera = _camera;
    return Dialog(
      key: const ValueKey('recovery-qr-scanner'),
      child: Padding(
        padding: const EdgeInsets.all(GlassSpacing.s12),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(l.recoveryScanQrHelp, textAlign: TextAlign.center),
            const SizedBox(height: GlassSpacing.s12),
            AspectRatio(
              aspectRatio: 1,
              child: _failed
                  ? Center(child: Text(l.recoveryScanQrUnavailable, textAlign: TextAlign.center))
                  : camera == null
                  ? const Center(child: CircularProgressIndicator())
                  : ClipRRect(borderRadius: BorderRadius.circular(12), child: CameraPreview(camera)),
            ),
            const SizedBox(height: GlassSpacing.s12),
            GlassButton(label: l.commonCancel, onPressed: () => Navigator.of(context).pop()),
          ],
        ),
      ),
    );
  }
}
