import 'package:consolecrypt/app/theme/glass_metrics.dart';
import 'package:consolecrypt/app/theme/glass_palette.dart';
import 'package:consolecrypt/app/theme/glass_tokens.dart';
import 'package:consolecrypt/core/glass/glass_strings.dart';
import 'package:consolecrypt/core/models/snippet.dart';
import 'package:material_ui/material_ui.dart';

/// Capsule badge (LIQUID_GLASS_SPEC §4.12): height 20 (dense 16), padding 8
/// (dense 6), optional icon 14 (dense 12) + caption/600 label, background
/// = tone colour α .16; IC adds a 1 px border at α .5. Lives on content or
/// fills, never directly on clear glass. Never ellipsised.
class GlassBadge extends StatelessWidget {
  const GlassBadge({
    required this.label,
    super.key,
    this.tone = GlassTone.neutral,
    this.icon,
    this.dense = false,
    this.weight = FontWeight.w600,
  });

  final String label;
  final GlassTone tone;
  final IconData? icon;
  final bool dense;
  final FontWeight weight;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final fg = tone == GlassTone.neutral ? tokens.secondaryLabel : tokens.palette.tone(tone);
    final bg = tokens.palette.toneFill(tone).withValues(alpha: 0.16);
    final ic = tokens.highContrast;
    return DecoratedBox(
      decoration: ShapeDecoration(
        color: bg,
        shape: StadiumBorder(side: ic ? BorderSide(color: fg.withValues(alpha: 0.5)) : BorderSide.none),
      ),
      child: SizedBox(
        height: dense ? GlassSizes.badgeDense : GlassSizes.badge,
        child: Padding(
          padding: EdgeInsets.symmetric(horizontal: dense ? GlassSpacing.s6 : GlassSpacing.s8),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              if (icon != null) ...[
                Icon(icon, size: dense ? GlassSizes.iconBadgeDense : GlassSizes.iconBadge, color: fg),
                const SizedBox(width: GlassSpacing.s4),
              ],
              Text(
                label,
                softWrap: false,
                overflow: TextOverflow.visible,
                style: tokens.typography.caption.copyWith(
                  color: fg,
                  fontWeight: weight,
                  fontSize: dense ? (tokens.typography.caption.fontSize! - 1) : null,
                  height: dense ? 1.0 : null,
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// Risk badge (§4.12): icon + label + colour, never colour alone.
///
/// | Risk | Colour | Icon |
/// |---|---|---|
/// | read_only | info blue | `visibility_rounded` |
/// | unknown | unknown violet | `help_rounded` |
/// | modifying | warning amber | `edit_rounded` |
/// | destructive | danger red, weight 700 | `dangerous_rounded` |
///
/// Show the *effective* risk (local rules win over AI hints). [label] is
/// passed in so the caller controls localisation.
class GlassRiskBadge extends StatelessWidget {
  const GlassRiskBadge({required this.risk, required this.label, super.key, this.dense = false});

  final RiskLevel risk;
  final String label;
  final bool dense;

  static (GlassTone, IconData) describe(RiskLevel risk) => switch (risk) {
    RiskLevel.readOnly => (GlassTone.info, Icons.visibility_rounded),
    RiskLevel.unknown => (GlassTone.unknown, Icons.help_rounded),
    RiskLevel.modifying => (GlassTone.warning, Icons.edit_rounded),
    RiskLevel.destructive => (GlassTone.danger, Icons.dangerous_rounded),
  };

  @override
  Widget build(BuildContext context) {
    final (tone, icon) = describe(risk);
    return Semantics(
      label: GlassStrings.of(context).risk(label),
      child: ExcludeSemantics(
        child: GlassBadge(
          label: label,
          tone: tone,
          icon: icon,
          dense: dense,
          weight: risk == RiskLevel.destructive ? FontWeight.w700 : FontWeight.w600,
        ),
      ),
    );
  }
}
