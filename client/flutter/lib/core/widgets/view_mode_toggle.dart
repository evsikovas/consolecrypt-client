import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:material_ui/material_ui.dart';

/// Cards grid / compact list switch shown next to a collection search field
/// (hosts, databases). Buttons are keyed `<keyPrefix>-cards` / `<keyPrefix>-list`.
class ViewModeToggle extends StatelessWidget {
  const ViewModeToggle({required this.cards, required this.onChanged, required this.keyPrefix, super.key});

  final bool cards;
  final ValueChanged<bool> onChanged;
  final String keyPrefix;

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        GlassIconButton(
          key: ValueKey('$keyPrefix-cards'),
          tooltip: l.inventoryCards,
          selected: cards,
          style: GlassIconButtonStyle.plain,
          icon: Icons.grid_view_rounded,
          onPressed: () => onChanged(true),
        ),
        GlassIconButton(
          key: ValueKey('$keyPrefix-list'),
          tooltip: l.inventoryList,
          selected: !cards,
          style: GlassIconButtonStyle.plain,
          icon: Icons.view_list_rounded,
          onPressed: () => onChanged(false),
        ),
      ],
    );
  }
}
