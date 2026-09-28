import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/sftp_browser_service.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:material_ui/material_ui.dart';

/// Result of the first-use hint.
final class EditHintResult {
  const EditHintResult({required this.dontShowAgain});

  final bool dontShowAgain;
}

/// First use of "edit in external app": explains the local working copy
/// and that the other app is outside ConsoleCrypt's control
/// (SFTP_BROWSER_SPEC §2 security notes). `null` = cancelled.
Future<EditHintResult?> showEditHintDialog(BuildContext context, String fileName) => showDialog<EditHintResult>(
  context: context,
  builder: (_) => _EditHintDialog(fileName: fileName),
);

class _EditHintDialog extends StatefulWidget {
  const _EditHintDialog({required this.fileName});

  final String fileName;

  @override
  State<_EditHintDialog> createState() => _EditHintDialogState();
}

class _EditHintDialogState extends State<_EditHintDialog> {
  bool _dontShow = true;

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final theme = Theme.of(context);
    return AlertDialog(
      key: const ValueKey('sftp-edit-hint'),
      icon: const Icon(Icons.open_in_new),
      title: Text(l10n.sftpEditHintTitle),
      content: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 480),
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(l10n.sftpEditHintBody(widget.fileName)),
              const SizedBox(height: 12),
              Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Icon(Icons.shield_outlined, size: 18, color: theme.colorScheme.tertiary),
                  const SizedBox(width: 8),
                  Expanded(child: Text(l10n.sftpEditHintControl, style: theme.textTheme.bodySmall)),
                ],
              ),
              const SizedBox(height: 8),
              CheckboxListTile(
                key: const ValueKey('sftp-edit-hint-dont-show'),
                contentPadding: EdgeInsets.zero,
                controlAffinity: ListTileControlAffinity.leading,
                dense: true,
                value: _dontShow,
                onChanged: (v) => setState(() => _dontShow = v ?? false),
                title: Text(l10n.sftpEditHintDontShow),
              ),
            ],
          ),
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.of(context).pop(), child: Text(l10n.commonCancel)),
        FilledButton(
          key: const ValueKey('sftp-edit-hint-open'),
          onPressed: () => Navigator.of(context).pop(EditHintResult(dontShowAgain: _dontShow)),
          child: Text(l10n.commonOpen),
        ),
      ],
    );
  }
}

/// Conflict resolution for [session] (status Conflict). `null` = decide later.
Future<EditConflictResolution?> showEditConflictDialog(BuildContext context, EditSessionInfo session) =>
    showDialog<EditConflictResolution>(
      context: context,
      builder: (_) => _EditConflictDialog(session: session),
    );

class _EditConflictDialog extends StatelessWidget {
  const _EditConflictDialog({required this.session});

  final EditSessionInfo session;

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final theme = Theme.of(context);
    final status = session.status;
    final remote = status is EditStatusConflict ? status.remote : null;
    final (stem, _) = _split(session.fileName);

    Widget option(EditConflictResolution value, IconData icon, String title, String body, {bool danger = false}) {
      final color = danger ? theme.colorScheme.error : theme.colorScheme.primary;
      return Padding(
        padding: const EdgeInsets.only(top: 8),
        child: OutlinedButton(
          key: ValueKey('sftp-conflict-${value.wireName}'),
          style: OutlinedButton.styleFrom(
            alignment: AlignmentDirectional.centerStart,
            padding: const EdgeInsets.all(12),
            side: BorderSide(color: color.withValues(alpha: 0.5)),
          ),
          onPressed: () => Navigator.of(context).pop(value),
          child: Row(
            children: [
              Icon(icon, color: color),
              const SizedBox(width: 12),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(title, style: theme.textTheme.titleSmall?.copyWith(color: color)),
                    const SizedBox(height: 2),
                    Text(body, style: theme.textTheme.bodySmall),
                  ],
                ),
              ),
            ],
          ),
        ),
      );
    }

    return AlertDialog(
      key: const ValueKey('sftp-conflict-dialog'),
      icon: Icon(Icons.call_split, color: theme.colorScheme.error),
      title: Text(l10n.sftpConflictTitle(session.fileName)),
      content: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 520),
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(remote == null ? l10n.sftpConflictDeletedMessage : l10n.sftpConflictMessage),
              if (remote != null && remote.modifiedAt != null) ...[
                const SizedBox(height: 6),
                Text(
                  l10n.sftpConflictRemoteMeta(formatBytes(l10n, remote.size), formatDateTime(l10n, remote.modifiedAt!)),
                  style: theme.textTheme.bodySmall?.copyWith(color: theme.colorScheme.onSurfaceVariant),
                ),
              ],
              const SizedBox(height: 4),
              option(
                EditConflictResolution.overwriteRemote,
                Icons.cloud_upload_outlined,
                remote == null ? l10n.sftpConflictRecreateTitle : l10n.sftpConflictOverwriteTitle,
                remote == null ? l10n.sftpConflictRecreateBody : l10n.sftpConflictOverwriteBody,
                danger: remote != null,
              ),
              if (remote != null)
                option(
                  EditConflictResolution.keepRemoteCopyLocally,
                  Icons.difference_outlined,
                  l10n.sftpConflictKeepTitle,
                  l10n.sftpConflictKeepBody(stem),
                ),
              if (remote != null)
                option(
                  EditConflictResolution.discardLocal,
                  Icons.restore,
                  l10n.sftpConflictDiscardTitle,
                  l10n.sftpConflictDiscardBody,
                  danger: true,
                ),
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
          key: const ValueKey('sftp-conflict-later'),
          onPressed: () => Navigator.of(context).pop(),
          child: Text(l10n.sftpConflictLater),
        ),
      ],
    );
  }
}

(String, String) _split(String name) {
  final dot = name.lastIndexOf('.');
  return dot <= 0 ? (name, '') : (name.substring(0, dot), name.substring(dot));
}

/// What to do with the leftovers of the last run.
final class LeftoverDecision {
  const LeftoverDecision({required this.recover, required this.discard});

  final Set<EditSessionId> recover;
  final Set<EditSessionId> discard;
}

/// "Recover unsaved edits?" — `null` = decide later (kept for next start).
Future<LeftoverDecision?> showEditLeftoversDialog(
  BuildContext context, {
  required List<EditLeftover> leftovers,
  required Map<ObjectId, Host> hosts,
}) => showDialog<LeftoverDecision>(
  context: context,
  barrierDismissible: false,
  builder: (_) => _LeftoversDialog(leftovers: leftovers, hosts: hosts),
);

class _LeftoversDialog extends StatefulWidget {
  const _LeftoversDialog({required this.leftovers, required this.hosts});

  final List<EditLeftover> leftovers;
  final Map<ObjectId, Host> hosts;

  @override
  State<_LeftoversDialog> createState() => _LeftoversDialogState();
}

class _LeftoversDialogState extends State<_LeftoversDialog> {
  late final Set<EditSessionId> _selected = {
    for (final l in widget.leftovers)
      if (_resumable(l) && (l.locallyModified ?? false)) l.id,
  };

  bool _resumable(EditLeftover l) => l.canResume && widget.hosts.containsKey(l.hostId);

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final theme = Theme.of(context);
    final all = {for (final l in widget.leftovers) l.id};
    return AlertDialog(
      key: const ValueKey('sftp-leftovers-dialog'),
      icon: const Icon(Icons.restore_page_outlined),
      title: Text(l10n.sftpLeftoversTitle),
      content: SizedBox(
        width: 520,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(l10n.sftpLeftoversMessage),
            const SizedBox(height: 8),
            Flexible(
              child: ListView(
                shrinkWrap: true,
                children: [
                  for (final l in widget.leftovers)
                    CheckboxListTile(
                      key: ValueKey('sftp-leftover-${l.fileName ?? l.id.value}'),
                      contentPadding: EdgeInsets.zero,
                      controlAffinity: ListTileControlAffinity.leading,
                      dense: true,
                      value: _selected.contains(l.id),
                      onChanged: _resumable(l)
                          ? (v) => setState(() => v == true ? _selected.add(l.id) : _selected.remove(l.id))
                          : null,
                      title: Text(l.fileName ?? l10n.commonUnknown, overflow: TextOverflow.ellipsis),
                      subtitle: Text(_subtitle(l10n, l), maxLines: 2, overflow: TextOverflow.ellipsis),
                      secondary: (l.locallyModified ?? false)
                          ? Tooltip(
                              message: l10n.sftpLeftoverModified,
                              child: Icon(Icons.edit_note, color: theme.colorScheme.tertiary),
                            )
                          : null,
                    ),
                ],
              ),
            ),
            const SizedBox(height: 8),
            Text(l10n.sftpLeftoversRecoverHint, style: theme.textTheme.bodySmall),
          ],
        ),
      ),
      actions: [
        TextButton(
          key: const ValueKey('sftp-leftovers-later'),
          onPressed: () => Navigator.of(context).pop(),
          child: Text(l10n.sftpLeftoversLater),
        ),
        TextButton(
          key: const ValueKey('sftp-leftovers-discard'),
          style: TextButton.styleFrom(foregroundColor: theme.colorScheme.error),
          onPressed: () => Navigator.of(context).pop(LeftoverDecision(recover: const {}, discard: all)),
          child: Text(l10n.sftpLeftoversDiscardAll),
        ),
        FilledButton(
          key: const ValueKey('sftp-leftovers-recover'),
          onPressed: _selected.isEmpty
              ? null
              : () =>
                    Navigator.of(context)
                        .pop(LeftoverDecision(recover: {..._selected}, discard: all.difference(_selected))),
          child: Text(l10n.sftpLeftoversRecover),
        ),
      ],
    );
  }

  String _subtitle(AppLocalizations l10n, EditLeftover l) {
    if (!l.canResume) return l10n.sftpLeftoverUnreadable;
    final host = widget.hosts[l.hostId];
    if (host == null) return l10n.sftpLeftoverUnknownHost;
    final state = (l.locallyModified ?? false) ? l10n.sftpLeftoverModified : l10n.sftpLeftoverUnchanged;
    return '${host.name}:${l.remotePath} · $state';
  }
}
