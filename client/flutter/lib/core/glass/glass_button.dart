import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/app/theme/glass_materials.dart';
import 'package:consolecrypt/app/theme/glass_metrics.dart';
import 'package:consolecrypt/app/theme/glass_motion.dart';
import 'package:consolecrypt/app/theme/glass_tokens.dart';
import 'package:consolecrypt/core/glass/glass_interaction.dart';
import 'package:consolecrypt/core/glass/glass_scope.dart';
import 'package:consolecrypt/core/glass/glass_surface.dart';
import 'package:material_ui/material_ui.dart';

/// Button styles of LIQUID_GLASS_SPEC §4.2.
enum GlassButtonStyle {
  /// Secondary: `glass.thin` (a fill when already on glass).
  glass,

  /// The one primary per view or dialog: accent-tinted glass, `onAccent` label.
  prominent,

  /// Danger fill + rim, white label — only inside confirmation dialogs.
  destructive,

  /// "Delete…" entry points that open a confirmation: danger label + icon.
  destructiveQuiet,

  /// Tertiary / inline actions: no surface, accent label.
  plain,
}

/// Kit button: hover (+`fill.hover`), pressed (`fill.pressed`, scale 0.97,
/// hotspot), keyboard focus ring, disabled (content α .38), busy (14 px
/// spinner, fixed width), minimum 24 px hit target.
class GlassButton extends StatelessWidget {
  const GlassButton({
    required this.label,
    required this.onPressed,
    super.key,
    this.icon,
    this.style = GlassButtonStyle.glass,
    this.size = GlassControlSize.md,
    this.busy = false,
    this.busyLabel,
    this.focusNode,
    this.autofocus = false,
    this.tooltip,
    this.semanticLabel,
    this.expand = false,
  });

  const GlassButton.prominent({
    required this.label,
    required this.onPressed,
    super.key,
    this.icon,
    this.size = GlassControlSize.md,
    this.busy = false,
    this.busyLabel,
    this.focusNode,
    this.autofocus = false,
    this.tooltip,
    this.semanticLabel,
    this.expand = false,
  }) : style = GlassButtonStyle.prominent;

  const GlassButton.destructive({
    required this.label,
    required this.onPressed,
    super.key,
    this.icon,
    this.size = GlassControlSize.md,
    this.busy = false,
    this.busyLabel,
    this.focusNode,
    this.autofocus = false,
    this.tooltip,
    this.semanticLabel,
    this.expand = false,
  }) : style = GlassButtonStyle.destructive;

  const GlassButton.plain({
    required this.label,
    required this.onPressed,
    super.key,
    this.icon,
    this.size = GlassControlSize.md,
    this.busy = false,
    this.busyLabel,
    this.focusNode,
    this.autofocus = false,
    this.tooltip,
    this.semanticLabel,
    this.expand = false,
  }) : style = GlassButtonStyle.plain;

  final String label;

  /// `null` disables the button (a busy button is disabled too).
  final VoidCallback? onPressed;
  final IconData? icon;
  final GlassButtonStyle style;
  final GlassControlSize size;
  final bool busy;

  /// Label while [busy] (the "…ing" form); the width does not change.
  final String? busyLabel;
  final FocusNode? focusNode;
  final bool autofocus;
  final String? tooltip;
  final String? semanticLabel;

  /// Stretch to the available width.
  final bool expand;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final scope = GlassScope.of(context);
    final onGlass = AppPlatform.isMobile || GlassOnGlass.isOnGlass(context);
    // Nothing moves on the secure material (approvals, secrets, risk).
    final still = GlassOnGlass.maybeOf(context)?.secure ?? false;
    final motion = tokens.motion(scope.appearance);
    final shape = size.isCapsule
        ? const StadiumBorder() as OutlinedBorder
        : GlassRadii.shape(size == GlassControlSize.sm ? tokens.radii.sm : tokens.radii.md);
    final enabled = onPressed != null && !busy;
    final p = tokens.palette;

    final Color labelColor = switch (style) {
      GlassButtonStyle.glass => p.label,
      GlassButtonStyle.prominent => p.onAccent,
      GlassButtonStyle.destructive => p.onDangerFill,
      GlassButtonStyle.destructiveQuiet => p.danger,
      GlassButtonStyle.plain => p.accent,
    };

    Widget result = GlassInteractive(
      onPressed: enabled ? onPressed : null,
      focusNode: focusNode,
      autofocus: autofocus,
      semanticLabel: semanticLabel ?? (busy ? (busyLabel ?? label) : null),
      builder: (context, state) {
        final content = _ButtonContent(
          label: label,
          busyLabel: busyLabel,
          icon: icon,
          busy: busy,
          color: labelColor.withValues(alpha: state.enabled || busy ? 1 : 0.38),
          size: size,
          style: tokens.typography.button,
          iconSize: size == GlassControlSize.sm ? 14 : (size.isCapsule ? 18 : 16),
          expand: expand,
        );
        final overlay = state.pressed
            ? tokens.surfaces.fillPressed
            : state.hovered
            ? tokens.surfaces.fillHover
            : const Color(0x00000000);
        final padded = ColoredBox(
          color: overlay,
          child: SizedBox(
            height: AppPlatform.isMobile ? size.height.clamp(48.0, double.infinity) : size.height,
            child: Padding(
              padding: EdgeInsets.symmetric(horizontal: size.isCapsule ? size.height / 2 : 10),
              child: content,
            ),
          ),
        );
        Widget surface = switch (style) {
          GlassButtonStyle.prominent =>
            onGlass
                ? GlassFill(
                    color: p.accentFill,
                    shape: shape,
                    child: ClipPath(
                      clipper: ShapeBorderClipper(shape: shape),
                      child: padded,
                    ),
                  )
                : GlassSurface(
                    variant: GlassVariant.thin,
                    shape: shape,
                    tint: tokens.prominentTint,
                    interactive: true,
                    enabled: state.enabled,
                    child: padded,
                  ),
          GlassButtonStyle.destructive => GlassFill(
            color: p.dangerFill,
            shape: shape,
            rim: const Color(0x40FFFFFF),
            child: ClipPath(
              clipper: ShapeBorderClipper(shape: shape),
              child: padded,
            ),
          ),
          GlassButtonStyle.glass || GlassButtonStyle.destructiveQuiet =>
            onGlass
                ? GlassFill(
                    color: tokens.surfaces.fillField,
                    shape: shape,
                    child: ClipPath(
                      clipper: ShapeBorderClipper(shape: shape),
                      child: padded,
                    ),
                  )
                : GlassSurface(
                    variant: GlassVariant.thin,
                    shape: shape,
                    interactive: true,
                    enabled: state.enabled,
                    child: padded,
                  ),
          GlassButtonStyle.plain => ClipPath(
            clipper: ShapeBorderClipper(shape: shape),
            child: padded,
          ),
        };
        surface = GlassFocusRing(visible: state.focusVisible, shape: shape, child: surface);
        if (still) return surface;
        return AnimatedScale(
          scale: state.pressed && motion.allowsMovement ? GlassMotion.pressScaleControl : 1,
          duration: GlassMotion.instant,
          child: surface,
        );
      },
    );
    if (tooltip != null) result = Tooltip(message: tooltip, child: result);
    return result;
  }
}

class _ButtonContent extends StatelessWidget {
  const _ButtonContent({
    required this.label,
    required this.busyLabel,
    required this.icon,
    required this.busy,
    required this.color,
    required this.size,
    required this.style,
    required this.iconSize,
    required this.expand,
  });

  final String label;
  final String? busyLabel;
  final IconData? icon;
  final bool busy;
  final Color color;
  final GlassControlSize size;
  final TextStyle style;
  final double iconSize;
  final bool expand;

  @override
  Widget build(BuildContext context) {
    final text = style.copyWith(color: color);
    Widget row(Widget? leading, String value) => Row(
      mainAxisSize: expand ? MainAxisSize.max : MainAxisSize.min,
      mainAxisAlignment: MainAxisAlignment.center,
      children: [
        if (leading != null) ...[leading, const SizedBox(width: GlassSpacing.s6)],
        Flexible(
          child: Text(value, style: text, maxLines: 1, overflow: TextOverflow.ellipsis, softWrap: false),
        ),
      ],
    );
    final idle = row(icon == null ? null : Icon(icon, size: iconSize, color: color), label);
    if (busyLabel == null && !busy) return idle;
    final spinner = RepaintBoundary(
      child: SizedBox.square(dimension: 14, child: CircularProgressIndicator(strokeWidth: 2, color: color)),
    );
    // Both states are laid out so the width never changes.
    return Stack(
      alignment: Alignment.center,
      children: [
        Visibility(visible: !busy, maintainSize: true, maintainAnimation: true, maintainState: true, child: idle),
        Visibility(
          visible: busy,
          maintainSize: true,
          maintainAnimation: true,
          maintainState: true,
          child: row(busy ? spinner : const SizedBox.square(dimension: 14), busyLabel ?? label),
        ),
      ],
    );
  }
}

/// Visual style of [GlassIconButton].
enum GlassIconButtonStyle {
  /// `glass.thin` circle (a fill when on glass).
  glass,

  /// No surface; hover fill only (glass-free, e.g. the sidebar Lock button).
  plain,

  /// Tinted glass circle with an `onAccent` glyph: the view's one primary
  /// action when there is no room for its label (compact toolbar).
  prominent,
}

/// Circular icon button (toolbar 32, "+" tab 28). Always has a tooltip, which
/// is also its semantic label.
class GlassIconButton extends StatelessWidget {
  const GlassIconButton({
    required this.icon,
    required this.tooltip,
    required this.onPressed,
    super.key,
    this.style = GlassIconButtonStyle.glass,
    this.size = GlassSizes.toolbarButton,
    this.iconSize = GlassSizes.iconToolbar,
    this.selected = false,
    this.color,
    this.focusNode,
    this.autofocus = false,
  });

  final IconData icon;
  final String tooltip;
  final VoidCallback? onPressed;
  final GlassIconButtonStyle style;
  final double size;
  final double iconSize;

  /// Toggle state: accent icon + selection fill.
  final bool selected;
  final Color? color;
  final FocusNode? focusNode;
  final bool autofocus;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final motion = tokens.motion(GlassScope.of(context).appearance);
    final onGlass = AppPlatform.isMobile || GlassOnGlass.isOnGlass(context);
    final still = GlassOnGlass.maybeOf(context)?.secure ?? false;
    const shape = StadiumBorder();
    return Tooltip(
      message: tooltip,
      child: GlassInteractive(
        onPressed: onPressed,
        focusNode: focusNode,
        autofocus: autofocus,
        semanticLabel: tooltip,
        selected: selected ? true : null,
        builder: (context, state) {
          final prominent = style == GlassIconButtonStyle.prominent;
          final fg =
              (prominent
                      ? tokens.palette.onAccent
                      : selected
                      ? tokens.palette.accent
                      : (color ?? tokens.palette.label))
                  .withValues(alpha: state.enabled ? 1 : 0.38);
          final overlay = selected
              ? tokens.selectionOnGlass
              : state.pressed
              ? tokens.surfaces.fillPressed
              : state.hovered
              ? tokens.surfaces.fillHover
              : const Color(0x00000000);
          final glyph = SizedBox.square(
            dimension: AppPlatform.isMobile ? size.clamp(48.0, double.infinity) : size,
            child: DecoratedBox(
              decoration: ShapeDecoration(color: overlay, shape: shape),
              child: Icon(icon, size: iconSize, color: fg),
            ),
          );
          Widget surface = switch (style) {
            GlassIconButtonStyle.glass when !onGlass => GlassSurface(
              variant: GlassVariant.thin,
              shape: shape,
              interactive: true,
              enabled: state.enabled,
              child: glyph,
            ),
            GlassIconButtonStyle.glass => GlassFill(color: tokens.surfaces.fillField, shape: shape, child: glyph),
            GlassIconButtonStyle.plain => glyph,
            GlassIconButtonStyle.prominent when !onGlass => GlassSurface(
              variant: GlassVariant.thin,
              shape: shape,
              tint: tokens.prominentTint,
              interactive: true,
              enabled: state.enabled,
              child: glyph,
            ),
            GlassIconButtonStyle.prominent => GlassFill(color: tokens.palette.accentFill, shape: shape, child: glyph),
          };
          surface = GlassFocusRing(visible: state.focusVisible, shape: shape, child: surface);
          if (still) return surface;
          return AnimatedScale(
            scale: state.pressed && motion.allowsMovement ? GlassMotion.pressScaleControl : 1,
            duration: GlassMotion.instant,
            child: surface,
          );
        },
      ),
    );
  }
}
