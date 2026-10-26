import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:material_ui/material_ui.dart';

/// Ask AI conversations of this unlock and those kept in the vault. Closes
/// with the conversation to continue, or null.
class AiHistoryDialog extends StatefulWidget {
  const AiHistoryDialog({required this.history, required this.providers, super.key});
  final AiConversationHistory history;

  /// Configured providers: a conversation of a removed provider cannot go on.
  final List<AiProviderConfig> providers;

  @override
  State<AiHistoryDialog> createState() => _AiHistoryDialogState();
}

class _AiHistoryDialogState extends State<AiHistoryDialog> {
  List<AiConversation> _rows = const [];
  bool _loading = true, _failed = false;

  @override
  void initState() {
    super.initState();
    unawaited(_load());
  }

  Future<void> _load() async {
    setState(() {
      _loading = true;
      _failed = false;
    });
    try {
      final rows = await widget.history.listConversations();
      if (mounted) setState(() => _rows = rows);
    } on Object {
      if (mounted) setState(() => _failed = true);
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  Future<void> _delete(AiConversation conversation) async {
    try {
      await widget.history.deleteConversation(conversation.id);
    } on Object {
      if (mounted) setState(() => _failed = true);
    }
    await _load();
  }

  bool _resumable(AiConversation c) => widget.providers.any((p) => p.id == c.providerId);

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    return GlassDialog(
      key: const ValueKey('ai-history-dialog'),
      title: l.aiHistoryTitle,
      content: SizedBox(
        width: 640,
        height: MediaQuery.sizeOf(context).height * .6,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            if (_loading) const LinearProgressIndicator(),
            if (_failed) Text(l.aiHistoryFailed, style: TextStyle(color: tokens.palette.danger)),
            Expanded(
              child: _rows.isEmpty && !_loading
                  ? Text(l.aiHistoryEmpty, key: const ValueKey('ai-history-empty'))
                  : ListView.builder(
                      itemCount: _rows.length,
                      itemBuilder: (_, index) {
                        final c = _rows[index];
                        final count = c.messages.where((m) => m.role != ChatRole.system).length;
                        return ListTile(
                          key: ValueKey('ai-history-${c.id.value}'),
                          title: Text(
                            c.title.isEmpty ? l.aiHistoryUntitled : c.title,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                          ),
                          subtitle: Text(
                            [
                              formatRelative(l, c.updatedAt),
                              l.aiHistoryMessages(count),
                              if (!_resumable(c)) l.aiHistoryProviderGone,
                              c.persisted ? l.aiHistoryKept : l.aiHistoryUntilLock,
                            ].join(' · '),
                          ),
                          enabled: _resumable(c),
                          onTap: _resumable(c) ? () => closeDialog<AiConversation>(context, c) : null,
                          trailing: GlassIconButton(
                            key: ValueKey('ai-history-delete-${c.id.value}'),
                            icon: Icons.delete_outline,
                            tooltip: l.commonDelete,
                            onPressed: _loading ? null : () => unawaited(_delete(c)),
                          ),
                        );
                      },
                    ),
            ),
          ],
        ),
      ),
      primaryAction: GlassButton(label: l.commonClose, onPressed: () => closeDialog<AiConversation>(context)),
    );
  }
}
