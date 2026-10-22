import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_history_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:consolecrypt/kubernetes/kube_workspace_panel.dart';
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
    final approved = await showKubePassphraseConfirmation(
      context,
      subject: widget.contextName,
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
    final tokens = GlassTokens.of(context);
    final status = widget.sessionId == null
        ? null
        : (switch (_recording) {
            'recording' => (l.kubeHistoryRecording, tokens.palette.success),
            'disabled' => (l.kubeHistoryDisabled, tokens.palette.tertiary),
            'connecting' || 'reconnecting' => (l.kubeHistoryConnecting, tokens.palette.warning),
            _ => (l.kubeHistoryUnavailable, tokens.palette.tertiary),
          });
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(l.kubeHistoryHelp, style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel)),
        if (status case (final label, final color)) ...[
          const SizedBox(height: GlassSpacing.s4),
          Row(
            children: [
              StatusDot(color: color),
              const SizedBox(width: GlassSpacing.s6),
              Text(label, style: tokens.typography.callout.copyWith(color: tokens.palette.label)),
            ],
          ),
        ],
        const SizedBox(height: GlassSpacing.s8),
        Row(
          children: [
            Expanded(
              child: GlassField(
                key: const ValueKey('kube-history-search'),
                search: true,
                placeholder: l.commonSearch,
                onChanged: (v) => setState(() => _query = v.toLowerCase()),
              ),
            ),
            const SizedBox(width: GlassSpacing.s8),
            KubePanelTab(
              key: const ValueKey('kube-history-warnings'),
              label: l.kubeHistoryWarnings,
              selected: _warningOnly,
              leading: StatusDot(color: tokens.palette.warning, size: 7),
              onSelected: () {
                if (_loading) return;
                setState(() => _warningOnly = !_warningOnly);
                unawaited(_load());
              },
            ),
            const SizedBox(width: GlassSpacing.s4),
            GlassIconButton(icon: Icons.refresh, tooltip: l.kubeRefresh, onPressed: _loading ? null : _load),
            GlassIconButton(
              icon: Icons.delete_sweep_outlined,
              tooltip: l.kubeHistoryClear,
              onPressed: _loading ? null : _clear,
            ),
          ],
        ),
        const SizedBox(height: GlassSpacing.s8),
        if (_loading) const LinearProgressIndicator(minHeight: 2),
        if (_failed) Text(l.kubeRequestFailed, style: TextStyle(color: tokens.palette.danger)),
        Expanded(
          child: ListView.separated(
            key: const ValueKey('kube-history-list'),
            itemCount: rows.length + (_more ? 1 : 0),
            separatorBuilder: (_, _) => const SizedBox(height: GlassSpacing.s6),
            itemBuilder: (context, index) {
              if (index == rows.length) {
                return Center(
                  child: GlassButton(label: l.kubeLoadMore, onPressed: _loading ? null : () => _load(more: true)),
                );
              }
              return _EventCard(key: ValueKey('kube-event-${rows[index]['uid']}'), event: rows[index]);
            },
          ),
        ),
      ],
    );
  }
}

/// One recorded event as a block: type icon, reason and object, namespace,
/// time and repeat count; the message unfolds below.
class _EventCard extends StatefulWidget {
  const _EventCard({required this.event, super.key});
  final Map<String, dynamic> event;
  @override
  State<_EventCard> createState() => _EventCardState();
}

class _EventCardState extends State<_EventCard> {
  bool _open = false;

  static String _time(int? ms) {
    if (ms == null) return '';
    final t = DateTime.fromMillisecondsSinceEpoch(ms).toLocal();
    String two(int v) => v.toString().padLeft(2, '0');
    return '${t.year}-${two(t.month)}-${two(t.day)} ${two(t.hour)}:${two(t.minute)}:${two(t.second)}'; // l10n-ignore: ISO-like timestamp
  }

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final event = widget.event;
    final warning = event['event_type'] == 'Warning';
    final color = warning ? tokens.palette.warning : tokens.palette.accent;
    final message = event['message'] as String? ?? '';
    final count = event['count'];
    final title = [event['reason'], '${event['object_kind']} ${event['object_name']}'].join(' · '); // l10n-ignore: ids
    final subtitle = [event['namespace'], _time(event['last_seen_ms'] as int?)].join(' · '); // l10n-ignore: ids
    return ContentSurface(
      padding: EdgeInsets.zero,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          GlassInteractive(
            onPressed: message.isEmpty ? null : () => setState(() => _open = !_open),
            builder: (context, state) => Padding(
              padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s12, vertical: GlassSpacing.s8),
              child: Row(
                children: [
                  Icon(warning ? Icons.warning_amber_rounded : Icons.info_outline_rounded, size: 20, color: color),
                  const SizedBox(width: GlassSpacing.s12),
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(
                          title,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: tokens.typography.bodyEmph.copyWith(color: tokens.palette.label),
                        ),
                        const SizedBox(height: 2),
                        Text(subtitle, style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel)),
                      ],
                    ),
                  ),
                  if (count is int && count > 1) ...[
                    GlassBadge(
                      label: '×$count',
                      dense: true,
                      tone: warning ? GlassTone.warning : GlassTone.neutral,
                    ), // l10n-ignore: count
                    const SizedBox(width: GlassSpacing.s8),
                  ],
                  if (message.isNotEmpty)
                    AnimatedRotation(
                      turns: _open ? .5 : 0,
                      duration: const Duration(milliseconds: 150),
                      child: Icon(Icons.expand_more_rounded, color: tokens.secondaryLabel),
                    ),
                ],
              ),
            ),
          ),
          if (_open)
            Container(
              margin: const EdgeInsets.fromLTRB(GlassSpacing.s12, 0, GlassSpacing.s12, GlassSpacing.s12),
              padding: const EdgeInsets.all(GlassSpacing.s8),
              decoration: BoxDecoration(
                color: tokens.surfaces.inset,
                borderRadius: BorderRadius.circular(8),
                border: Border(left: BorderSide(color: color, width: 3)),
              ),
              child: SelectableText(
                message,
                style: tokens.typography.mono.copyWith(color: tokens.palette.label, fontSize: 12),
              ),
            ),
        ],
      ),
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
