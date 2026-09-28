import 'dart:async';

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/sftp/edit_flows.dart';
import 'package:consolecrypt/sftp/file_kind.dart';
import 'package:consolecrypt/sftp/sftp_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

enum SftpActivityTab { transfers, editing }

final class SftpActivityState {
  const SftpActivityState({this.open = false, this.tab = SftpActivityTab.transfers});

  final bool open;
  final SftpActivityTab tab;
}

/// The collapsible bottom drawer with Transfers and Editing.
class SftpActivityController extends Notifier<SftpActivityState> {
  @override
  SftpActivityState build() => const SftpActivityState();

  void show(SftpActivityTab tab) => state = SftpActivityState(open: true, tab: tab);

  /// Toolbar buttons: open on [tab], or close if it is already showing.
  void toggle(SftpActivityTab tab) =>
      state = state.open && state.tab == tab ? SftpActivityState(tab: tab) : SftpActivityState(open: true, tab: tab);

  void close() => state = SftpActivityState(tab: state.tab);
}

final sftpActivityProvider = NotifierProvider<SftpActivityController, SftpActivityState>(SftpActivityController.new);

/// Bottom drawer: transfer progress (cancel / retry) and edit sessions
/// (status, reveal, reopen, sync, stop, conflict resolution).
class SftpActivityDrawer extends ConsumerWidget {
  const SftpActivityDrawer({super.key, this.height = 196});

  final double height;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final activity = ref.watch(sftpActivityProvider);
    if (!activity.open) return const SizedBox.shrink();
    final l10n = context.l10n;
    final transfers = ref.watch(transfersProvider).value ?? const <TransferJob>[];
    final edits = ref.watch(editSessionsProvider).value ?? const <EditSessionInfo>[];
    final controller = ref.read(sftpActivityProvider.notifier);
    return Column(
      key: const ValueKey('sftp-activity'),
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        const Divider(height: 1),
        Padding(
          padding: const EdgeInsets.fromLTRB(12, 6, 4, 2),
          child: Row(
            children: [
              Flexible(
                child: SingleChildScrollView(
                  scrollDirection: Axis.horizontal,
                  child: SegmentedButton<SftpActivityTab>(
                    showSelectedIcon: false,
                    style: const ButtonStyle(visualDensity: VisualDensity.compact),
                    segments: [
                      ButtonSegment(
                        value: SftpActivityTab.transfers,
                        icon: const Icon(Icons.swap_vert, size: 16),
                        label: Text(
                          '${l10n.sftpToolbarTransfers} (${transfers.length})',
                          key: const ValueKey('sftp-activity-tab-transfers'),
                        ),
                      ),
                      ButtonSegment(
                        value: SftpActivityTab.editing,
                        icon: const Icon(Icons.edit_note, size: 16),
                        label: Text(
                          '${l10n.sftpToolbarEditing} (${edits.length})',
                          key: const ValueKey('sftp-activity-tab-editing'),
                        ),
                      ),
                    ],
                    selected: {activity.tab},
                    onSelectionChanged: (s) => controller.show(s.first),
                  ),
                ),
              ),
              const Spacer(),
              if (activity.tab == SftpActivityTab.transfers && transfers.any((t) => !t.isActive))
                TextButton(
                  key: const ValueKey('sftp-transfers-clear'),
                  onPressed: () => unawaited(ref.read(sftpBrowserServiceProvider).clearFinishedTransfers()),
                  child: Text(l10n.sftpTransfersClear),
                ),
              IconButton(
                key: const ValueKey('sftp-activity-close'),
                tooltip: l10n.sftpActivityHide,
                icon: const Icon(Icons.expand_more, size: 20),
                onPressed: controller.close,
              ),
            ],
          ),
        ),
        SizedBox(
          height: height,
          child: switch (activity.tab) {
            SftpActivityTab.transfers => _TransfersList(transfers: transfers),
            SftpActivityTab.editing => _EditingList(sessions: edits),
          },
        ),
      ],
    );
  }
}

class _Empty extends StatelessWidget {
  const _Empty(this.message, this.icon);

  final String message;
  final IconData icon;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Center(
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 24),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(icon, color: theme.colorScheme.outline),
            const SizedBox(width: 10),
            Flexible(
              child: Text(
                message,
                style: theme.textTheme.bodySmall?.copyWith(color: theme.colorScheme.onSurfaceVariant),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _TransfersList extends ConsumerWidget {
  const _TransfersList({required this.transfers});

  final List<TransferJob> transfers;

  static String _status(AppLocalizations l10n, TransferJob t) => switch (t.state) {
    TransferState.queued => l10n.sftpTransferQueued,
    TransferState.completed => l10n.sftpTransferDone(formatBytes(l10n, t.totalBytes)),
    // t.error is the core's English diagnostic (detail only).
    TransferState.failed => l10n.sftpTransferFailed(t.error ?? ''),
    TransferState.cancelled => l10n.sftpTransferCancelled,
    TransferState.running => [
      l10n.sftpTransferProgress(formatBytes(l10n, t.transferredBytes), formatBytes(l10n, t.totalBytes)),
      if (t.bytesPerSecond > 0) l10n.sftpTransferSpeed(formatBytes(l10n, t.bytesPerSecond)),
    ].join(' · '),
  };

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    final theme = Theme.of(context);
    if (transfers.isEmpty) return _Empty(l10n.sftpTransfersEmpty, Icons.swap_vert);
    final debug = ref.watch(sftpDebugControlsProvider);
    return ListView.builder(
      padding: const EdgeInsets.symmetric(horizontal: 8),
      itemCount: transfers.length,
      itemBuilder: (context, i) {
        final t = transfers[i];
        final upload = t.direction == TransferDirection.upload;
        final failed = t.state == TransferState.failed;
        return SizedBox(
          key: ValueKey('transfer-${t.fileName}'),
          height: 48,
          child: Row(
            children: [
              Tooltip(
                message: upload ? l10n.sftpTransferDirectionUpload : l10n.sftpTransferDirectionDownload,
                child: Icon(upload ? Icons.upload : Icons.download, size: 18, color: theme.colorScheme.primary),
              ),
              const SizedBox(width: 10),
              Expanded(
                child: Column(
                  mainAxisAlignment: MainAxisAlignment.center,
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Row(
                      children: [
                        Flexible(
                          child: Text(t.fileName, overflow: TextOverflow.ellipsis, style: theme.textTheme.bodyMedium),
                        ),
                        const SizedBox(width: 6),
                        Flexible(
                          child: Text(
                            l10n.sftpTransferTo(_parent(t.destinationPath)),
                            overflow: TextOverflow.ellipsis,
                            style: theme.textTheme.bodySmall?.copyWith(color: theme.colorScheme.onSurfaceVariant),
                          ),
                        ),
                      ],
                    ),
                    const SizedBox(height: 3),
                    if (t.isActive) LinearProgressIndicator(value: t.state == TransferState.queued ? null : t.fraction),
                    const SizedBox(height: 2),
                    Text(
                      _status(l10n, t),
                      key: ValueKey('transfer-status-${t.fileName}'),
                      overflow: TextOverflow.ellipsis,
                      style: theme.textTheme.labelSmall?.copyWith(
                        color: failed ? theme.colorScheme.error : theme.colorScheme.onSurfaceVariant,
                      ),
                    ),
                  ],
                ),
              ),
              if (t.isActive)
                IconButton(
                  key: ValueKey('transfer-cancel-${t.fileName}'),
                  tooltip: l10n.commonCancel,
                  icon: const Icon(Icons.close, size: 18),
                  onPressed: () => unawaited(ref.read(sftpServiceProvider).cancelTransfer(t.id)),
                )
              else if (failed || t.state == TransferState.cancelled)
                IconButton(
                  key: ValueKey('transfer-retry-${t.fileName}'),
                  tooltip: l10n.commonRetry,
                  icon: const Icon(Icons.refresh, size: 18),
                  onPressed: () =>
                      runWithFeedback(context, () => ref.read(sftpBrowserServiceProvider).retryTransfer(t.id)),
                ),
              if (debug != null && i == 0)
                IconButton(
                  key: const ValueKey('sftp-debug-fail-transfer'),
                  tooltip: l10n.sftpDebugFailTransfer,
                  icon: const Icon(Icons.bug_report_outlined, size: 16),
                  onPressed: debug.debugFailNextTransfer,
                ),
            ],
          ),
        );
      },
    );
  }

  static String _parent(String path) {
    final i = path.lastIndexOf(RegExp(r'[\\/]'));
    return i <= 0 ? path.substring(0, i + 1) : path.substring(0, i);
  }
}

/// Label of "reveal in file manager" for the platform.
String revealLabel(AppLocalizations l10n) => AppPlatform.isMacOS
    ? l10n.sftpEditRevealFinder
    : (AppPlatform.isWindows ? l10n.sftpEditRevealExplorer : l10n.sftpEditRevealOther);

class _EditingList extends ConsumerWidget {
  const _EditingList({required this.sessions});

  final List<EditSessionInfo> sessions;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    if (sessions.isEmpty) return _Empty(l10n.sftpEditingEmpty, Icons.edit_note);
    final hosts = ref.watch(hostByIdProvider);
    final service = ref.read(sftpBrowserServiceProvider);
    final debug = ref.watch(sftpDebugControlsProvider);
    final theme = Theme.of(context);
    return ListView.builder(
      padding: const EdgeInsets.symmetric(horizontal: 8),
      itemCount: sessions.length,
      itemBuilder: (context, i) {
        final s = sessions[i];
        final status = s.status;
        final host = hosts[s.hostId]?.name ?? '';
        final kind = fileKindForName(s.fileName);
        return SizedBox(
          key: ValueKey('sftp-edit-session-${s.fileName}'),
          height: 48,
          child: Row(
            children: [
              Icon(kind.icon, size: 18, color: kind.color(theme.colorScheme)),
              const SizedBox(width: 10),
              Expanded(
                child: Column(
                  mainAxisAlignment: MainAxisAlignment.center,
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(s.fileName, overflow: TextOverflow.ellipsis, style: theme.textTheme.bodyMedium),
                    Text(
                      [
                        '$host:${s.remotePath}',
                        if (s.app != null) l10n.sftpEditOpenedWith(s.app!.displayName),
                        if (s.lastSyncedAt != null) l10n.sftpEditLastSynced(formatRelative(l10n, s.lastSyncedAt!)),
                      ].join(' · '),
                      overflow: TextOverflow.ellipsis,
                      style: theme.textTheme.labelSmall?.copyWith(color: theme.colorScheme.onSurfaceVariant),
                    ),
                  ],
                ),
              ),
              const SizedBox(width: 8),
              EditStatusChip(status: status, key: ValueKey('sftp-edit-status-${s.fileName}')),
              const SizedBox(width: 4),
              if (status is EditStatusConflict)
                TextButton(
                  key: ValueKey('sftp-edit-resolve-${s.fileName}'),
                  onPressed: () => resolveEditConflict(context, ref, s),
                  child: Text(l10n.sftpEditResolve),
                ),
              if (status is EditStatusError && status.retryable)
                TextButton(
                  key: ValueKey('sftp-edit-retry-${s.fileName}'),
                  onPressed: () => runWithFeedback(context, () => service.syncEditSession(s.id)),
                  child: Text(l10n.commonRetry),
                ),
              IconButton(
                key: ValueKey('sftp-edit-reveal-${s.fileName}'),
                tooltip: revealLabel(l10n),
                icon: const Icon(Icons.folder_open_outlined, size: 18),
                onPressed: () => runWithFeedback(context, () => service.revealEditSession(s.id)),
              ),
              IconButton(
                key: ValueKey('sftp-edit-reopen-${s.fileName}'),
                tooltip: l10n.sftpEditReopen,
                icon: const Icon(Icons.open_in_new, size: 18),
                onPressed: () => runWithFeedback(context, () => service.reopenEditSession(s.id)),
              ),
              IconButton(
                key: ValueKey('sftp-edit-sync-${s.fileName}'),
                tooltip: l10n.sftpEditSyncNow,
                icon: const Icon(Icons.sync, size: 18),
                onPressed: status is EditStatusUploading || status is EditStatusOpening
                    ? null
                    : () => runWithFeedback(context, () => service.syncEditSession(s.id)),
              ),
              IconButton(
                key: ValueKey('sftp-edit-stop-${s.fileName}'),
                tooltip: l10n.sftpEditStop,
                icon: const Icon(Icons.stop_circle_outlined, size: 18),
                onPressed: () => stopEditing(context, ref, s),
              ),
              if (debug != null)
                PopupMenuButton<int>(
                  key: ValueKey('sftp-edit-debug-${s.fileName}'),
                  tooltip: l10n.sftpDebugMenu,
                  icon: const Icon(Icons.bug_report_outlined, size: 16),
                  onSelected: (v) => switch (v) {
                    0 => debug.debugSimulateSave(s.id),
                    1 => debug.debugSimulateRemoteChange(s.id),
                    _ => debug.debugFailNextUpload(s.id),
                  },
                  itemBuilder: (_) => [
                    PopupMenuItem(key: const ValueKey('sftp-debug-save'), value: 0, child: Text(l10n.sftpDebugSave)),
                    PopupMenuItem(
                      key: const ValueKey('sftp-debug-remote-change'),
                      value: 1,
                      child: Text(l10n.sftpDebugRemoteChange),
                    ),
                    PopupMenuItem(
                      key: const ValueKey('sftp-debug-fail-upload'),
                      value: 2,
                      child: Text(l10n.sftpDebugFailUpload),
                    ),
                  ],
                ),
            ],
          ),
        );
      },
    );
  }
}

/// Status pill of an edit session (Synced, Uploading %, Modified, Conflict, Error…).
class EditStatusChip extends StatelessWidget {
  const EditStatusChip({required this.status, super.key});

  final EditStatus status;

  static const _green = Color(0xFF2E9E5B);

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final scheme = Theme.of(context).colorScheme;
    final (label, color, icon) = switch (status) {
      EditStatusOpening() => (l10n.sftpEditStatusOpening, scheme.outline, Icons.hourglass_empty),
      EditStatusSynced() => (l10n.sftpEditStatusSynced, _green, Icons.check_circle_outline),
      EditStatusModified() => (l10n.sftpEditStatusModified, scheme.tertiary, Icons.edit_outlined),
      EditStatusUploading(:final fraction) => (
        fraction == null ? l10n.sftpEditStatusUploadingUnknown : l10n.sftpEditStatusUploading((fraction * 100).round()),
        scheme.primary,
        Icons.cloud_upload_outlined,
      ),
      EditStatusConflict() => (l10n.sftpEditStatusConflict, scheme.error, Icons.call_split),
      EditStatusError() => (l10n.sftpEditStatusError, scheme.error, Icons.error_outline),
      EditStatusClosed() => (l10n.sftpEditStatusClosed, scheme.outline, Icons.stop_circle_outlined),
    };
    final detail = status is EditStatusError ? (status as EditStatusError).message : null;
    final chip = DecoratedBox(
      decoration: BoxDecoration(
        color: color.withValues(alpha: 0.12),
        borderRadius: BorderRadius.circular(10),
        border: Border.all(color: color.withValues(alpha: 0.4)),
      ),
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(icon, size: 13, color: color),
            const SizedBox(width: 4),
            Text(label, style: Theme.of(context).textTheme.labelSmall?.copyWith(color: color)),
          ],
        ),
      ),
    );
    return detail == null ? chip : Tooltip(message: detail, child: chip);
  }
}
