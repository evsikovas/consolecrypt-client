import 'package:consolecrypt/app/theme/glass_materials.dart';
import 'package:consolecrypt/app/theme/glass_metrics.dart';
import 'package:consolecrypt/app/theme/glass_tokens.dart';
import 'package:consolecrypt/core/glass/glass_budget.dart';
import 'package:consolecrypt/core/glass/glass_interaction.dart';
import 'package:consolecrypt/core/glass/glass_scope.dart';
import 'package:consolecrypt/core/glass/glass_strings.dart';
import 'package:consolecrypt/core/glass/glass_surface.dart';
import 'package:material_ui/material_ui.dart';

/// The secure material (LIQUID_GLASS_SPEC §2.1 `glass.secure`, §4.7, §4.13)
/// for device approval, secret reveal, risky-command confirmation,
/// Recovery Kit and password prompts.
///
/// Guarantees (tested): **no** `BackdropFilter`, **no** shader or
/// `ImageFiltered`, **no** animation, no highlight or hotspot — just an
/// effectively opaque tint (α .96; 1.0 when [opaque], in the solid tier or
/// under Increase Contrast), a flat rim and an outer shadow. The user's
/// "Clear" setting does not apply here. Entrance fades belong to the route
/// around it, never to its contents.
class SecureSurface extends StatelessWidget {
  const SecureSurface({
    required this.child,
    super.key,
    this.padding = const EdgeInsets.all(GlassSpacing.dialog),
    this.radius,
    this.opaque = false,
    this.shadow = true,
    this.suppressLiveBlur = false,
  });

  final Widget child;
  final EdgeInsetsGeometry padding;

  /// Defaults to `r.dialog`.
  final double? radius;

  /// α 1.0 regardless of settings (device approval, §4.13).
  final bool opaque;
  final bool shadow;

  /// While mounted, no live blur anywhere on screen (device approval).
  final bool suppressLiveBlur;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final scope = GlassScope.of(context);
    final appearance = scope.appearance;
    final dpr = MediaQuery.maybeDevicePixelRatioOf(context) ?? 1.0;
    final hair = GlassTokens.hairline(dpr);
    final ic = appearance.increaseContrast || tokens.highContrast;
    final alpha = tokens.tintAlpha(GlassVariant.secure, appearance, forceOpaque: opaque);
    final shape = GlassRadii.shape(radius ?? tokens.radii.dialog);
    final rim = ic ? tokens.optics.highContrastRim : tokens.optics.secureRim;
    final shadows = shadow
        ? [for (final s in tokens.material(GlassVariant.secure).shadows) s.toBoxShadow()]
        : const <BoxShadow>[];

    Widget result = CustomPaint(
      painter: GlassShadowPainter(shape: shape, shadows: shadows, darkEdge: tokens.optics.darkEdge, hairline: hair),
      child: DecoratedBox(
        key: const ValueKey('secure-surface-fill'),
        decoration: ShapeDecoration(
          color: tokens.optics.tintBase.withValues(alpha: alpha),
          shape: shape.copyWith(
            side: BorderSide(color: rim, width: ic ? 1 : hair),
          ),
        ),
        child: Padding(
          padding: padding,
          child: GlassOnGlass(
            variant: GlassVariant.secure,
            child: DefaultTextStyle.merge(
              style: tokens.typography.body.copyWith(color: tokens.palette.label),
              child: IconTheme.merge(
                data: IconThemeData(color: tokens.palette.label),
                child: KeyedSubtree(key: const ValueKey('secure-surface-content'), child: child),
              ),
            ),
          ),
        ),
      ),
    );
    if (suppressLiveBlur) {
      result = GlassBlurSuppressor(budget: scope.budget, includeOverlays: true, child: result);
    }
    return Semantics(container: true, explicitChildNodes: true, child: result);
  }
}

/// Device-approval verification code (§4.13): a fixed 3 × 2 grid that never
/// wraps; each group in a `surface.inset` box with its number above, digits
/// in the `code` style (mono 28/34, tabular figures, letter-spacing 1.5).
/// No animation, shimmer, gradient or blur. Selectable, and read group by
/// group by screen readers.
class GlassVerificationCode extends StatelessWidget {
  const GlassVerificationCode({required this.groups, super.key});

  /// The six digit groups.
  final List<String> groups;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final strings = GlassStrings.of(context);
    final ic = tokens.highContrast || GlassScope.of(context).appearance.increaseContrast;
    Widget cell(int i) {
      final digits = i < groups.length ? groups[i] : '';
      return Semantics(
        label: strings.codeGroup(i + 1, digits),
        child: ExcludeSemantics(
          child: Column(
            key: ValueKey('code-group-$i'),
            crossAxisAlignment: CrossAxisAlignment.start,
            mainAxisSize: MainAxisSize.min,
            children: [
              Text('${i + 1}', style: tokens.typography.caption.copyWith(color: tokens.palette.tertiary)),
              const SizedBox(height: GlassSpacing.s4),
              DecoratedBox(
                decoration: ShapeDecoration(
                  color: tokens.surfaces.inset,
                  shape: GlassRadii.shape(tokens.radii.md)
                      .copyWith(side: ic ? BorderSide(color: tokens.palette.label, width: 1.5) : BorderSide.none),
                ),
                child: Padding(
                  padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s16, vertical: GlassSpacing.s12),
                  child: Text(digits, style: tokens.typography.code.copyWith(color: tokens.palette.label)),
                ),
              ),
            ],
          ),
        ),
      );
    }

    Widget row(int start) => Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        cell(start),
        const SizedBox(width: GlassSpacing.s12),
        cell(start + 1),
        const SizedBox(width: GlassSpacing.s12),
        cell(start + 2),
      ],
    );

    return Semantics(
      label: strings.verificationCode,
      container: true,
      explicitChildNodes: true,
      child: SelectionArea(
        child: FittedBox(
          fit: BoxFit.scaleDown,
          alignment: AlignmentDirectional.centerStart,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            mainAxisSize: MainAxisSize.min,
            children: [
              row(0),
              const SizedBox(height: GlassSpacing.s12),
              row(3),
            ],
          ),
        ),
      ),
    );
  }
}
