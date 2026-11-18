import 'package:consolecrypt/core/glass/glass.dart';
import 'package:material_ui/material_ui.dart';

class AutomationTechnicalBlock extends StatelessWidget {
  const AutomationTechnicalBlock({super.key, required this.child, this.padding = EdgeInsets.zero});

  final Widget child;
  final EdgeInsets padding;

  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.only(bottom: 6),
    child: ContentSurface(kind: ContentSurfaceKind.inset, padding: padding, child: child),
  );
}
