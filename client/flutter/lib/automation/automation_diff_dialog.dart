import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Every page and file is bound to the prepared revision. Errors never approve
/// the review, and late results cannot reappear after profile lock/change.
class AutomationDiffDialog extends ConsumerStatefulWidget {
  const AutomationDiffDialog({
    super.key,
    required this.scope,
    required this.service,
    required this.projectId,
    required this.digest,
  });
  final AutomationScope scope;
  final AutomationDiffService service;
  final String projectId, digest;
  @override
  ConsumerState<AutomationDiffDialog> createState() => _AutomationDiffDialogState();
}

class _AutomationDiffDialogState extends ConsumerState<AutomationDiffDialog> {
  AutomationJson? page, file;
  Object? error;
  bool busy = true;
  bool get current => mounted && identical(ref.read(automationScopeProvider), widget.scope);
  @override
  void initState() {
    super.initState();
    load(0);
  }

  Future<void> load(int offset) async {
    setState(() {
      busy = true;
      error = null;
      file = null;
    });
    try {
      final result = await widget.service.projectDiff(widget.projectId, currentDigest: widget.digest, offset: offset);
      if (current) {
        if (result['current_digest'] != widget.digest) throw const FormatException('Changed diff revision');
        setState(() => page = result);
      }
    } catch (e) {
      if (current) setState(() => error = e);
    } finally {
      if (current) setState(() => busy = false);
    }
  }

  Future<void> open(String path) async {
    setState(() {
      busy = true;
      error = null;
      file = null;
    });
    try {
      final result = await widget.service.projectFileDiff(widget.projectId, path, currentDigest: widget.digest);
      if (current) {
        if (result['current_digest'] != widget.digest || result['path'] != path) {
          throw const FormatException('Changed diff revision');
        }
        setState(() => file = result);
      }
    } catch (e) {
      if (current) setState(() => error = e);
    } finally {
      if (current) setState(() => busy = false);
    }
  }

  Widget textSide(String label, String? text) {
    final content = text ?? '';
    return Expanded(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(label, style: Theme.of(context).textTheme.labelLarge),
          if (content.length > 32768) Text(context.l10n.automationGitDiffTruncated),
          Expanded(
            child: SingleChildScrollView(
              child: SelectableText(
                content.length > 32768 ? content.substring(0, 32768) : content,
                style: const TextStyle(fontFamily: 'monospace', fontSize: 12),
              ),
            ),
          ),
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    if (!identical(ref.watch(automationScopeProvider), widget.scope)) return const SizedBox.shrink();
    final offset = page?['offset'] as int? ?? 0, total = page?['total'] as int? ?? 0;
    final changes = (page?['changes'] as List? ?? []).cast<AutomationJson>();
    return AlertDialog(
      title: Text(l.automationGitDiff),
      content: SizedBox(
        width: 920,
        height: 560,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(l.automationGitDiffHint),
            if (busy) const LinearProgressIndicator(),
            if (error != null)
              Text(errorMessage(l, error!), style: TextStyle(color: Theme.of(context).colorScheme.error)),
            if (file == null) ...[
              Row(
                children: [
                  Text('$total · ${changes.isEmpty ? 0 : offset + 1}–${offset + changes.length}'),
                  const Spacer(),
                  IconButton(
                    tooltip: l.automationInventoryPrevious,
                    onPressed: busy || offset == 0 ? null : () => load((offset - 50).clamp(0, total)),
                    icon: const Icon(Icons.chevron_left),
                  ),
                  IconButton(
                    tooltip: l.automationInventoryNext,
                    onPressed: busy || offset + changes.length >= total ? null : () => load(offset + 50),
                    icon: const Icon(Icons.chevron_right),
                  ),
                ],
              ),
              Expanded(
                child: ListView.builder(
                  itemCount: changes.length,
                  itemBuilder: (context, index) {
                    final change = changes[index];
                    final kind = switch (change['kind']) {
                      'added' => l.automationGitDiffAdded,
                      'deleted' => l.automationGitDiffDeleted,
                      _ => l.automationGitDiffModified,
                    };
                    final beforeMode = (change['before'] as Map?)?['mode'] as int?;
                    final afterMode = (change['after'] as Map?)?['mode'] as int?;
                    return ListTile(
                      dense: true,
                      title: Text(change['path'] as String),
                      subtitle: Text(
                        beforeMode != afterMode
                            ? '$kind · ${beforeMode?.toRadixString(8) ?? '—'} → ${afterMode?.toRadixString(8) ?? '—'}'
                            : kind,
                      ),
                      onTap: busy ? null : () => open(change['path'] as String),
                    );
                  },
                ),
              ),
            ] else ...[
              TextButton(onPressed: busy ? null : () => setState(() => file = null), child: Text(l.commonBack)),
              Text(file!['path'] as String),
              if (file!['preview'] != 'text')
                Expanded(child: Center(child: Text(l.automationGitDiffBinary)))
              else
                Expanded(
                  child: LayoutBuilder(
                    builder: (context, size) {
                      final children = [
                        textSide(l.automationGitDiffOriginal, file!['before'] as String?),
                        const SizedBox(width: 16, height: 16),
                        textSide(l.automationGitDiffCurrent, file!['after'] as String?),
                      ];
                      return size.maxWidth < 620
                          ? Column(children: children)
                          : Row(crossAxisAlignment: CrossAxisAlignment.stretch, children: children);
                    },
                  ),
                ),
            ],
          ],
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context, false), child: Text(l.commonCancel)),
        FilledButton(
          key: const ValueKey('automation-diff-reviewed'),
          onPressed: busy || error != null || page == null ? null : () => Navigator.pop(context, true),
          child: Text(l.automationGitDiffReviewed),
        ),
      ],
    );
  }
}
