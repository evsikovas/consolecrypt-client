import 'package:consolecrypt/app/platform.dart';
import 'package:material_ui/material_ui.dart';

/// Desktop-only overlay thumbs; layout and the authoritative grid controllers
/// do not change when the pointer expands a thumb.
class DbResultScrollbars extends StatelessWidget {
  const DbResultScrollbars({
    required this.vertical,
    required this.horizontal,
    required this.records,
    required this.child,
    super.key,
  });
  final ScrollController vertical, horizontal;
  final bool records;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    if (AppPlatform.isMobile) return child;
    bool active(Set<WidgetState> states) =>
        states.contains(WidgetState.hovered) || states.contains(WidgetState.dragged);
    final color = Theme.of(context).colorScheme.onSurface;
    Widget content = child;
    if (!records) {
      content = Scrollbar(
        key: const ValueKey('db-result-scrollbar-horizontal'),
        controller: horizontal,
        thumbVisibility: true,
        interactive: true,
        scrollbarOrientation: ScrollbarOrientation.bottom,
        notificationPredicate: (notification) =>
            notification.metrics.axis == Axis.horizontal && notification.depth == 0,
        child: content,
      );
    }
    return ScrollConfiguration(
      behavior: ScrollConfiguration.of(context).copyWith(scrollbars: false),
      child: ScrollbarTheme(
        data: ScrollbarThemeData(
          thickness: WidgetStateProperty.resolveWith((states) => active(states) ? 10 : 3),
          thumbColor: WidgetStateProperty.resolveWith((states) => color.withValues(alpha: active(states) ? .65 : .30)),
          trackVisibility: WidgetStateProperty.resolveWith(active),
          trackColor: WidgetStatePropertyAll(color.withValues(alpha: .06)),
          trackBorderColor: const WidgetStatePropertyAll(Colors.transparent),
          radius: const Radius.circular(6),
          crossAxisMargin: 2,
          mainAxisMargin: 4,
          minThumbLength: 32,
        ),
        child: Scrollbar(
          key: const ValueKey('db-result-scrollbar-vertical'),
          controller: vertical,
          thumbVisibility: true,
          interactive: true,
          notificationPredicate: (notification) =>
              notification.metrics.axis == Axis.vertical && notification.depth == (records ? 0 : 1),
          child: content,
        ),
      ),
    );
  }
}
