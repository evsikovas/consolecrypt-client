import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_history_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:consolecrypt/kubernetes/kube_write_access.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class KubeHistoryPanel extends ConsumerStatefulWidget {
  const KubeHistoryPanel({
    required this.contextId,
    required this.contextName,
    required this.service,
    this.sessionId,
    this.namespace,
    super.key,
  });
  final String contextId, contextName;
  final String? sessionId, namespace;
  final KubeHistoryService service;
  @override
  ConsumerState<KubeHistoryPanel> createState() => _KubeHistoryPanelState();
}

class _KubeHistoryPanelState extends ConsumerState<KubeHistoryPanel> {
  late final KubeScope _scope = ref.read(kubeScopeProvider);
  final _rows = <KubeJson>[];
  bool _warningOnly = false, _loading = false, _failed = false, _more = false;
  String _query = '', _recording = 'disabled';
  int _generation = 0;
  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(kubeScopeProvider));
  @override
  void initState() {
    super.initState();
    unawaited(_load());
  }

  @override
  void dispose() {
    _generation++;
    _rows.clear();
    super.dispose();
  }

  Future<void> _load({bool more = false}) async {
    if (!_current || _loading) return;
    final generation = ++_generation;
    final before = more && _rows.isNotEmpty ? _rows.last['last_seen_ms'] as int? : null;
    final beforeUid = more && _rows.isNotEmpty ? _rows.last['uid'] as String? : null;
    setState(() {
      _loading = true;
      _failed = false;
      if (!more) _rows.clear();
    });
    try {
      if (widget.sessionId != null && !more) {
        try {
          final status = await widget.service.historyStatus(widget.sessionId!);
          if (_current && generation == _generation) _recording = status['state'] as String? ?? 'unavailable';
        } catch (_) {
          _recording = 'unavailable';
        }
      }
      final rows = await widget.service.eventHistory(
        widget.contextId,
        namespace: widget.namespace,
        warningOnly: _warningOnly,
        beforeMs: before,
        beforeUid: beforeUid,
        limit: 200,
      );
      if (!_current || generation != _generation) return;
      setState(() {
        final ids = _rows.map((row) => row['uid']).toSet();
        _rows.addAll(rows.where((row) => ids.add(row['uid'])));
        _more = rows.length == 200 && _rows.length < 5000;
      });
    } catch (_) {
      if (_current && generation == _generation) setState(() => _failed = true);
    } finally {
      if (_current && generation == _generation) setState(() => _loading = false);
    }
  }

  Future<void> _clear() async {
    if (!_current || _loading) return;
    final approved = await showTypedKubeConfirmation(
      context,
      expected: widget.contextName,
      title: context.l10n.kubeHistoryClear,
      message: context.l10n.kubeHistoryClearHelp,
      confirmLabel: context.l10n.commonDelete,
    );
    if (!_current || !approved) return;
    setState(() => _loading = true);
    try {
      await widget.service.clearEventHistory(widget.contextId);
      if (_current) {
        setState(() {
          _rows.clear();
          _more = false;
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
    if (!_scope.unlocked || !identical(_scope, ref.watch(kubeScopeProvider))) return const SizedBox.shrink();
    final l = context.l10n;
    final rows = _rows
        .where(
          (row) => '${row['namespace']} ${row['object_kind']} ${row['object_name']} ${row['reason']} ${row['message']}'
              .toLowerCase()
              .contains(_query),
        )
        .toList();
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(l.kubeHistoryHelp),
        if (widget.sessionId != null)
          Text(switch (_recording) {
            'recording' => l.kubeHistoryRecording,
            'disabled' => l.kubeHistoryDisabled,
            'connecting' || 'reconnecting' => l.kubeHistoryConnecting,
            _ => l.kubeHistoryUnavailable,
          }),
        Row(
          children: [
            Expanded(
              child: TextField(
                key: const ValueKey('kube-history-search'),
                decoration: InputDecoration(labelText: l.commonSearch),
                onChanged: (v) => setState(() => _query = v.toLowerCase()),
              ),
            ),
            GlassIconButton(icon: Icons.refresh, tooltip: l.kubeRefresh, onPressed: _loading ? null : _load),
            GlassIconButton(
              icon: Icons.delete_sweep_outlined,
              tooltip: l.kubeHistoryClear,
              onPressed: _loading ? null : _clear,
            ),
          ],
        ),
        Align(
          alignment: Alignment.centerLeft,
          child: FilterChip(
            key: const ValueKey('kube-history-warnings'),
            label: Text(l.kubeHistoryWarnings),
            selected: _warningOnly,
            onSelected: _loading
                ? null
                : (v) {
                    setState(() => _warningOnly = v);
                    unawaited(_load());
                  },
          ),
        ),
        if (_loading) const LinearProgressIndicator(),
        if (_failed) Text(l.kubeRequestFailed),
        Expanded(
          child: ListView.builder(
            key: const ValueKey('kube-history-list'),
            itemCount: rows.length + (_more ? 1 : 0),
            itemBuilder: (context, index) {
              if (index == rows.length) {
                return GlassButton(label: l.kubeLoadMore, onPressed: _loading ? null : () => _load(more: true));
              }
              final event = rows[index];
              final timestamp = event['last_seen_ms'] as int?;
              return ExpansionTile(
                key: ValueKey('kube-event-${event['uid']}'),
                leading: Icon(event['event_type'] == 'Warning' ? Icons.warning_amber_outlined : Icons.info_outline),
                title: Text(
                  /* l10n-ignore: Kubernetes event identifiers, timestamp and numeric count. */ '${event['reason']} · ${event['object_kind']} ${event['object_name']}',
                ), // l10n-ignore: Kubernetes event identifiers, timestamp and numeric count.
                subtitle: Text(
                  // l10n-ignore: Kubernetes event identifiers, timestamp and numeric count.
                  /* l10n-ignore: Kubernetes event identifiers, timestamp and numeric count. */ '${event['namespace']} · ${timestamp == null ? '' : DateTime.fromMillisecondsSinceEpoch(timestamp).toLocal()} · ×${event['count']}',
                ),
                children: [SelectableText(event['message'] as String? ?? '')],
              );
            },
          ),
        ),
      ],
    );
  }
}

class KubeHistoryDialog extends ConsumerStatefulWidget {
  const KubeHistoryDialog({required this.connection, required this.service, super.key});
  final KubeConnection connection;
  final KubeHistoryService service;
  @override
  ConsumerState<KubeHistoryDialog> createState() => _KubeHistoryDialogState();
}

class _KubeHistoryDialogState extends ConsumerState<KubeHistoryDialog> {
  late final KubeScope _scope = ref.read(kubeScopeProvider);
  @override
  Widget build(BuildContext context) {
    ref.listen(kubeScopeProvider, (_, next) {
      if (identical(_scope, next)) return;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted) return;
        final route = ModalRoute.of(context);
        if (route?.isActive == true) Navigator.of(context).removeRoute(route!);
      });
    });
    return GlassDialog(
      title: '${context.l10n.kubeHistory} · ${widget.connection.name}',
      width: 1000,
      content: SizedBox(
        height: 580,
        child: KubeHistoryPanel(
          contextId: widget.connection.id,
          contextName: widget.connection.name,
          service: widget.service,
        ),
      ),
      primaryAction: GlassButton(label: context.l10n.commonClose, onPressed: () => closeDialog<void>(context)),
    );
  }
}
