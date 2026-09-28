import 'dart:async';
import 'dart:math' as math;

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/app/theme/glass_materials.dart';
import 'package:consolecrypt/app/theme/glass_metrics.dart';
import 'package:consolecrypt/app/theme/glass_motion.dart';
import 'package:consolecrypt/app/theme/glass_tokens.dart';
import 'package:consolecrypt/core/glass/glass_interaction.dart';
import 'package:consolecrypt/core/glass/glass_scope.dart';
import 'package:consolecrypt/core/glass/glass_strings.dart';
import 'package:consolecrypt/core/glass/glass_surface.dart';
import 'package:material_ui/material_ui.dart';

/// An entry of a [showGlassMenu] menu.
sealed class GlassMenuEntry<T> {
  const GlassMenuEntry();
}

/// A selectable menu item (height 26, leading icon 16, trailing shortcut).
/// With a [subtitle] the item grows to two lines (40 high).
final class GlassMenuItem<T> extends GlassMenuEntry<T> {
  const GlassMenuItem({
    required this.value,
    required this.label,
    this.icon,
    this.subtitle,
    this.checked,
    this.shortcut,
    this.destructive = false,
    this.enabled = true,
    this.key,
  });

  final T value;
  final String label;
  final IconData? icon;

  /// Secondary line (callout), e.g. a profile's kind and server.
  final String? subtitle;

  /// Choice menus: `true` shows a check mark, `false` reserves its space so
  /// labels stay aligned; `null` (actions) reserves nothing.
  final bool? checked;

  /// Shortcut hint, e.g. "⌘K" / "Ctrl+K".
  final String? shortcut;

  /// Danger label and icon; never pre-selected.
  final bool destructive;
  final bool enabled;
  final Key? key;
}

/// A hairline separator with 6 px vertical margins.
final class GlassMenuDivider<T> extends GlassMenuEntry<T> {
  const GlassMenuDivider();
}

/// Global rect of the widget at [context] (menu/popover anchor).
Rect glassAnchorRect(BuildContext context) {
  final box = context.findRenderObject()! as RenderBox;
  return box.localToGlobal(Offset.zero) & box.size;
}

/// Shows a `glass.regular` menu (LIQUID_GLASS_SPEC §4.5) under [anchor]
/// (global rect; flips above when there is no room). Live backdrop in its
/// own layer (the overlay slot); a menu opened while another overlay holds
/// the slot renders solid. Appears with fade + scale from 0.96 over 180 ms,
/// dismisses with a 120 ms fade. ↑/↓ move, Enter selects, Esc closes.
Future<T?> showGlassMenu<T>({
  required BuildContext context,
  required Rect anchor,
  required List<GlassMenuEntry<T>> entries,
  double minWidth = GlassSizes.menuMinWidth,
}) => _pushOverlay<T>(
  context,
  anchor: anchor,
  radius: GlassTokens.of(context).radii.menu,
  padding: const EdgeInsets.all(GlassSpacing.s6),
  builder: (context) => _GlassMenuBody<T>(entries: entries, minWidth: minWidth),
);

/// Shows arbitrary content on a `glass.regular` popover anchored at
/// [anchor] (e.g. the sync-status details).
Future<T?> showGlassPopover<T>({
  required BuildContext context,
  required Rect anchor,
  required WidgetBuilder builder,
  double? width,
}) => _pushOverlay<T>(
  context,
  anchor: anchor,
  radius: GlassTokens.of(context).radii.menu,
  padding: const EdgeInsets.all(GlassSpacing.s12),
  builder: (context) => width == null ? builder(context) : SizedBox(width: width, child: builder(context)),
);

Future<T?> _pushOverlay<T>(
  BuildContext context, {
  required Rect anchor,
  required double radius,
  required EdgeInsets padding,
  required WidgetBuilder builder,
}) {
  final navigator = Navigator.of(context, rootNavigator: true);
  return navigator.push(
    _GlassAnchoredRoute<T>(
      anchor: anchor,
      radius: radius,
      padding: padding,
      builder: builder,
      themes: InheritedTheme.capture(from: context, to: navigator.context),
      scope: GlassScope.of(context),
      barrierLabel: GlassStrings.of(context).dismiss,
    ),
  );
}

class _GlassAnchoredRoute<T> extends PopupRoute<T> {
  _GlassAnchoredRoute({
    required this.anchor,
    required this.radius,
    required this.padding,
    required this.builder,
    required this.themes,
    required this.scope,
    required this.barrierLabel,
  });

  final Rect anchor;
  final double radius;
  final EdgeInsets padding;
  final WidgetBuilder builder;
  final CapturedThemes themes;
  final GlassScopeData scope;

  @override
  final String barrierLabel;

  @override
  Color? get barrierColor => null;

  @override
  bool get barrierDismissible => true;

  @override
  Duration get transitionDuration => GlassMotion.base;

  @override
  Duration get reverseTransitionDuration => GlassMotion.fast;

  @override
  Widget buildPage(BuildContext context, Animation<double> animation, Animation<double> secondaryAnimation) {
    final flipped = _flipsAbove(MediaQuery.sizeOf(context));
    return themes.wrap(
      GlassScope(
        data: scope,
        child: CustomSingleChildLayout(
          delegate: _AnchoredLayout(anchor: anchor, flipped: flipped),
          child: AnimatedBuilder(
            animation: animation,
            builder: (context, child) {
              final reverse = animation.status == AnimationStatus.reverse;
              final t = (reverse ? GlassMotion.dismiss : GlassMotion.appear).transform(animation.value);
              final movement = !scope.appearance.reduceMotion && !reverse;
              final scale = movement ? GlassMotion.materializeScale + (1 - GlassMotion.materializeScale) * t : 1.0;
              return Transform.scale(
                scale: scale,
                alignment: flipped ? Alignment.bottomCenter : Alignment.topCenter,
                child: GlassSurface(
                  variant: GlassVariant.regular,
                  shape: GlassRadii.shape(radius),
                  backdrop: BackdropMode.live,
                  overlay: true,
                  presence: t,
                  padding: padding,
                  child: child!,
                ),
              );
            },
            child: Material(
              type: MaterialType.transparency,
              child: Builder(builder: builder),
            ),
          ),
        ),
      ),
    );
  }

  bool _flipsAbove(Size screen) {
    final below = screen.height - anchor.bottom;
    return below < 240 && anchor.top > below;
  }
}

class _AnchoredLayout extends SingleChildLayoutDelegate {
  const _AnchoredLayout({required this.anchor, required this.flipped});

  static const _margin = 8.0;
  static const _gap = 4.0;

  final Rect anchor;
  final bool flipped;

  @override
  BoxConstraints getConstraintsForChild(BoxConstraints constraints) {
    final maxHeight = flipped ? anchor.top - _gap - _margin : constraints.maxHeight - anchor.bottom - _gap - _margin;
    return BoxConstraints(maxWidth: math.max(0, constraints.maxWidth - 2 * _margin), maxHeight: math.max(0, maxHeight));
  }

  @override
  Offset getPositionForChild(Size size, Size childSize) {
    final x = anchor.left.clamp(_margin, math.max(_margin, size.width - childSize.width - _margin)).toDouble();
    final y = flipped ? anchor.top - _gap - childSize.height : anchor.bottom + _gap;
    return Offset(x, y.clamp(_margin, math.max(_margin, size.height - childSize.height - _margin)).toDouble());
  }

  @override
  bool shouldRelayout(_AnchoredLayout old) => old.anchor != anchor || old.flipped != flipped;
}

class _GlassMenuBody<T> extends StatelessWidget {
  const _GlassMenuBody({required this.entries, required this.minWidth});

  final List<GlassMenuEntry<T>> entries;
  final double minWidth;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final firstEnabled = entries.indexWhere((e) => e is GlassMenuItem<T> && e.enabled && !e.destructive);
    return IntrinsicWidth(
      child: ConstrainedBox(
        constraints: BoxConstraints(minWidth: minWidth),
        child: SingleChildScrollView(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            mainAxisSize: MainAxisSize.min,
            children: [
              for (final (i, entry) in entries.indexed)
                switch (entry) {
                  GlassMenuDivider<T>() => Padding(
                    padding: const EdgeInsets.symmetric(vertical: GlassSpacing.s6),
                    child: SizedBox(height: 1, child: ColoredBox(color: tokens.surfaces.separator)),
                  ),
                  final GlassMenuItem<T> item => _GlassMenuTile<T>(item: item, autofocus: i == firstEnabled),
                },
            ],
          ),
        ),
      ),
    );
  }
}

class _GlassMenuTile<T> extends StatelessWidget {
  const _GlassMenuTile({required this.item, required this.autofocus});

  final GlassMenuItem<T> item;
  final bool autofocus;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final p = tokens.palette;
    final radius = GlassRadii.concentric(tokens.radii.menu, GlassSpacing.s6, height: GlassSizes.menuItem);
    return GlassInteractive(
      key: item.key,
      autofocus: autofocus,
      minHitSize: 0,
      onPressed: item.enabled ? () => Navigator.of(context).pop(item.value) : null,
      builder: (context, state) {
        final highlighted = state.enabled && (state.hovered || state.focusVisible || state.pressed);
        final Color fg;
        final Color bg;
        if (!state.enabled) {
          fg = p.label.withValues(alpha: 0.38);
          bg = const Color(0x00000000);
        } else if (highlighted) {
          fg = item.destructive ? p.onDangerFill : p.onAccent;
          bg = item.destructive ? p.dangerFill : p.accentFill;
        } else {
          fg = item.destructive ? p.danger : p.label;
          bg = const Color(0x00000000);
        }
        final label = Text(
          item.label,
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: tokens.typography.body.copyWith(color: fg, fontWeight: item.destructive ? FontWeight.w600 : null),
        );
        final subtitle = item.subtitle;
        return DecoratedBox(
          decoration: ShapeDecoration(color: bg, shape: GlassRadii.shape(radius)),
          child: SizedBox(
            height: AppPlatform.isMobile
                ? (subtitle == null ? 48 : 64)
                : subtitle == null
                ? GlassSizes.menuItem
                : GlassSizes.menuItem + 14,
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s8),
              child: Row(
                children: [
                  if (item.checked != null) ...[
                    SizedBox.square(
                      dimension: GlassSizes.iconMenu,
                      child: item.checked! ? Icon(Icons.check_rounded, size: GlassSizes.iconMenu, color: fg) : null,
                    ),
                    const SizedBox(width: GlassSpacing.s6),
                  ],
                  if (item.icon != null) ...[
                    Icon(item.icon, size: GlassSizes.iconMenu, color: fg),
                    const SizedBox(width: GlassSpacing.s8),
                  ],
                  Expanded(
                    child: subtitle == null
                        ? label
                        : Column(
                            mainAxisAlignment: MainAxisAlignment.center,
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              label,
                              Text(
                                subtitle,
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style: tokens.typography.caption.copyWith(
                                  color: highlighted ? fg : tokens.secondaryLabel,
                                  fontWeight: FontWeight.w400,
                                ),
                              ),
                            ],
                          ),
                  ),
                  if (item.shortcut != null) ...[
                    const SizedBox(width: GlassSpacing.s16),
                    Text(
                      item.shortcut!,
                      style: tokens.typography.callout.copyWith(color: highlighted ? fg : tokens.palette.tertiary),
                    ),
                  ],
                ],
              ),
            ),
          ),
        );
      },
    );
  }
}

/// Opens a [showGlassMenu] anchored at itself.
class GlassMenuButton<T> extends StatelessWidget {
  const GlassMenuButton({required this.entries, required this.onSelected, required this.builder, super.key});

  final List<GlassMenuEntry<T>> entries;
  final ValueChanged<T> onSelected;

  /// Builds the trigger; call `open` from its `onPressed`.
  final Widget Function(BuildContext context, VoidCallback open) builder;

  @override
  Widget build(BuildContext context) => Builder(
    builder: (context) => builder(context, () {
      unawaited(
        showGlassMenu<T>(context: context, anchor: glassAnchorRect(context), entries: entries).then((value) {
          if (value != null) onSelected(value);
        }),
      );
    }),
  );
}
