import 'dart:math' as math;
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:consolecrypt/app/theme/glass_materials.dart';
import 'package:consolecrypt/app/theme/glass_tokens.dart';
import 'package:flutter/widgets.dart';

/// Layer 1 (LIQUID_GLASS_SPEC §2.4, §3): the static soft gradient the glass
/// "refracts". Painted once into its own [RepaintBoundary]
/// (`isComplex: true, willChange: false`); it only repaints on resize,
/// theme change or profile cue change, so it never forces live blurs to
/// re-rasterize. There is no ambient motion (optional, off by default in
/// the spec; not implemented).
class AmbientBackdrop extends StatelessWidget {
  const AmbientBackdrop({super.key, this.child, this.expressive = false, this.localProfileCue = false});

  final Widget? child;

  /// Onboarding / welcome: blobs 1.4× stronger (§4.15).
  final bool expressive;

  /// Local-only profile (ADR-0106): blob A becomes warm graphite.
  final bool localProfileCue;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final dpr = MediaQuery.maybeDevicePixelRatioOf(context) ?? 1.0;
    Widget paint(ui.Image? grainTile) => CustomPaint(
      key: const ValueKey('ambient-backdrop'),
      isComplex: true,
      painter: AmbientPainter(
        spec: tokens.ambient,
        expressive: expressive,
        localProfileCue: localProfileCue,
        grainTile: grainTile,
        devicePixelRatio: dpr,
      ),
      child: const SizedBox.expand(),
    );
    final backdrop = RepaintBoundary(
      child: tokens.ambient.grain <= 0
          ? paint(null)
          : FutureBuilder<ui.Image>(
              future: _AmbientGrain.load(),
              initialData: _AmbientGrain.cached,
              builder: (context, snapshot) => paint(snapshot.data),
            ),
    );
    final c = child;
    if (c == null) return backdrop;
    return Stack(
      children: [
        Positioned.fill(child: backdrop),
        c,
      ],
    );
  }
}

/// Painter of [AmbientBackdrop] (public for the gallery and tests).
class AmbientPainter extends CustomPainter {
  AmbientPainter({
    required this.spec,
    this.expressive = false,
    this.localProfileCue = false,
    this.grainTile,
    this.devicePixelRatio = 1,
  });

  final AmbientSpec spec;
  final bool expressive;
  final bool localProfileCue;
  final ui.Image? grainTile;
  final double devicePixelRatio;

  @override
  void paint(Canvas canvas, Size size) {
    final rect = Offset.zero & size;
    canvas.drawRect(rect, Paint()..color = spec.base);
    final extent = math.max(size.width, size.height);
    for (final (i, blob) in spec.blobs.indexed) {
      final b = (i == 0 && localProfileCue) ? spec.localCueBlob : blob;
      final alpha = (b.color.a * (expressive ? 1.4 : 1.0)).clamp(0.0, 1.0);
      final center = Offset(b.center.dx * size.width, b.center.dy * size.height);
      final radius = math.max(b.radius * extent, 1.0);
      final color = b.color.withValues(alpha: alpha);
      canvas.drawRect(rect, Paint()..shader = ui.Gradient.radial(center, radius, [color, color.withValues(alpha: 0)]));
    }
    if (spec.grain > 0 && grainTile != null) _paintGrain(canvas, rect);
  }

  /// One cached tile shades every physical pixel. Sparse points left almost
  /// all of the dark 8-bit gradient unchanged, so its contours stayed visible.
  /// Overlay noise is centred on neutral grey: it does not whiten the blue
  /// background, and needs neither a live shader filter nor a saveLayer.
  void _paintGrain(Canvas canvas, Rect rect) {
    final shader = ui.ImageShader(
      grainTile!,
      ui.TileMode.repeated,
      ui.TileMode.repeated,
      Matrix4.diagonal3Values(1 / devicePixelRatio, 1 / devicePixelRatio, 1).storage,
    );
    canvas.drawRect(
      rect,
      Paint()
        ..shader = shader
        ..blendMode = BlendMode.overlay
        ..color = Color.fromRGBO(255, 255, 255, (spec.grain * 4).clamp(0.0, 1.0))
        ..filterQuality = FilterQuality.none
        ..isAntiAlias = false,
    );
    shader.dispose();
  }

  @override
  bool shouldRepaint(AmbientPainter oldDelegate) =>
      oldDelegate.spec != spec ||
      oldDelegate.expressive != expressive ||
      oldDelegate.localProfileCue != localProfileCue ||
      oldDelegate.grainTile != grainTile ||
      oldDelegate.devicePixelRatio != devicePixelRatio;
}

/// Process-wide, deterministic 64 KiB texture. It is generated and decoded
/// once, never regenerated on resize, theme changes or streaming frames.
abstract final class _AmbientGrain {
  static const side = 128;
  static ui.Image? cached;
  static Future<ui.Image>? _pending;

  static Future<ui.Image> load() => _pending ??= _create();

  static Future<ui.Image> _create() async {
    final pixels = Uint8List(side * side * 4);
    var seed = 0x2545F491;
    for (var i = 0; i < pixels.length; i += 4) {
      seed ^= (seed << 13) & 0xffffffff;
      seed ^= seed >>> 17;
      seed ^= (seed << 5) & 0xffffffff;
      seed &= 0xffffffff;
      final value = seed & 0xff;
      pixels[i] = value;
      pixels[i + 1] = value;
      pixels[i + 2] = value;
      pixels[i + 3] = 255;
    }
    final buffer = await ui.ImmutableBuffer.fromUint8List(pixels);
    final descriptor = ui.ImageDescriptor.raw(buffer, width: side, height: side, pixelFormat: ui.PixelFormat.rgba8888);
    ui.Codec? codec;
    try {
      codec = await descriptor.instantiateCodec();
      final frame = await codec.getNextFrame();
      return cached = frame.image;
    } finally {
      codec?.dispose();
      descriptor.dispose();
      buffer.dispose();
    }
  }
}
