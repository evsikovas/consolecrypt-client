import 'dart:async';
import 'dart:math' as math;

import 'package:consolecrypt/app/theme/glass_metrics.dart';
import 'package:consolecrypt/app/theme/glass_tokens.dart';
import 'package:consolecrypt/core/glass/glass_interaction.dart';
import 'package:consolecrypt/core/glass/glass_menu.dart';
import 'package:material_ui/material_ui.dart';

/// One option of a [GlassSelect].
@immutable
final class GlassSelectItem<T> {
  const GlassSelectItem({required this.value, required this.label, this.icon, this.key});

  final T value;
  final String label;
  final IconData? icon;

  /// Key of the menu item (for tests).
  final Key? key;
}

/// Pop-up choice (the kit's dropdown): a `fill.field` control (md, `r.md`)
/// showing the current label with an up/down chevron; it opens a
/// [showGlassMenu] with a check mark on the current value (LIQUID_GLASS_SPEC
/// §4.4–§4.5). A fill, never glass, so it also works inside dialogs.
class GlassSelect<T> extends StatelessWidget {
  const GlassSelect({
    required this.value,
    required this.items,
    required this.onChanged,
    super.key,
    this.semanticLabel,
    this.expand = false,
  });

  final T value;
  final List<GlassSelectItem<T>> items;

  /// `null` disables the control.
  final ValueChanged<T>? onChanged;
  final String? semanticLabel;

  /// Stretch to the available width.
  final bool expand;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final current = items.where((i) => i.value == value).firstOrNull;
    final shape = GlassRadii.shape(tokens.radii.md);
    final change = onChanged;
    return Builder(
      builder: (context) => GlassInteractive(
        semanticLabel: semanticLabel == null ? current?.label : '$semanticLabel: ${current?.label ?? ''}',
        onPressed: change == null
            ? null
            : () {
                final anchor = glassAnchorRect(context);
                unawaited(
                  showGlassMenu<T>(
                    context: context,
                    anchor: anchor,
                    minWidth: math.max(anchor.width, 160),
                    entries: [
                      for (final item in items)
                        GlassMenuItem<T>(
                          key: item.key,
                          value: item.value,
                          label: item.label,
                          icon: item.icon,
                          checked: item.value == value,
                        ),
                    ],
                  ).then((picked) {
                    if (picked != null && picked != value) change(picked);
                  }),
                );
              },
        builder: (context, state) {
          final fg = tokens.palette.label.withValues(alpha: state.enabled ? 1 : 0.38);
          final bg = state.pressed
              ? tokens.surfaces.fillPressed
              : state.hovered
              ? Color.alphaBlend(tokens.surfaces.fillHover, tokens.surfaces.fillField)
              : tokens.surfaces.fillField;
          return GlassFocusRing(
            visible: state.focusVisible,
            shape: shape,
            child: DecoratedBox(
              decoration: ShapeDecoration(color: bg, shape: shape),
              child: SizedBox(
                height: GlassControlSize.md.height,
                child: Padding(
                  padding: const EdgeInsetsDirectional.only(start: 10, end: 6),
                  child: Row(
                    mainAxisSize: expand ? MainAxisSize.max : MainAxisSize.min,
                    children: [
                      if (current?.icon != null) ...[
                        Icon(current!.icon, size: 16, color: fg),
                        const SizedBox(width: GlassSpacing.s6),
                      ],
                      Flexible(
                        fit: expand ? FlexFit.tight : FlexFit.loose,
                        child: Text(
                          current?.label ?? '',
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: tokens.typography.body.copyWith(color: fg),
                        ),
                      ),
                      const SizedBox(width: GlassSpacing.s6),
                      Icon(Icons.unfold_more_rounded, size: 16, color: tokens.secondaryLabel),
                    ],
                  ),
                ),
              ),
            ),
          );
        },
      ),
    );
  }
}
