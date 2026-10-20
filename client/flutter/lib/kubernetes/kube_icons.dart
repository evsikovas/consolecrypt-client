import 'dart:math' as math;

import 'package:material_ui/material_ui.dart';

/// Ship's-wheel glyph used for Kubernetes in navigation. Vector-drawn so it
/// follows the surrounding [IconTheme] colour (sidebar selection, accent).
class KubeWheelIcon extends StatelessWidget {
  const KubeWheelIcon({super.key, this.size, this.color});

  final double? size;
  final Color? color;

  @override
  Widget build(BuildContext context) {
    final theme = IconTheme.of(context);
    final extent = size ?? theme.size ?? 24;
    return ExcludeSemantics(
      child: SizedBox.square(
        dimension: extent,
        child: CustomPaint(painter: _WheelPainter(color ?? theme.color ?? const Color(0xFF8A8F98))),
      ),
    );
  }
}

class _WheelPainter extends CustomPainter {
  const _WheelPainter(this.color);
  final Color color;

  @override
  void paint(Canvas canvas, Size size) {
    final s = size.shortestSide;
    final c = size.center(Offset.zero);
    final stroke = s * 0.085;
    final line = Paint()
      ..color = color
      ..style = PaintingStyle.stroke
      ..strokeWidth = stroke
      ..strokeCap = StrokeCap.round;
    final fill = Paint()..color = color;
    final ring = s * 0.30;
    canvas.drawCircle(c, ring, line);
    canvas.drawCircle(c, s * 0.085, fill);
    for (var i = 0; i < 8; i++) {
      final a = i * math.pi / 4 - math.pi / 2;
      final d = Offset(math.cos(a), math.sin(a));
      canvas.drawLine(c + d * (s * 0.10), c + d * (s * 0.40), line);
      canvas.drawCircle(c + d * (s * 0.43), s * 0.055, fill);
    }
  }

  @override
  bool shouldRepaint(_WheelPainter old) => old.color != color;
}
