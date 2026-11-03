import 'dart:ui' show lerpDouble;

import 'package:material_ui/material_ui.dart';

/// Text-field border of LIQUID_GLASS_SPEC §4.4 for `material_ui` inputs
/// (`TextFormField` with validators, dropdown fields): the field is a
/// `fill.field` shape with continuous corners (`r.md`), no stroke at rest, a
/// subtle underline on focus and a danger underline on error.
///
/// It is not an outline border, so the floating label sits inside the fill
/// (like `GlassField`, whose fill it matches) instead of on a stroke.
class GlassInputBorder extends InputBorder {
  const GlassInputBorder({required this.radius, super.borderSide = BorderSide.none});

  final double radius;

  @override
  bool get isOutline => false;

  @override
  EdgeInsetsGeometry get dimensions => EdgeInsets.zero;

  OutlinedBorder get _shape => RoundedSuperellipseBorder(borderRadius: BorderRadius.circular(radius));

  @override
  GlassInputBorder copyWith({BorderSide? borderSide, double? radius}) =>
      GlassInputBorder(radius: radius ?? this.radius, borderSide: borderSide ?? this.borderSide);

  @override
  Path getInnerPath(Rect rect, {TextDirection? textDirection}) =>
      _shape.getInnerPath(rect.deflate(borderSide.width), textDirection: textDirection);

  @override
  Path getOuterPath(Rect rect, {TextDirection? textDirection}) =>
      _shape.getOuterPath(rect, textDirection: textDirection);

  @override
  void paint(
    Canvas canvas,
    Rect rect, {
    double? gapStart,
    double gapExtent = 0.0,
    double gapPercentage = 0.0,
    TextDirection? textDirection,
  }) {
    if (borderSide.style == BorderStyle.none || borderSide.width == 0) return;
    // Focus/error stays below the text and floating label.
    final y = rect.bottom - borderSide.width / 2;
    canvas.drawLine(Offset(rect.left + radius, y), Offset(rect.right - radius, y), borderSide.toPaint());
  }

  @override
  ShapeBorder scale(double t) => GlassInputBorder(radius: radius * t, borderSide: borderSide.scale(t));

  @override
  ShapeBorder? lerpFrom(ShapeBorder? a, double t) {
    if (a is GlassInputBorder) {
      return GlassInputBorder(
        radius: lerpDouble(a.radius, radius, t)!,
        borderSide: BorderSide.lerp(a.borderSide, borderSide, t),
      );
    }
    return super.lerpFrom(a, t);
  }

  @override
  ShapeBorder? lerpTo(ShapeBorder? b, double t) {
    if (b is GlassInputBorder) {
      return GlassInputBorder(
        radius: lerpDouble(radius, b.radius, t)!,
        borderSide: BorderSide.lerp(borderSide, b.borderSide, t),
      );
    }
    return super.lerpTo(b, t);
  }

  @override
  bool operator ==(Object other) =>
      other is GlassInputBorder && other.radius == radius && other.borderSide == borderSide;

  @override
  int get hashCode => Object.hash(radius, borderSide);
}
