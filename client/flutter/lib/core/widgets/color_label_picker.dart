import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/settings/color_picker.dart';
import 'package:material_ui/material_ui.dart';

/// Label colours offered next to "no colour" (clusters, databases…).
const kLabelColors = ['#4285F4', '#22A06B', '#E56910', '#AE2E24', '#8270DB', '#1D9BB2']; // l10n-ignore: colours

/// `#RRGGBB` → colour (null for none or an invalid value).
Color? labelColor(String? value) {
  if (value == null) return null;
  final rgb = int.tryParse(value.replaceFirst('#', ''), radix: 16);
  return rgb == null || value.length != 7 ? null : Color(0xFF000000 | rgb);
}

String _hex(int rgb) => '#${(rgb & 0xFFFFFF).toRadixString(16).padLeft(6, '0').toUpperCase()}';

/// A row of colour swatches: none, the label palette and a custom colour.
class ColorLabelPicker extends StatelessWidget {
  const ColorLabelPicker({required this.value, required this.onChanged, this.enabled = true, super.key});
  final String? value;
  final ValueChanged<String?> onChanged;
  final bool enabled;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final l = context.l10n;
    final custom = value != null && !kLabelColors.contains(value!.toUpperCase()) ? value : null;
    Widget swatch(String? color, {IconData? icon, String? tooltip, VoidCallback? onPressed}) {
      final selected = icon == Icons.palette_outlined ? custom != null : value?.toUpperCase() == color?.toUpperCase();
      return Padding(
        padding: const EdgeInsets.only(right: 6),
        child: Tooltip(
          message: tooltip ?? color ?? l.kubeNoColor,
          child: GlassInteractive(
            key: ValueKey('color-label-${icon == Icons.palette_outlined ? 'custom' : color ?? 'none'}'),
            semanticLabel: tooltip ?? color ?? l.kubeNoColor,
            selected: selected,
            onPressed: enabled ? (onPressed ?? () => onChanged(color)) : null,
            builder: (context, state) => Container(
              width: 28,
              height: 28,
              decoration: BoxDecoration(
                shape: BoxShape.circle,
                color: labelColor(color) ?? Colors.transparent,
                border: Border.all(
                  color: selected || state.hovered ? tokens.palette.label : tokens.surfaces.separator,
                  width: selected ? 2.5 : 1,
                ),
              ),
              child: icon == null
                  ? null
                  : Icon(icon, size: 15, color: labelColor(color) == null ? tokens.secondaryLabel : Colors.white),
            ),
          ),
        ),
      );
    }

    return Wrap(
      crossAxisAlignment: WrapCrossAlignment.center,
      runSpacing: 6,
      children: [
        swatch(null, icon: Icons.block_rounded),
        for (final color in kLabelColors) swatch(color),
        swatch(
          custom,
          icon: Icons.palette_outlined,
          tooltip: l.colorPickerCustom,
          onPressed: () async {
            final picked = await pickRgbColor(
              context,
              title: l.colorPickerCustom,
              color: (labelColor(value) ?? const Color(0xFF4285F4)).toARGB32() & 0xFFFFFF,
            );
            if (picked != null) onChanged(_hex(picked));
          },
        ),
      ],
    );
  }
}
