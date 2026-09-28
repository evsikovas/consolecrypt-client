import 'package:consolecrypt/app/theme/glass_metrics.dart';
import 'package:consolecrypt/app/theme/glass_tokens.dart';
import 'package:consolecrypt/core/glass/glass_budget.dart';
import 'package:consolecrypt/core/glass/glass_scope.dart';
import 'package:material_ui/material_ui.dart';

/// Debug overlay (gallery): live backdrop filters, refraction shaders,
/// refused requests and the resolved tier, against the §6.6 budget.
/// Developer-facing; its text is intentionally not localised.
class GlassPerfOverlay extends StatelessWidget {
  const GlassPerfOverlay({super.key});

  @override
  Widget build(BuildContext context) {
    final scope = GlassScope.of(context);
    final tokens = GlassTokens.of(context);
    final style = tokens.typography.mono.copyWith(fontSize: 11, height: 1.4, color: const Color(0xFFF5F5F7));
    return IgnorePointer(
      child: DecoratedBox(
        key: const ValueKey('glass-perf-overlay'),
        decoration: BoxDecoration(color: const Color(0xE6101114), borderRadius: BorderRadius.circular(tokens.radii.md)),
        child: Padding(
          padding: const EdgeInsets.all(GlassSpacing.s8),
          child: ValueListenableBuilder<GlassBudgetSnapshot>(
            valueListenable: scope.budget.snapshot,
            builder: (context, s, _) {
              final a = scope.appearance;
              final over = s.chromeLive > scope.budget.maxChrome || s.overlayLive > scope.budget.maxOverlay;
              final flags = [if (a.increaseContrast) 'IC', if (a.reduceMotion) 'RM', if (a.reduceTransparency) 'RT'];
              final reason = scope.solidReason == null ? '' : ' (${scope.solidReason!.name})';
              final budget =
                  '(chrome ${s.chromeLive}/${scope.budget.maxChrome}, overlay ${s.overlayLive}/${scope.budget.maxOverlay})';
              final lines = <String>[
                'tier ${a.tier.name}$reason',
                'mode ${a.mode.name}${flags.isEmpty ? '' : ' · ${flags.join(' · ')}'}',
                'live blur ${s.live} $budget${over ? ' OVER BUDGET' : ''}',
                'refraction ${s.refractive} · denied ${s.denied}',
                'suppression ${s.suppression.name}',
                'shader ${scope.refractionProgram == null ? 'not loaded' : 'loaded'}',
              ];
              return Text(lines.join('\n'), style: style);
            },
          ),
        ),
      ),
    );
  }
}
