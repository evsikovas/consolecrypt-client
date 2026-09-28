import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/l10n/labels.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:material_ui/material_ui.dart';

/// Risk level (CLIENT_SPEC §11.3) as a [GlassRiskBadge] (LIQUID_GLASS_SPEC
/// §4.12): icon + localized label + role colour, never colour alone, never
/// ellipsised. Always pass the *effective* risk.
class RiskBadge extends StatelessWidget {
  const RiskBadge({required this.risk, super.key, this.dense = false});

  final RiskLevel risk;
  final bool dense;

  @override
  Widget build(BuildContext context) => GlassRiskBadge(risk: risk, label: risk.localized(context.l10n), dense: dense);
}
