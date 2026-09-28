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
    final backdrop = RepaintBoundary(
      child: CustomPaint(
        key: const ValueKey('ambient-backdrop'),
        isComplex: true,
        painter: AmbientPainter(spec: tokens.ambient, expressive: expressive, localProfileCue: localProfileCue),
        child: const SizedBox.expand(),
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
  AmbientPainter({required this.spec, this.expressive = false, this.localProfileCue = false});

  final AmbientSpec spec;
  final bool expressive;
  final bool localProfileCue;

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
    if (spec.grain > 0) _paintGrain(canvas, size);
  }

  /// Deterministic monochrome grain against banding (dark mode).
  void _paintGrain(Canvas canvas, Size size) {
    final count = math.min((size.width * size.height / 48).floor(), 24000);
    if (count <= 0) return;
    final light = Float32List(count);
    final dark = Float32List(count);
    var seed = 0x2545F491;
    int next() => seed = (seed * 1103515245 + 12345) & 0x7fffffff;
    var l = 0;
    var d = 0;
    for (var i = 0; i < count ~/ 2; i++) {
      final x = next() % 100000 / 100000 * size.width;
      final y = next() % 100000 / 100000 * size.height;
      if (next().isEven) {
        light[l++] = x;
        light[l++] = y;
      } else {
        dark[d++] = x;
        dark[d++] = y;
      }
    }
    final alpha = spec.grain * 2;
    canvas
      ..drawRawPoints(
        ui.PointMode.points,
        Float32List.sublistView(light, 0, l),
        Paint()
          ..color = Color.fromRGBO(255, 255, 255, alpha)
          ..strokeWidth = 1,
      )
      ..drawRawPoints(
        ui.PointMode.points,
        Float32List.sublistView(dark, 0, d),
        Paint()
          ..color = Color.fromRGBO(0, 0, 0, alpha)
          ..strokeWidth = 1,
      );
  }

  @override
  bool shouldRepaint(AmbientPainter oldDelegate) =>
      oldDelegate.spec.base != spec.base ||
      oldDelegate.spec.grain != spec.grain ||
      oldDelegate.expressive != expressive ||
      oldDelegate.localProfileCue != localProfileCue;
}
