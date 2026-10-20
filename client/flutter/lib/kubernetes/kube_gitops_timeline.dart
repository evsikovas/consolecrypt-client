import 'dart:async';

import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_history_service.dart';
import 'package:consolecrypt/kubernetes/kube_insights_service.dart';
import 'package:consolecrypt/kubernetes/kube_operations_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class KubeGitOpsTimeline extends ConsumerStatefulWidget {
  const KubeGitOpsTimeline({
    required this.session,
    required this.resources,
    required this.service,
    required this.onOpen,
    this.namespace,
    super.key,
  });
  final KubeSession session;
  final List<KubeInsightResource> resources;
  final KubeBrowserService service;
  final String? namespace;
  final ValueChanged<KubeInsightResource> onOpen;
  @override
  ConsumerState<KubeGitOpsTimeline> createState() => _KubeGitOpsTimelineState();
}

class _KubeGitOpsTimelineState extends ConsumerState<KubeGitOpsTimeline> with WidgetsBindingObserver {
  late final KubeScope _scope = ref.read(kubeScopeProvider);
  final _events = <String, _TimelineEntry>{};
  Timer? _refresh;
  int _generation = 0;
  bool _expanded = false, _loading = false, _partial = false;
  bool _foreground = true;
  bool get _current => mounted && _foreground && _scope.unlocked && identical(_scope, ref.read(kubeScopeProvider));
  List<KubeInsightResource> get _resources => widget.resources
      .where((r) => (r.isFlux || r.isArgo) && (widget.namespace == null || r.resource.namespace == widget.namespace))
      .toList();

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.resumed) {
      _foreground = true;
      if (_expanded) unawaited(_load());
    } else if (state == AppLifecycleState.paused ||
        state == AppLifecycleState.hidden ||
        state == AppLifecycleState.detached) {
      _foreground = false;
      _generation++;
      _refresh?.cancel();
    }
  }

  @override
  void didUpdateWidget(KubeGitOpsTimeline old) {
    super.didUpdateWidget(old);
    final previous = old.resources.map((r) => '${r.resource.uid}:${r.resource.resourceVersion}').join('|');
    final next = widget.resources.map((r) => '${r.resource.uid}:${r.resource.resourceVersion}').join('|');
    if (previous != next || old.namespace != widget.namespace) {
      _generation++;
      _refresh?.cancel();
      if (old.namespace != widget.namespace) _events.clear();
      if (_expanded) _refresh = Timer(const Duration(seconds: 1), () => unawaited(_load()));
    }
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _generation++;
    _refresh?.cancel();
    _events.clear();
    super.dispose();
  }

  Future<void> _load() async {
    if (!_current || !_expanded) return;
    final generation = ++_generation, resources = _resources;
    final events = <String, _TimelineEntry>{};
    var partial = resources.length > 12;
    setState(() => _loading = true);
    final service = widget.service;
    if (service is KubeOperationsService) {
      final selected = resources.take(12).toList();
      for (var offset = 0; offset < selected.length; offset += 3) {
        if (!_current || generation != _generation) return;
        await Future.wait(
          selected.skip(offset).take(3).map((resource) async {
            if (resource.resource.uid.isEmpty) {
              partial = true;
              return;
            }
            try {
              final rows = await (service as KubeOperationsService).objectEvents(
                widget.session.id,
                resource.reference,
                resource.resource.uid,
              );
              for (final row in rows.take(50)) {
                final object = row.object;
                final time = DateTime.tryParse(
                  (object['lastTimestamp'] ?? object['eventTime'] ?? (object['series'] as Map?)?['lastObservedTime'])
                          ?.toString() ??
                      '',
                );
                if (time == null) continue;
                final annotations = (object['metadata'] as Map?)?['annotations'] as Map? ?? const {};
                events[row.uid] = _TimelineEntry(
                  resource: resource,
                  time: time,
                  result: object['reason']?.toString() ?? '—',
                  warning: object['type'] == 'Warning',
                  revision: annotations['reconcile.fluxcd.io/revision']?.toString() ?? '—',
                );
              }
            } catch (_) {
              partial = true;
            }
          }),
        );
      }
    } else {
      partial = true;
    }
    if (!_current || generation != _generation) return;
    if (service is KubeHistoryService) {
      try {
        final history = service as KubeHistoryService;
        final status = await history.historyStatus(widget.session.id);
        if (!_current || generation != _generation) return;
        if (status['enabled'] == true) {
          final rows = await history.eventHistory(widget.session.contextId, namespace: widget.namespace, limit: 100);
          final byUid = {for (final resource in resources) resource.resource.uid: resource};
          for (final row in rows) {
            final resource = byUid[row['object_uid']];
            final time = row['last_seen_ms'];
            if (resource == null || time is! int) continue;
            events.putIfAbsent(
              row['uid'] as String,
              () => _TimelineEntry(
                resource: resource,
                time: DateTime.fromMillisecondsSinceEpoch(time),
                result: row['reason']?.toString() ?? '—',
                warning: row['event_type'] == 'Warning',
              ),
            );
          }
        }
      } catch (_) {
        partial = true;
      }
    }
    if (_current && generation == _generation) {
      setState(() {
        _events
          ..clear()
          ..addAll(events);
        _partial = partial;
        _loading = false;
      });
    }
  }

  List<_TimelineEntry> _statusEntries() {
    final entries = <_TimelineEntry>[];
    for (final resource in _resources) {
      final status = resource.object['status'] as KubeJson? ?? const {};
      final revision =
          status['lastAppliedRevision'] ??
          (status['artifact'] as Map?)?['revision'] ??
          (status['sync'] as Map?)?['revision'];
      for (final condition in (status['conditions'] as List? ?? const []).whereType<KubeJson>()) {
        if (!const ['Ready', 'Stalled'].contains(condition['type'])) continue;
        final time = DateTime.tryParse(condition['lastTransitionTime']?.toString() ?? '');
        if (time != null) {
          entries.add(
            _TimelineEntry(
              resource: resource,
              time: time,
              revision: revision?.toString() ?? '—',
              result: condition['reason']?.toString() ?? '—',
              warning: condition['type'] == 'Stalled' ? condition['status'] == 'True' : condition['status'] != 'True',
            ),
          );
        }
      }
      for (final item in (status['history'] as List? ?? const []).take(20).whereType<KubeJson>()) {
        final time = DateTime.tryParse((item['lastDeployed'] ?? item['deployStartedAt'])?.toString() ?? '');
        if (time == null) continue;
        entries.add(
          _TimelineEntry(
            resource: resource,
            time: time,
            revision: item['chartVersion']?.toString() ?? '—',
            result: item['status']?.toString() ?? '—',
            duration: item['duration']?.toString() ?? '—',
            warning: item['status'] == 'failed',
          ),
        );
      }
    }
    return entries;
  }

  @override
  Widget build(BuildContext context) {
    final scope = ref.watch(kubeScopeProvider);
    if (!_current || !identical(scope, _scope)) {
      _events.clear();
      return const SizedBox.shrink();
    }
    final entries = [..._events.values, ..._statusEntries()]..sort((a, b) => b.time.compareTo(a.time));
    return ConstrainedBox(
      constraints: const BoxConstraints(maxHeight: 230),
      child: SingleChildScrollView(
        child: ExpansionTile(
          key: const ValueKey('kube-gitops-timeline'),
          title: Text(context.l10n.kubeGitOpsTimeline),
          onExpansionChanged: (value) {
            _expanded = value;
            if (value) {
              unawaited(_load());
            } else {
              _generation++;
              _refresh?.cancel();
            }
          },
          children: [
            if (_loading) const LinearProgressIndicator(),
            if (_partial) Text(context.l10n.kubeInsightsPartial),
            if (entries.isEmpty) Text(context.l10n.kubeGitOpsHistoryEmpty),
            for (final entry in entries.take(40))
              ListTile(
                leading: Icon(entry.warning ? Icons.error_outline : Icons.check_circle_outline),
                title: Text('${entry.resource.resource.namespace}/${entry.resource.resource.name}'),
                subtitle: Text('${entry.time.toLocal()} · ${entry.revision} · ${entry.duration}\n${entry.result}'),
                onTap: () => widget.onOpen(entry.resource),
              ),
          ],
        ),
      ),
    );
  }
}

final class _TimelineEntry {
  const _TimelineEntry({
    required this.resource,
    required this.time,
    required this.result,
    required this.warning,
    this.revision = '—',
    this.duration = '—',
  });
  final KubeInsightResource resource;
  final DateTime time;
  final String result, revision, duration;
  final bool warning;
}
