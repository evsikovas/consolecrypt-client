import 'package:consolecrypt/app/tool_icons.dart';
import 'package:material_ui/material_ui.dart';

/// Ship's-wheel glyph used for Kubernetes in navigation. Vector-drawn so it
/// follows the surrounding [IconTheme] colour (sidebar selection, accent).
class KubeWheelIcon extends StatelessWidget {
  const KubeWheelIcon({super.key, this.size, this.color});

  final double? size;
  final Color? color;

  @override
  Widget build(BuildContext context) => Icon(ToolIcons.kubernetes, size: size, color: color);
}
