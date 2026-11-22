import 'package:consolecrypt/core/glass/glass.dart';
import 'package:material_ui/material_ui.dart';

/// Compact, divided sections for inventory and operational workspaces.
class WorkspaceSection extends StatelessWidget {
  const WorkspaceSection({super.key, required this.title, required this.child, this.trailing});
  final String title;
  final Widget child;
  final Widget? trailing;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    return ContentSurface(
      padding: EdgeInsets.zero,
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
            child: Row(
              children: [
                Expanded(
                  child: Semantics(header: true, child: Text(title, style: tokens.typography.bodyEmph)),
                ),
                ?trailing,
              ],
            ),
          ),
          Divider(height: 1, color: tokens.surfaces.separator),
          child,
        ],
      ),
    );
  }
}

/// Aligns the same columns in headers and data rows.
class WorkspaceColumns extends StatelessWidget {
  const WorkspaceColumns({super.key, required this.cells, this.flex, this.actions, this.actionWidth = 0});
  final List<Widget> cells;
  final List<int>? flex;
  final double actionWidth;
  final Widget? actions;
  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 5),
    child: Row(
      children: [
        for (final (index, cell) in cells.indexed)
          Expanded(
            flex: flex?[index] ?? 1,
            child: Padding(padding: const EdgeInsetsDirectional.only(end: 8), child: cell),
          ),
        if (actionWidth > 0) SizedBox(width: actionWidth, child: actions),
      ],
    ),
  );
}

class WorkspaceColumnsLayout extends StatelessWidget {
  const WorkspaceColumnsLayout({super.key, required this.main, required this.side});
  final Widget main, side;
  @override
  Widget build(BuildContext context) => LayoutBuilder(
    builder: (context, bounds) {
      final wide = bounds.maxWidth >= 920 * MediaQuery.textScalerOf(context).scale(12) / 12;
      return wide
          ? Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              spacing: 14,
              children: [
                Expanded(flex: 7, child: main),
                Expanded(flex: 3, child: side),
              ],
            )
          : Column(crossAxisAlignment: CrossAxisAlignment.stretch, spacing: 12, children: [main, side]);
    },
  );
}
