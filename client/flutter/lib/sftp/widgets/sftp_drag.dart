import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:material_ui/material_ui.dart';

/// Remote rows being dragged inside the app (move / download to the local pane).
final class SftpDragData {
  const SftpDragData(this.paths);

  final List<String> paths;
}

/// Local-pane rows being dragged onto the remote list (upload).
final class LocalDragData {
  const LocalDragData(this.paths);

  final List<String> paths;
}

/// Drag feedback: "3 items" chip following the pointer.
class DragCountChip extends StatelessWidget {
  const DragCountChip({required this.count, required this.icon, super.key});

  final int count;
  final IconData icon;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Material(
      elevation: 6,
      color: theme.colorScheme.primary,
      borderRadius: BorderRadius.circular(8),
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(icon, size: 16, color: theme.colorScheme.onPrimary),
            const SizedBox(width: 6),
            Text(
              context.l10n.sftpDragItems(count),
              style: theme.textTheme.labelMedium?.copyWith(color: theme.colorScheme.onPrimary),
            ),
          ],
        ),
      ),
    );
  }
}
