import 'dart:async';

import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/sftp/file_kind.dart';
import 'package:consolecrypt/sftp/sftp_actions.dart';
import 'package:consolecrypt/sftp/sftp_controller.dart';
import 'package:consolecrypt/sftp/sftp_rows.dart';
import 'package:consolecrypt/sftp/widgets/sftp_activity.dart';
import 'package:consolecrypt/sftp/widgets/sftp_drag.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Optional local pane (side-by-side transfers): folders of this device;
/// double-click a file or drag it onto the remote list to upload, drop
/// remote rows here to download.
class SftpLocalPane extends ConsumerWidget {
  const SftpLocalPane({super.key});

  Future<void> _download(BuildContext context, WidgetRef ref, SftpDragData data, String directory) async {
    final l10n = context.l10n;
    final ids = await runWithFeedback(
      context,
      () => ref.read(sftpControllerProvider.notifier).download(data.paths, directory),
    );
    if (ids != null && context.mounted) {
      ref.read(sftpActivityProvider.notifier).show(SftpActivityTab.transfers);
      showSnack(context, l10n.sftpDownloadStarted(ids.length, directory));
    }
  }

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    final theme = Theme.of(context);
    final state = ref.watch(sftpControllerProvider);
    final controller = ref.read(sftpControllerProvider.notifier);
    final entries = [...state.local]
      ..sort((a, b) {
        if (a.isDirectory != b.isDirectory) return a.isDirectory ? -1 : 1;
        return compareNatural(a.name, b.name);
      });
    final visible = [
      for (final e in entries)
        if (state.prefs.showHidden || !e.name.startsWith('.')) e,
    ];
    return DragTarget<SftpDragData>(
      onWillAcceptWithDetails: (_) => true,
      onAcceptWithDetails: (d) => unawaited(_download(context, ref, d.data, state.localPath)),
      builder: (context, candidates, _) => DecoratedBox(
        key: const ValueKey('sftp-local-pane'),
        position: DecorationPosition.foreground,
        decoration: BoxDecoration(
          border: candidates.isNotEmpty ? Border.all(color: theme.colorScheme.primary, width: 2) : null,
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            SizedBox(
              height: 38,
              child: Padding(
                padding: const EdgeInsets.fromLTRB(12, 2, 4, 2),
                child: Row(
                  children: [
                    Icon(Icons.laptop_mac, size: 16, color: theme.colorScheme.onSurfaceVariant),
                    const SizedBox(width: 6),
                    Expanded(
                      child: Tooltip(
                        message: state.localPath,
                        child: Text(
                          '${l10n.sftpLocalPaneTitle} · ${state.localPath}',
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: theme.textTheme.bodySmall,
                        ),
                      ),
                    ),
                    IconButton(
                      key: const ValueKey('sftp-local-up'),
                      tooltip: l10n.sftpUp,
                      icon: const Icon(Icons.arrow_upward, size: 18),
                      onPressed: state.localPath == parentLocalPath(state.localPath)
                          ? null
                          : () => unawaited(controller.openLocal(parentLocalPath(state.localPath))),
                    ),
                    IconButton(
                      key: const ValueKey('sftp-upload'),
                      tooltip: l10n.sftpUploadTooltip,
                      icon: const Icon(Icons.arrow_forward, size: 18),
                      onPressed: state.localSelection.isEmpty
                          ? null
                          : () => unawaited(uploadPaths(context, ref, state.localSelection.toList(), state.remotePath)),
                    ),
                  ],
                ),
              ),
            ),
            const Divider(height: 1),
            Expanded(
              child: ListView.builder(
                itemExtent: 26,
                itemCount: visible.length,
                itemBuilder: (context, i) {
                  final e = visible[i];
                  final selected = state.localSelection.contains(e.path);
                  final kind = e.isDirectory ? const FileKind(FileKindGroup.folder) : fileKindForName(e.name);
                  final fg = selected ? theme.colorScheme.onPrimary : theme.colorScheme.onSurface;
                  final row = GestureDetector(
                    behavior: HitTestBehavior.opaque,
                    onTap: () => controller.selectLocal(
                      e.path,
                      toggle: HardwareKeyboard.instance.isMetaPressed || HardwareKeyboard.instance.isControlPressed,
                    ),
                    onDoubleTap: () => e.isDirectory
                        ? unawaited(controller.openLocal(e.path))
                        : unawaited(uploadPaths(context, ref, [e.path], state.remotePath)),
                    child: ColoredBox(
                      color: selected
                          ? theme.colorScheme.primary
                          : (i.isOdd ? theme.colorScheme.onSurface.withValues(alpha: 0.025) : Colors.transparent),
                      child: Padding(
                        padding: const EdgeInsets.symmetric(horizontal: 12),
                        child: Row(
                          children: [
                            Icon(kind.icon, size: 16, color: selected ? fg : kind.color(theme.colorScheme)),
                            const SizedBox(width: 6),
                            Expanded(
                              child: Text(
                                e.name,
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style: theme.textTheme.bodyMedium?.copyWith(fontSize: 13, color: fg),
                              ),
                            ),
                            SizedBox(
                              width: 72,
                              child: Text(
                                e.isDirectory ? '--' : formatBytes(l10n, e.size),
                                textAlign: TextAlign.end,
                                style: theme.textTheme.bodySmall?.copyWith(fontSize: 12, color: fg),
                              ),
                            ),
                          ],
                        ),
                      ),
                    ),
                  );
                  final dragPaths = selected ? state.localSelection.toList() : [e.path];
                  final draggable = Draggable<LocalDragData>(
                    key: ValueKey('local-row-${e.name}'),
                    data: LocalDragData(dragPaths),
                    dragAnchorStrategy: pointerDragAnchorStrategy,
                    feedback: DragCountChip(count: dragPaths.length, icon: kind.icon),
                    child: row,
                  );
                  if (!e.isDirectory) return draggable;
                  return DragTarget<SftpDragData>(
                    onWillAcceptWithDetails: (_) => true,
                    onAcceptWithDetails: (d) => unawaited(_download(context, ref, d.data, e.path)),
                    builder: (context, candidates, _) => candidates.isEmpty
                        ? draggable
                        : DecoratedBox(
                            position: DecorationPosition.foreground,
                            decoration: BoxDecoration(color: theme.colorScheme.primary.withValues(alpha: 0.22)),
                            child: draggable,
                          ),
                  );
                },
              ),
            ),
          ],
        ),
      ),
    );
  }
}
