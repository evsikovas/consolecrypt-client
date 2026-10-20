import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_ai_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:consolecrypt/kubernetes/kube_write_access.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class KubeAiHistoryDialog extends ConsumerStatefulWidget {
  const KubeAiHistoryDialog({required this.contextId, required this.service, this.onResume, super.key});
  final String contextId;
  final KubeAiHistoryService service;
  final void Function(String conversationId, List<KubeAiEvent> events)? onResume;
  @override
  ConsumerState<KubeAiHistoryDialog> createState() => _KubeAiHistoryDialogState();
}

class _KubeAiHistoryDialogState extends ConsumerState<KubeAiHistoryDialog> with WidgetsBindingObserver {
  late final KubeScope _scope = ref.read(kubeScopeProvider);
  List<KubeJson> _rows = [];
  KubeJson? _selected;
  bool _loading = true, _failed = false;
  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(kubeScopeProvider));
  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    ref.listenManual(kubeScopeProvider, (_, next) {
      if (!identical(_scope, next)) _close();
    });
    unawaited(_load());
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _rows.clear();
    _selected = null;
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.hidden || state == AppLifecycleState.paused) _close();
  }

  void _close() {
    _rows.clear();
    _selected = null;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted) return;
      final route = ModalRoute.of(context);
      if (route?.isActive == true) Navigator.of(context).removeRoute(route!);
    });
    if (mounted) setState(() {});
  }

  Future<void> _load() async {
    try {
      final rows = await widget.service.kubeAiHistory(widget.contextId);
      if (_current) setState(() => _rows = rows.take(100).toList());
    } catch (_) {
      if (_current) setState(() => _failed = true);
    } finally {
      if (_current) setState(() => _loading = false);
    }
  }

  Future<void> _delete(KubeJson row) async {
    if (!_current || _loading) return;
    final conversation = row['conversation'] as KubeJson;
    final title = conversation['title'] as String;
    final approved = await showTypedKubeConfirmation(
      context,
      expected: title,
      title: context.l10n.commonDelete,
      message: context.l10n.kubeAiHistoryDeleteHelp,
      confirmLabel: context.l10n.commonDelete,
    );
    if (!_current || !approved) return;
    setState(() {
      _loading = true;
      _failed = false;
    });
    try {
      await widget.service.deleteKubeAiConversation(conversation['id'] as String);
      if (_current) {
        setState(() {
          _rows.remove(row);
          if (identical(_selected, row)) _selected = null;
        });
      }
    } catch (_) {
      if (_current) setState(() => _failed = true);
    } finally {
      if (_current) setState(() => _loading = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final selected = _selected;
    final messages = ((selected?['conversation'] as KubeJson?)?['messages'] as List? ?? const []).cast<KubeJson>();
    return GlassDialog(
      title: l.kubeAiHistory,
      content: SizedBox(
        width: 700,
        height: MediaQuery.sizeOf(context).height * .65,
        child: !_current
            ? const SizedBox.shrink()
            : Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  if (_loading) const LinearProgressIndicator(),
                  if (_failed) Text(l.kubeRequestFailed),
                  if (selected != null)
                    GlassButton(label: l.commonBack, onPressed: () => setState(() => _selected = null)),
                  Expanded(
                    child: selected != null
                        ? ListView.builder(
                            itemCount: messages.length,
                            itemBuilder: (_, index) {
                              final message = messages[index];
                              return Padding(
                                padding: const EdgeInsets.all(8),
                                child: Column(
                                  crossAxisAlignment: CrossAxisAlignment.stretch,
                                  children: [
                                    Text(
                                      message['role'] == 'user' ? l.kubeAiQuestion : l.kubeAiAssistant,
                                      style: Theme.of(context).textTheme.titleSmall,
                                    ),
                                    SelectableText(message['content'] as String? ?? ''),
                                  ],
                                ),
                              );
                            },
                          )
                        : _rows.isEmpty && !_loading
                        ? Text(l.kubeNoResources)
                        : ListView.builder(
                            itemCount: _rows.length,
                            itemBuilder: (_, index) {
                              final row = _rows[index], conversation = _rows[index]['conversation'] as KubeJson;
                              return ListTile(
                                title: Text(conversation['title'] as String? ?? l.kubeAiAssistant),
                                subtitle: Text(row['synced'] == true ? l.kubeAiHistorySynced : l.kubeAiHistoryLocal),
                                onTap: () => setState(() => _selected = row),
                                trailing: GlassIconButton(
                                  icon: Icons.delete_outline,
                                  tooltip: l.commonDelete,
                                  onPressed: _loading ? null : () => _delete(row),
                                ),
                              );
                            },
                          ),
                  ),
                ],
              ),
      ),
      primaryAction: GlassButton(label: l.commonClose, onPressed: () => closeDialog<void>(context)),
      secondaryActions: [
        if (selected != null && widget.onResume != null)
          GlassButton.prominent(
            label: l.commonContinue,
            onPressed: () {
              if (!_current) return;
              final id = (selected['conversation'] as KubeJson)['id'] as String;
              final events = messages
                  .where((m) => m['role'] == 'user' || m['role'] == 'assistant')
                  .map(
                    (m) => KubeAiEvent(m['role'] == 'user' ? 'question' : 'answer', {
                      'content': m['content'] as String? ?? '',
                    }),
                  )
                  .toList();
              final resume = widget.onResume!;
              closeDialog<void>(context);
              WidgetsBinding.instance.addPostFrameCallback((_) {
                resume(id, events);
              });
            },
          ),
      ],
    );
  }
}
