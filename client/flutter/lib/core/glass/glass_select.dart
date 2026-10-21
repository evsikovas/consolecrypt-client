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
  const GlassSelectItem({required this.value, required this.label, this.icon, this.subtitle, this.key});

  final T value;
  final String label;
  final IconData? icon;

  /// Second line in the menu only (e.g. size and requirements).
  final String? subtitle;

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
                  showGlassMenu<_Picked<T>>(
                    context: context,
                    anchor: anchor,
                    minWidth: math.max(anchor.width, 160),
                    entries: [
                      for (final item in items)
                        GlassMenuItem<_Picked<T>>(
                          key: item.key,
                          value: _Picked(item.value),
                          label: item.label,
                          icon: item.icon,
                          subtitle: item.subtitle,
                          checked: item.value == value,
                        ),
                    ],
                  ).then((picked) {
                    if (picked != null && picked.value != value) change(picked.value);
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

/// Form-field variant of [GlassSelect]: the same fill, label and error line
/// as the app's text fields, opening the glass menu instead of a Material
/// dropdown (so it follows the appearance settings and the compact type).
class GlassDropdownField<T> extends FormField<T> {
  GlassDropdownField({
    required List<GlassSelectItem<T>> items,
    required ValueChanged<T>? onChanged,
    T? value,
    String? label,
    String? hint,
    String? helper,
    IconData? prefixIcon,
    super.validator,
    super.autovalidateMode,
    super.key,
  }) : super(
         initialValue: value,
         enabled: onChanged != null,
         builder: (field) {
           final state = field as _GlassDropdownFieldState<T>;
           final tokens = GlassTokens.of(state.context);
           final current = items.where((i) => i.value == field.value).firstOrNull;
           final enabled = onChanged != null;
           final fg = tokens.palette.label.withValues(alpha: enabled ? 1 : 0.38);
           return Builder(
             builder: (context) => GlassInteractive(
               semanticLabel: label == null ? current?.label : '$label: ${current?.label ?? ''}',
               minHitSize: 0,
               onPressed: !enabled
                   ? null
                   : () {
                       final anchor = glassAnchorRect(context);
                       unawaited(
                         showGlassMenu<_Picked<T>>(
                           context: context,
                           anchor: anchor,
                           minWidth: math.max(anchor.width, 160),
                           entries: [
                             for (final item in items)
                               GlassMenuItem<_Picked<T>>(
                                 key: item.key,
                                 value: _Picked(item.value),
                                 label: item.label,
                                 icon: item.icon,
                                 subtitle: item.subtitle,
                                 checked: item.value == field.value,
                               ),
                           ],
                         ).then((picked) {
                           if (picked == null || picked.value == field.value || !state.mounted) return;
                           field.didChange(picked.value);
                           onChanged(picked.value);
                         }),
                       );
                     },
               builder: (context, interaction) => InputDecorator(
                 isFocused: interaction.focusVisible,
                 isHovering: interaction.hovered,
                 isEmpty: current == null,
                 decoration: InputDecoration(
                   labelText: label,
                   hintText: hint,
                   helperText: helper,
                   errorText: field.errorText,
                   enabled: enabled,
                   prefixIcon: prefixIcon == null ? null : Icon(prefixIcon, size: 18),
                   suffixIcon: Icon(Icons.unfold_more_rounded, size: 16, color: tokens.secondaryLabel),
                   suffixIconConstraints: const BoxConstraints(minWidth: 30, minHeight: 20),
                 ),
                 child: current == null
                     ? null
                     : Row(
                         children: [
                           if (current.icon != null) ...[
                             Icon(current.icon, size: 16, color: fg),
                             const SizedBox(width: GlassSpacing.s6),
                           ],
                           Expanded(
                             child: Text(
                               current.label,
                               maxLines: 1,
                               overflow: TextOverflow.ellipsis,
                               style: tokens.typography.body.copyWith(color: fg),
                             ),
                           ),
                         ],
                       ),
               ),
             ),
           );
         },
       );

  @override
  FormFieldState<T> createState() => _GlassDropdownFieldState<T>();
}

class _GlassDropdownFieldState<T> extends FormFieldState<T> {
  @override
  void didUpdateWidget(GlassDropdownField<T> oldWidget) {
    super.didUpdateWidget(oldWidget);
    // Parent-driven value (like DropdownButtonFormField's `initialValue`
    // being replaced by a rebuilt `value`).
    if (widget.initialValue != oldWidget.initialValue && widget.initialValue != value) {
      setValue(widget.initialValue);
    }
  }
}

/// Menu result wrapper, so a `null` item stays distinguishable from a
/// dismissed menu.
final class _Picked<T> {
  const _Picked(this.value);
  final T value;
}
