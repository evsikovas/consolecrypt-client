import 'package:consolecrypt/core/glass/glass.dart';
import 'package:material_ui/material_ui.dart';

class EditorToolbarButton extends StatelessWidget {
  const EditorToolbarButton({
    required this.icon,
    required this.tooltip,
    required this.onPressed,
    this.selected = false,
    super.key,
  });

  final IconData icon;
  final String tooltip;
  final VoidCallback? onPressed;
  final bool selected;

  @override
  Widget build(BuildContext context) => GlassIconButton(
    icon: icon,
    tooltip: tooltip,
    onPressed: onPressed,
    selected: selected,
    size: 32,
    iconSize: 18,
    style: GlassIconButtonStyle.plain,
  );
}
