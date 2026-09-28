import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/sftp/sftp_controller.dart';
import 'package:consolecrypt/sftp/sftp_providers.dart';
import 'package:consolecrypt/sftp/sftp_rows.dart';
import 'package:consolecrypt/sftp/widgets/sftp_activity.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Status bar (chrome): "N folders, M files, size" of the current view or a
/// selection summary; active edit sessions; lock + protocol.
class SftpStatusBar extends ConsumerWidget {
  const SftpStatusBar({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    final theme = Theme.of(context);
    final state = ref.watch(sftpControllerProvider);
    final rows = ref.watch(sftpRowsProvider);
    final edits = ref.watch(editSessionsProvider).value ?? const <EditSessionInfo>[];
    final attention = edits.any((e) => e.status.needsAttention);
    final style = theme.textTheme.labelMedium?.copyWith(color: theme.colorScheme.onSurfaceVariant);

    final view = SftpCounts.of(rows.where((r) => r.depth == 0).map((r) => r.entry));
    final String summary;
    if (state.selection.isNotEmpty) {
      final selected = SftpCounts.of(state.selectedEntries);
      summary = l10n.sftpStatusSelection(
        formatCount(l10n, state.selection.length),
        formatCount(l10n, rows.length),
        formatBytes(l10n, selected.bytes),
      );
    } else {
      summary = l10n.sftpStatusSummary(
        l10n.sftpStatusFolders(view.folders),
        l10n.sftpStatusFiles(view.files),
        formatBytes(l10n, view.bytes),
      );
    }

    return SizedBox(
      height: 28,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 12),
        child: Row(
          children: [
            Expanded(
              child: Text(
                summary,
                key: const ValueKey('sftp-status-summary'),
                style: style,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
              ),
            ),
            if (edits.isNotEmpty)
              InkWell(
                key: const ValueKey('sftp-status-edits'),
                borderRadius: BorderRadius.circular(6),
                onTap: () => ref.read(sftpActivityProvider.notifier).show(SftpActivityTab.editing),
                child: Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
                  child: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Icon(
                        attention ? Icons.error_outline : Icons.edit_note,
                        size: 15,
                        color: attention ? theme.colorScheme.error : theme.colorScheme.primary,
                      ),
                      const SizedBox(width: 4),
                      Text(
                        l10n.sftpStatusEditing(edits.length),
                        style: style?.copyWith(color: attention ? theme.colorScheme.error : null),
                      ),
                    ],
                  ),
                ),
              ),
            const SizedBox(width: 12),
            Tooltip(
              message: l10n.sftpStatusSecure,
              child: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Icon(Icons.lock, size: 13, color: theme.colorScheme.primary),
                  const SizedBox(width: 4),
                  Text('SFTP', key: const ValueKey('sftp-status-protocol'), style: style),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}
