import 'dart:math' as math;
import 'dart:ui' as ui;

import 'package:consolecrypt/app/theme/glass_materials.dart';
import 'package:consolecrypt/app/theme/glass_metrics.dart';
import 'package:consolecrypt/app/theme/glass_motion.dart';
import 'package:consolecrypt/app/theme/glass_tokens.dart';
import 'package:consolecrypt/core/glass/glass_backdrop.dart';
import 'package:consolecrypt/core/glass/glass_budget.dart';
import 'package:consolecrypt/core/glass/glass_interaction.dart';
import 'package:consolecrypt/core/glass/glass_scope.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';

/// The one glass primitive (LIQUID_GLASS_SPEC §6.3–§6.4). Paint recipe,
/// bottom → top:
///
/// 1. outer shadows + dark edge, painted **outside** the shape only;
/// 2. clip (`ClipRSuperellipse` / capsule);
/// 3. live backdrop only when [backdrop] is [BackdropMode.live], the tier is
///    not solid and the budget grants a slot (refraction on macOS/Impeller);
/// 4. tint (+ Clear/Tinted delta, IC; opaque in the solid tier);
/// 5. inner highlight, then the pointer hotspot ([interactive]);
/// 6. rim: gradient hairline inside (flat for IC);
/// 7. child.
///
/// Persistent chrome must stay [BackdropMode.static] (tint + rim + shadow
/// over the painted ambient backdrop), so streaming content never re-blurs
/// it. Never wrap a surface in `Opacity`: animate [presence] instead.
///
/// Secrets, codes and risk decisions do not go on this widget; use
/// `SecureSurface`.
class GlassSurface extends StatefulWidget {
  const GlassSurface({
    required this.child,
    super.key,
    this.variant = GlassVariant.regular,
    this.shape,
    this.padding,
    this.backdrop = BackdropMode.static,
    this.overlay = false,
    this.interactive = false,
    this.enabled = true,
    this.tint,
    this.presence = 1,
    this.shadows = true,
    this.repaintBoundary = true,
  }) : assert(variant != GlassVariant.secure, 'Use SecureSurface for the secure material.');

  final Widget child;
  final GlassVariant variant;

  /// `RoundedSuperellipseBorder` (default: `r.panel`) or `StadiumBorder`.
  final OutlinedBorder? shape;
  final EdgeInsetsGeometry? padding;
  final BackdropMode backdrop;

  /// Transient overlay (menu, dialog, toast): its live backdrop uses its own
  /// layer and the overlay slot; if refused it falls back to the solid tier
  /// (no second live blur, §3 rule 4).
  final bool overlay;

  /// Pointer hotspot and rim tilt (frosted/refractive tiers, not IC/RM).
  final bool interactive;
  final bool enabled;

  /// Prominent / destructive fill replacing the neutral tint.
  final Color? tint;

  /// Materialize factor 0…1: blur σ, tint, rim and shadows ramp with it and
  /// the child fades (never the backdrop filter itself).
  final double presence;
  final bool shadows;

  /// Isolate the surface's painting (glass chrome must not repaint with
  /// content, §6.6).
  final bool repaintBoundary;

  @override
  State<GlassSurface> createState() => _GlassSurfaceState();
}

class _GlassSurfaceState extends State<GlassSurface> with SingleTickerProviderStateMixin {
  GlassBackdropBudget? _budget;
  bool _granted = false;
  _GlassLight? _light;
  AnimationController? _press;

  bool get _wantsLive => widget.backdrop == BackdropMode.live;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _syncBudget();
  }

  @override
  void didUpdateWidget(GlassSurface oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.backdrop != widget.backdrop || oldWidget.overlay != widget.overlay) {
      _budget?.release(this);
      _granted = false;
      _syncBudget();
    }
  }

  void _syncBudget() {
    final scope = GlassScope.of(context);
    final budget = scope.budget;
    if (!identical(budget, _budget)) {
      _budget?.release(this);
      _budget = budget;
      _granted = false;
    }
    final solid = scope.appearance.isSolid;
    if (_wantsLive && !solid) {
      _granted = budget.acquire(this, widget.overlay ? GlassBackdropKind.overlay : GlassBackdropKind.chrome);
    } else {
      budget.release(this);
      _granted = false;
    }
  }

  _GlassLight _ensureLight() {
    final light = _light ??= _GlassLight();
    if (_press == null) {
      final controller = AnimationController(vsync: this, duration: GlassMotion.instant);
      controller.addListener(() => light.pressed = controller.value);
      _press = controller;
    }
    return light;
  }

  void _onHover(PointerEvent event, Size size) {
    final light = _light;
    if (light == null) return;
    light.pointer = size.isEmpty ? null : event.localPosition;
  }

  void _onDown(PointerDownEvent event) {
    final light = _light;
    if (light == null) return;
    light.pointer = event.localPosition;
    _press?.animateTo(1, duration: GlassMotion.instant, curve: Curves.easeOut);
  }

  void _onUp(PointerEvent event) {
    _press?.animateTo(0, duration: GlassMotion.hotspotDecay, curve: Curves.easeOut);
  }

  @override
  void dispose() {
    _budget?.release(this);
    _press?.dispose();
    _light?.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final scope = GlassScope.of(context);
    final dpr = MediaQuery.maybeDevicePixelRatioOf(context) ?? 1.0;
    final hair = GlassTokens.hairline(dpr);
    final shape = widget.shape ?? GlassRadii.shape(tokens.radii.panel);

    return ValueListenableBuilder<GlassSuppression>(
      valueListenable: scope.budget.suppression,
      builder: (context, suppression, _) {
        final suppressed =
            suppression == GlassSuppression.all || (suppression == GlassSuppression.chrome && !widget.overlay);
        final live = _wantsLive && _granted && !suppressed && !scope.appearance.isSolid;
        // An overlay that may not blur must not show the content underneath
        // through a translucent tint: render it in the solid tier.
        final overlayFallback = widget.overlay && _wantsLive && !live;
        final appearance = overlayFallback ? scope.appearance.copyWith(tier: GlassTier.solid) : scope.appearance;
        final resolved = tokens.resolveSurface(
          widget.variant,
          appearance,
          live: live,
          interactive: widget.interactive && widget.enabled,
          tint: widget.tint,
          refractionAvailable: scope.refractionProgram != null,
          presence: widget.presence,
        );
        return _build(context, tokens, scope, resolved, shape, hair, dpr);
      },
    );
  }

  Widget _build(
    BuildContext context,
    GlassTokens tokens,
    GlassScopeData scope,
    ResolvedGlass resolved,
    OutlinedBorder shape,
    double hair,
    double dpr,
  ) {
    // The widget structure below only depends on constructor flags, never on
    // the resolved state, so toggling live/static, suppression or hotspot
    // keeps the child's state (focus, text, scroll).
    Widget content = GlassOnGlass(variant: widget.variant, child: widget.child);
    if (widget.padding != null) content = Padding(padding: widget.padding!, child: content);
    // Overlays materialize: the child fades, the backdrop filter never does.
    if (widget.overlay) content = Opacity(opacity: widget.presence.clamp(0.0, 1.0), child: content);
    // The outer boundary isolates this pane from its neighbours. A separate
    // content layer is also needed: otherwise a spinner, caret or streaming
    // child reruns the static fill and expensive blurred shadows every frame.
    if (widget.repaintBoundary) content = RepaintBoundary(child: content);

    final light = resolved.hotspot ? _ensureLight() : null;
    content = Stack(
      children: [
        Positioned.fill(
          key: const ValueKey('glass-fill'),
          child: CustomPaint(
            painter: GlassFillPainter(resolved: resolved, shape: shape, hairline: hair, paintRim: light == null),
          ),
        ),
        if (light != null)
          Positioned.fill(
            key: const ValueKey('glass-light'),
            child: RepaintBoundary(
              child: CustomPaint(
                painter: _GlassLightPainter(light: light, resolved: resolved, shape: shape, hairline: hair),
              ),
            ),
          ),
        KeyedSubtree(key: const ValueKey('glass-content'), child: content),
      ],
    );

    if (_wantsLive) {
      content = GlassBackdrop(
        enabled: resolved.isLive,
        sigma: resolved.blurSigma,
        saturation: resolved.saturation,
        refraction: resolved.refraction,
        program: scope.refractionProgram,
        cornerRadius: _cornerRadius(shape),
        devicePixelRatio: dpr,
        grouped: !widget.overlay,
        onRefractionChanged: (on) => _budget?.markRefractive(this, refractive: on),
        child: content,
      );
    }

    content = GlassClip(shape: shape, child: content);

    content = CustomPaint(
      painter: GlassShadowPainter(
        shape: shape,
        shadows: widget.shadows ? resolved.shadows : const [],
        darkEdge: resolved.darkEdge,
        hairline: hair,
      ),
      child: content,
    );

    if (widget.interactive) {
      content = MouseRegion(
        opaque: false,
        onHover: (e) => _onHover(e, context.size ?? Size.zero),
        onExit: (_) => _light?.pointer = null,
        child: Listener(onPointerDown: _onDown, onPointerUp: _onUp, onPointerCancel: _onUp, child: content),
      );
    }
    if (widget.repaintBoundary) content = RepaintBoundary(child: content);
    return content;
  }

  static double _cornerRadius(OutlinedBorder shape) {
    if (shape is StadiumBorder) return 1e4;
    if (shape is RoundedSuperellipseBorder) {
      final r = shape.borderRadius.resolve(TextDirection.ltr);
      return r.topLeft.x;
    }
    return 0;
  }
}

/// Pointer state for the hotspot/tilt painter (no rebuild on hover).
class _GlassLight extends ChangeNotifier {
  Offset? _pointer;
  double _pressed = 0;

  Offset? get pointer => _pointer;
  set pointer(Offset? value) {
    if (value == _pointer) return;
    _pointer = value;
    notifyListeners();
  }

  double get pressed => _pressed;
  set pressed(double value) {
    if (value == _pressed) return;
    _pressed = value;
    notifyListeners();
  }
}

/// Clips to a glass shape: `ClipRSuperellipse` for continuous corners, an
/// exact capsule for `StadiumBorder`, a path otherwise.
class GlassClip extends StatelessWidget {
  const GlassClip({required this.shape, required this.child, super.key});

  final OutlinedBorder shape;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    final s = shape;
    if (s is RoundedSuperellipseBorder) return ClipRSuperellipse(borderRadius: s.borderRadius, child: child);
    if (s is StadiumBorder) return ClipRRect(clipper: const _CapsuleClipper(), child: child);
    return ClipPath(
      clipper: ShapeBorderClipper(shape: s),
      child: child,
    );
  }
}

class _CapsuleClipper extends CustomClipper<RRect> {
  const _CapsuleClipper();

  @override
  RRect getClip(Size size) => RRect.fromRectAndRadius(Offset.zero & size, Radius.circular(size.shortestSide / 2));

  @override
  bool shouldReclip(_CapsuleClipper oldClipper) => false;
}

Path _outsideOf(Path shapePath, Rect rect, double extent) => Path()
  ..fillType = PathFillType.evenOdd
  ..addRect(rect.inflate(extent))
  ..addPath(shapePath, Offset.zero);

/// Outer shadows + dark edge, both strictly outside the shape.
class GlassShadowPainter extends CustomPainter {
  const GlassShadowPainter({
    required this.shape,
    required this.shadows,
    required this.darkEdge,
    required this.hairline,
  });

  final OutlinedBorder shape;
  final List<BoxShadow> shadows;
  final Color darkEdge;
  final double hairline;

  @override
  void paint(Canvas canvas, Size size) {
    if (size.isEmpty) return;
    final rect = Offset.zero & size;
    final path = shape.getOuterPath(rect);
    var extent = 4.0;
    for (final s in shadows) {
      extent = math.max(extent, s.blurRadius * 2 + s.offset.distance + s.spreadRadius);
    }
    final outside = _outsideOf(path, rect, extent);
    canvas
      ..save()
      ..clipPath(outside);
    for (final s in shadows) {
      if (s.color.a == 0) continue;
      canvas.drawPath(
        path.shift(s.offset),
        Paint()
          ..color = s.color
          ..maskFilter = MaskFilter.blur(BlurStyle.normal, s.blurSigma),
      );
    }
    if (darkEdge.a > 0) {
      canvas.drawPath(
        path,
        Paint()
          ..style = PaintingStyle.stroke
          ..strokeWidth = hairline * 2
          ..color = darkEdge,
      );
    }
    canvas.restore();
  }

  @override
  bool shouldRepaint(GlassShadowPainter old) =>
      old.shape != shape || !listEquals(old.shadows, shadows) || old.darkEdge != darkEdge || old.hairline != hairline;
}

/// Tint, inner highlight and (unless the light painter owns it) the rim.
/// Painted inside the clip.
class GlassFillPainter extends CustomPainter {
  const GlassFillPainter({required this.resolved, required this.shape, required this.hairline, this.paintRim = true});

  final ResolvedGlass resolved;
  final OutlinedBorder shape;
  final double hairline;
  final bool paintRim;

  @override
  void paint(Canvas canvas, Size size) {
    if (size.isEmpty) return;
    final rect = Offset.zero & size;
    canvas.drawRect(rect, Paint()..color = resolved.tint);
    final top = resolved.highlightTop;
    if (top != null && top.a > 0) {
      final h = math.min(size.height, resolved.highlightHeight);
      final band = Rect.fromLTWH(0, 0, size.width, h);
      canvas.drawRect(
        band,
        Paint()..shader = ui.Gradient.linear(band.topCenter, band.bottomCenter, [top, top.withValues(alpha: 0)]),
      );
    }
    if (paintRim) paintGlassRim(canvas, rect, shape, resolved.rimColors, resolved.rimAngleDegrees, hairline);
  }

  @override
  bool shouldRepaint(GlassFillPainter old) =>
      old.resolved.tint != resolved.tint ||
      old.resolved.highlightTop != resolved.highlightTop ||
      !listEquals(old.resolved.rimColors, resolved.rimColors) ||
      old.shape != shape ||
      old.hairline != hairline ||
      old.paintRim != paintRim;
}

/// Strokes the rim hairline inside [shape]. A flat rim when all colours are
/// equal; otherwise a linear gradient at [angleDegrees] (CSS convention:
/// 135° runs top-left → bottom-right).
void paintGlassRim(
  Canvas canvas,
  Rect rect,
  OutlinedBorder shape,
  List<Color> colors,
  double angleDegrees,
  double hair,
) {
  if (colors.every((c) => c.a == 0)) return;
  final path = shape.getOuterPath(rect);
  final paint = Paint()
    ..style = PaintingStyle.stroke
    // Centered on the edge; the outer half is clipped away → `hair` inside.
    ..strokeWidth = hair * 2;
  if (colors.every((c) => c == colors.first)) {
    paint.color = colors.first;
  } else {
    final a = angleDegrees * math.pi / 180;
    final dir = Offset(math.sin(a), -math.cos(a));
    final half = (rect.width.abs() * dir.dx.abs() + rect.height.abs() * dir.dy.abs()) / 2;
    final center = rect.center;
    paint.shader = ui.Gradient.linear(center - dir * half, center + dir * half, colors, GlassOptics.rimStopPositions);
  }
  canvas
    ..save()
    ..clipPath(path)
    ..drawPath(path, paint)
    ..restore();
}

/// Pointer hotspot (radial light at the pointer, brighter while pressed)
/// and the rim with pointer tilt (±10°).
class _GlassLightPainter extends CustomPainter {
  _GlassLightPainter({required this.light, required this.resolved, required this.shape, required this.hairline})
    : super(repaint: light);

  final _GlassLight light;
  final ResolvedGlass resolved;
  final OutlinedBorder shape;
  final double hairline;

  @override
  void paint(Canvas canvas, Size size) {
    if (size.isEmpty) return;
    final rect = Offset.zero & size;
    final pointer = light.pointer;
    var angle = resolved.rimAngleDegrees;
    if (pointer != null) {
      final t = Color.lerp(resolved.hotspotColor, resolved.hotspotPressedColor, light.pressed)!;
      canvas.drawCircle(
        pointer,
        resolved.hotspotRadius,
        Paint()..shader = ui.Gradient.radial(pointer, resolved.hotspotRadius, [t, t.withValues(alpha: 0)]),
      );
      final dx = size.width == 0 ? 0.0 : (pointer.dx / size.width - 0.5) * 2;
      angle += dx.clamp(-1.0, 1.0) * GlassMotion.maxTiltDegrees;
    }
    paintGlassRim(canvas, rect, shape, resolved.rimColors, angle, hairline);
  }

  @override
  bool shouldRepaint(_GlassLightPainter old) =>
      !identical(old.light, light) || old.resolved != resolved || old.shape != shape || old.hairline != hairline;
}

/// [GlassSurface] with a continuous-corner rectangle and padding (sidebar,
/// dialog body, popovers).
class GlassPanel extends StatelessWidget {
  const GlassPanel({
    required this.child,
    super.key,
    this.variant = GlassVariant.regular,
    this.radius,
    this.padding = const EdgeInsets.all(GlassSpacing.card),
    this.backdrop = BackdropMode.static,
    this.overlay = false,
    this.presence = 1,
    this.width,
    this.height,
  });

  final Widget child;
  final GlassVariant variant;

  /// Defaults to `r.panel`.
  final double? radius;
  final EdgeInsetsGeometry padding;
  final BackdropMode backdrop;
  final bool overlay;
  final double presence;
  final double? width;
  final double? height;

  @override
  Widget build(BuildContext context) {
    final r = radius ?? GlassTokens.of(context).radii.panel;
    Widget result = GlassSurface(
      variant: variant,
      shape: GlassRadii.shape(r),
      padding: padding,
      backdrop: backdrop,
      overlay: overlay,
      presence: presence,
      child: child,
    );
    if (width != null || height != null) result = SizedBox(width: width, height: height, child: result);
    return result;
  }
}

/// [GlassSurface] as a capsule (toolbar groups, status pill, toasts).
class GlassCapsule extends StatelessWidget {
  const GlassCapsule({
    required this.child,
    super.key,
    this.variant = GlassVariant.thin,
    this.height = GlassSizes.toolbarGroup,
    this.padding = const EdgeInsets.symmetric(horizontal: GlassSpacing.s12),
    this.backdrop = BackdropMode.static,
    this.overlay = false,
    this.interactive = false,
    this.presence = 1,
  });

  final Widget child;
  final GlassVariant variant;

  /// `null` = size to the child.
  final double? height;
  final EdgeInsetsGeometry padding;
  final BackdropMode backdrop;
  final bool overlay;
  final bool interactive;
  final double presence;

  @override
  Widget build(BuildContext context) => GlassSurface(
    variant: variant,
    shape: const StadiumBorder(),
    padding: padding,
    backdrop: backdrop,
    overlay: overlay,
    interactive: interactive,
    presence: presence,
    child: height == null
        ? child
        : SizedBox(
            height: height,
            child: Center(widthFactor: 1, child: child),
          ),
  );
}
