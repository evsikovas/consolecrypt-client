import 'package:consolecrypt/app/theme/glass_materials.dart';
import 'package:consolecrypt/app/theme/glass_metrics.dart';
import 'package:consolecrypt/app/theme/glass_motion.dart';
import 'package:consolecrypt/app/theme/glass_tokens.dart';
import 'package:consolecrypt/core/glass/glass_interaction.dart';
import 'package:consolecrypt/core/glass/glass_scope.dart';
import 'package:consolecrypt/core/glass/glass_surface.dart';
import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

/// One option of a [GlassSegmented].
@immutable
final class GlassSegment<T> {
  const GlassSegment({required this.value, required this.label, this.icon, this.key});

  final T value;
  final String label;
  final IconData? icon;
  final Key? key;
}

/// Segmented control (LIQUID_GLASS_SPEC §4.3): `glass.thin` capsule track
/// (a `fill.field` track in content or on glass), height 28, padding 2; the
/// thumb slides with `spring.snappy` (jumps under Reduce Motion). Equal
/// segment widths, at most 5 segments. ←/→ change the selection.
class GlassSegmented<T> extends StatelessWidget {
  const GlassSegmented({
    required this.segments,
    required this.selected,
    required this.onChanged,
    super.key,
    this.inChrome = true,
    this.expand = false,
    this.semanticLabel,
  }) : assert(segments.length >= 2 && segments.length <= 5, '2–5 segments');

  final List<GlassSegment<T>> segments;
  final T selected;

  /// `null` disables the control.
  final ValueChanged<T>? onChanged;

  /// Chrome uses a glass track; content uses a fill track.
  final bool inChrome;

  /// Fill the available width instead of sizing to the widest label.
  final bool expand;
  final String? semanticLabel;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final motion = tokens.motion(GlassScope.of(context).appearance);
    final n = segments.length;
    final index = segments.indexWhere((s) => s.value == selected);
    final change = onChanged;

    void step(int delta) {
      if (change == null || index < 0) return;
      final next = (index + delta).clamp(0, n - 1);
      if (next != index) change(segments[next].value);
    }

    final thumb = index < 0
        ? const SizedBox.shrink()
        : AnimatedAlign(
            alignment: Alignment(-1 + 2 * index / (n - 1), 0),
            duration: motion.allowsMovement ? GlassMotion.medium : Duration.zero,
            curve: GlassMotion.springCurve(GlassMotion.springSnappy, GlassMotion.medium),
            child: FractionallySizedBox(
              widthFactor: 1 / n,
              heightFactor: 1,
              child: DecoratedBox(
                decoration: ShapeDecoration(
                  color: tokens.surfaces.segmentThumb,
                  shape: const StadiumBorder(),
                  shadows: const [BoxShadow(color: Color(0x1F000000), offset: Offset(0, 1), blurRadius: 3)],
                ),
              ),
            ),
          );

    final Widget row = Row(
      mainAxisSize: expand ? MainAxisSize.max : MainAxisSize.min,
      children: [
        for (final (i, s) in segments.indexed)
          Expanded(
            child: GlassInteractive(
              key: s.key,
              onPressed: change == null ? null : () => change(s.value),
              selected: i == index,
              inMutuallyExclusiveGroup: true,
              minHitSize: 0,
              builder: (context, state) {
                final color = (i == index ? tokens.palette.label : tokens.secondaryLabel).withValues(
                  alpha: state.enabled ? 1 : 0.38,
                );
                return GlassFocusRing(
                  visible: state.focusVisible,
                  shape: const StadiumBorder(),
                  child: SizedBox(
                    height: GlassSizes.segmented - 4,
                    child: Padding(
                      padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s12),
                      child: Row(
                        mainAxisAlignment: MainAxisAlignment.center,
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          if (s.icon != null) ...[
                            Icon(s.icon, size: 14, color: color),
                            const SizedBox(width: GlassSpacing.s4),
                          ],
                          Flexible(
                            child: Text(
                              s.label,
                              maxLines: 1,
                              softWrap: false,
                              overflow: TextOverflow.ellipsis,
                              style: tokens.typography.button.copyWith(color: color),
                            ),
                          ),
                        ],
                      ),
                    ),
                  ),
                );
              },
            ),
          ),
      ],
    );
    Widget body = Stack(
      children: [
        Positioned.fill(child: thumb),
        row,
      ],
    );
    if (!expand) body = IntrinsicWidth(child: body);
    body = CallbackShortcuts(
      bindings: {
        const SingleActivator(LogicalKeyboardKey.arrowLeft): () => step(-1),
        const SingleActivator(LogicalKeyboardKey.arrowRight): () => step(1),
      },
      child: Padding(padding: const EdgeInsets.all(2), child: body),
    );

    final onGlass = GlassOnGlass.isOnGlass(context);
    final Widget track = inChrome && !onGlass
        ? GlassSurface(variant: GlassVariant.thin, shape: const StadiumBorder(), child: body)
        : GlassFill(color: tokens.surfaces.fillField, shape: const StadiumBorder(), child: body);
    return Semantics(
      container: true,
      label: semanticLabel,
      child: SizedBox(height: GlassSizes.segmented, child: track),
    );
  }
}
