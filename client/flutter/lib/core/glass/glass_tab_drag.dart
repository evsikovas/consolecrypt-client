import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/app/theme/glass_tokens.dart';
import 'package:material_ui/material_ui.dart';

/// Drag-to-reorder for one tab of a [GlassTabStrip] or a tab column: drag a
/// tab onto another to move it there. Desktop only (on phones a long press
/// opens the tab menu). The dragged tab fades in place; the tab under the
/// pointer shows an insertion edge.
class GlassTabDrag extends StatelessWidget {
  const GlassTabDrag({
    required this.index,
    required this.axis,
    required this.onReorder,
    required this.child,
    super.key,
  });

  final int index;
  final Axis axis;
  final void Function(int from, int to)? onReorder;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    final reorder = onReorder;
    if (reorder == null || AppPlatform.isMobile) return child;
    final accent = GlassTokens.of(context).palette.accent;
    return LayoutBuilder(builder: (context, constraints) => _target(reorder, accent, constraints));
  }

  Widget _target(void Function(int, int) reorder, Color accent, BoxConstraints constraints) {
    // The dragged copy lives in the overlay: a column row keeps its width.
    final width = constraints.hasBoundedWidth ? constraints.maxWidth : null;
    return DragTarget<int>(
      onWillAcceptWithDetails: (details) => details.data != index,
      onAcceptWithDetails: (details) => reorder(details.data, index),
      builder: (context, candidates, _) {
        final from = candidates.isEmpty ? null : candidates.first;
        // The edge the tab will land at: before this one when it comes from
        // after it, after this one otherwise.
        final before = from != null && from > index;
        final edge = BorderSide(color: accent, width: 2);
        return DecoratedBox(
          position: DecorationPosition.foreground,
          decoration: BoxDecoration(
            border: from == null
                ? null
                : axis == Axis.horizontal
                ? BorderDirectional(start: before ? edge : BorderSide.none, end: before ? BorderSide.none : edge)
                : Border(top: before ? edge : BorderSide.none, bottom: before ? BorderSide.none : edge),
          ),
          child: Draggable<int>(
            data: index,
            axis: axis,
            feedback: SizedBox(
              width: width,
              child: Opacity(
                opacity: .85,
                child: Material(type: MaterialType.transparency, child: child),
              ),
            ),
            childWhenDragging: Opacity(opacity: .35, child: child),
            child: child,
          ),
        );
      },
    );
  }
}
