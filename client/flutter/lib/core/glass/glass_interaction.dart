import 'dart:math' as math;

import 'package:consolecrypt/app/theme/glass_materials.dart';
import 'package:consolecrypt/app/theme/glass_metrics.dart';
import 'package:consolecrypt/app/theme/glass_tokens.dart';
import 'package:flutter/widgets.dart';

/// Marks a subtree as sitting on glass. Controls read it to follow "no glass
/// on glass" (§1.4, §3 rule 4): on glass they use fills instead of another
/// glass pane.
class GlassOnGlass extends InheritedWidget {
  const GlassOnGlass({required this.variant, required super.child, super.key});

  /// Material of the surface underneath.
  final GlassVariant variant;

  /// The surface underneath is the secure material.
  bool get secure => variant == GlassVariant.secure;

  static GlassOnGlass? maybeOf(BuildContext context) => context.dependOnInheritedWidgetOfExactType<GlassOnGlass>();

  static bool isOnGlass(BuildContext context) => maybeOf(context) != null;

  @override
  bool updateShouldNotify(GlassOnGlass oldWidget) => variant != oldWidget.variant;
}

/// Interaction state handed to [GlassInteractive.builder].
@immutable
final class GlassInteractionState {
  const GlassInteractionState({
    this.hovered = false,
    this.pressed = false,
    this.focused = false,
    this.focusVisible = false,
    this.enabled = true,
  });

  final bool hovered;
  final bool pressed;
  final bool focused;

  /// Keyboard focus (show the focus ring).
  final bool focusVisible;
  final bool enabled;
}

/// Hover / press / keyboard focus / activation / semantics for kit controls.
///
/// Enter and Space activate (via the app's default `ActivateIntent`
/// shortcuts); the hit area is at least [minHitSize] (WCAG 2.5.8).
class GlassInteractive extends StatefulWidget {
  const GlassInteractive({
    required this.builder,
    required this.onPressed,
    super.key,
    this.focusNode,
    this.autofocus = false,
    this.semanticLabel,
    this.selected,
    this.button = true,
    this.inMutuallyExclusiveGroup = false,
    this.canRequestFocus = true,
    this.minHitSize = GlassSizes.minHitTarget,
    this.cursor = SystemMouseCursors.click,
    this.shortcuts,
    this.onHoverChanged,
  });

  final Widget Function(BuildContext context, GlassInteractionState state) builder;

  /// `null` disables the control.
  final VoidCallback? onPressed;
  final FocusNode? focusNode;
  final bool autofocus;
  final String? semanticLabel;
  final bool? selected;
  final bool button;
  final bool inMutuallyExclusiveGroup;
  final bool canRequestFocus;
  final double minHitSize;
  final MouseCursor cursor;
  final Map<ShortcutActivator, Intent>? shortcuts;
  final ValueChanged<bool>? onHoverChanged;

  @override
  State<GlassInteractive> createState() => _GlassInteractiveState();
}

class _GlassInteractiveState extends State<GlassInteractive> {
  bool _hovered = false;
  bool _pressed = false;
  bool _focused = false;
  bool _focusVisible = false;

  bool get _enabled => widget.onPressed != null;

  void _activate() => widget.onPressed?.call();

  @override
  void didUpdateWidget(GlassInteractive oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!_enabled && (_pressed || _hovered)) {
      _pressed = false;
      _hovered = false;
    }
  }

  @override
  Widget build(BuildContext context) {
    final state = GlassInteractionState(
      hovered: _hovered && _enabled,
      pressed: _pressed && _enabled,
      focused: _focused,
      focusVisible: _focusVisible && _enabled,
      enabled: _enabled,
    );
    Widget child = widget.builder(context, state);
    child = ConstrainedBox(
      constraints: BoxConstraints(minWidth: widget.minHitSize, minHeight: widget.minHitSize),
      child: Center(widthFactor: 1, heightFactor: 1, child: child),
    );
    child = GestureDetector(
      behavior: HitTestBehavior.opaque,
      onTapDown: _enabled ? (_) => setState(() => _pressed = true) : null,
      onTapUp: _enabled ? (_) => setState(() => _pressed = false) : null,
      onTapCancel: _enabled ? () => setState(() => _pressed = false) : null,
      onTap: _enabled ? _activate : null,
      excludeFromSemantics: true,
      child: child,
    );
    child = FocusableActionDetector(
      enabled: _enabled,
      focusNode: widget.focusNode,
      autofocus: widget.autofocus,
      descendantsAreFocusable: false,
      mouseCursor: _enabled ? widget.cursor : SystemMouseCursors.basic,
      shortcuts: widget.shortcuts,
      actions: {ActivateIntent: CallbackAction<ActivateIntent>(onInvoke: (_) => _activate())},
      onShowHoverHighlight: (v) {
        setState(() => _hovered = v);
        widget.onHoverChanged?.call(v);
      },
      onShowFocusHighlight: (v) => setState(() => _focusVisible = v),
      onFocusChange: (v) => setState(() => _focused = v),
      child: child,
    );
    return MergeSemantics(
      child: Semantics(
        button: widget.button,
        enabled: _enabled,
        selected: widget.selected,
        inMutuallyExclusiveGroup: widget.inMutuallyExclusiveGroup,
        label: widget.semanticLabel,
        onTap: _enabled ? _activate : null,
        child: child,
      ),
    );
  }
}

/// Keyboard focus ring (§4.2): 2 px accent at offset 2; 3 px in the label
/// colour under Increase Contrast. Painted outside the child, concentric
/// with [shape].
class GlassFocusRing extends StatelessWidget {
  const GlassFocusRing({required this.visible, required this.shape, required this.child, super.key});

  final bool visible;
  final OutlinedBorder shape;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    return CustomPaint(
      foregroundPainter: visible
          ? _FocusRingPainter(
              shape: shape,
              color: tokens.focusRingColor,
              width: tokens.focusRingWidth,
              offset: tokens.focusRingOffset,
            )
          : null,
      child: child,
    );
  }
}

/// Returns [shape] grown by [delta] with concentric corners.
OutlinedBorder growShape(OutlinedBorder shape, double delta) {
  if (shape is RoundedSuperellipseBorder) {
    final r = shape.borderRadius.resolve(TextDirection.ltr);
    Radius g(Radius x) => Radius.circular(math.max(0, x.x + delta));
    return RoundedSuperellipseBorder(
      borderRadius: BorderRadius.only(
        topLeft: g(r.topLeft),
        topRight: g(r.topRight),
        bottomLeft: g(r.bottomLeft),
        bottomRight: g(r.bottomRight),
      ),
    );
  }
  return shape;
}

class _FocusRingPainter extends CustomPainter {
  const _FocusRingPainter({required this.shape, required this.color, required this.width, required this.offset});

  final OutlinedBorder shape;
  final Color color;
  final double width;
  final double offset;

  @override
  void paint(Canvas canvas, Size size) {
    final grow = offset + width / 2;
    final rect = (Offset.zero & size).inflate(grow);
    canvas.drawPath(
      growShape(shape, grow).getOuterPath(rect),
      Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = width
        ..color = color,
    );
  }

  @override
  bool shouldRepaint(_FocusRingPainter old) =>
      old.shape != shape || old.color != color || old.width != width || old.offset != offset;
}

/// A plain (non-glass) filled shape with an optional hairline rim: accent
/// and danger fills, fills on glass.
class GlassFill extends StatelessWidget {
  const GlassFill({required this.color, required this.shape, required this.child, super.key, this.rim});

  final Color color;
  final OutlinedBorder shape;
  final Color? rim;
  final Widget child;

  @override
  Widget build(BuildContext context) => DecoratedBox(
    decoration: ShapeDecoration(
      color: color,
      shape: rim == null ? shape : shape.copyWith(side: BorderSide(color: rim!, width: 1)),
    ),
    child: child,
  );
}
