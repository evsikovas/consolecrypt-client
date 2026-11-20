import 'package:material_ui/material_ui.dart';

/// The same bounded content can be hosted in the app workspace or a secondary dialog.
class AutomationWorkspacePanel extends StatelessWidget {
  const AutomationWorkspacePanel({
    super.key,
    required this.title,
    required this.content,
    required this.actions,
    this.embedded = false,
  });
  final Widget title, content;
  final List<Widget> actions;
  final bool embedded;
  @override
  Widget build(BuildContext context) {
    if (!embedded) {
      return AlertDialog(
        title: title,
        content: SizedBox(width: 760, height: MediaQuery.sizeOf(context).height * .65, child: content),
        actions: actions,
      );
    }
    return Padding(
      padding: const EdgeInsets.all(16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          DefaultTextStyle(style: Theme.of(context).textTheme.titleLarge!, child: title),
          const SizedBox(height: 12),
          Expanded(child: content),
          const SizedBox(height: 12),
          Wrap(spacing: 8, runSpacing: 8, alignment: WrapAlignment.end, children: actions),
        ],
      ),
    );
  }
}
