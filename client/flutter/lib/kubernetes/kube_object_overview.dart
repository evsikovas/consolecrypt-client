import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_resource_table_view.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:material_ui/material_ui.dart';

Map<Object?, Object?> _map(Object? value) => value is Map ? value : const {};
List<Map<Object?, Object?>> _maps(Object? value) =>
    value is List ? value.whereType<Map<Object?, Object?>>().toList() : const [];
String? _text(Object? value) => value is String || value is num || value is bool ? value.toString() : null;

/// Compact "age" of a Kubernetes timestamp (2d, 5h, 12m).
String kubeAge(Object? timestamp) {
  final time = DateTime.tryParse(_text(timestamp) ?? '');
  if (time == null) return '—';
  final age = DateTime.now().toUtc().difference(time.toUtc());
  if (age.inDays > 0) return '${age.inDays}d'; // l10n-ignore: Kubernetes duration.
  if (age.inHours > 0) return '${age.inHours}h'; // l10n-ignore: Kubernetes duration.
  if (age.inMinutes > 0) return '${age.inMinutes}m'; // l10n-ignore: Kubernetes duration.
  return '${age.inSeconds.clamp(0, 59)}s'; // l10n-ignore: Kubernetes duration.
}

/// Readable status of an object for the inspector header.
String kubeObjectStatus(KubeResourceRef resource, KubeJson object) {
  final status = _map(object['status']);
  if (resource.api.isPod) {
    for (final state in _maps(status['containerStatuses'])) {
      final waiting = _map(_map(state['state'])['waiting']);
      if (_text(waiting['reason']) case final reason?) return reason;
    }
    if (_map(object['metadata'])['deletionTimestamp'] != null) return 'Terminating'; // l10n-ignore: Kubernetes state.
    return _text(status['phase']) ?? '';
  }
  final replicas = status['replicas'], ready = status['readyReplicas'] ?? status['numberReady'];
  if (replicas is num) return '${ready ?? 0}/$replicas';
  if (_text(status['phase']) case final phase?) return phase;
  for (final condition in _maps(status['conditions'])) {
    if (condition['type'] == 'Ready') {
      return condition['status'] == 'True' ? 'Ready' : 'NotReady'; // l10n-ignore: Kubernetes state.
    }
  }
  return '';
}

/// Section heading inside the inspector.
class KubeSection extends StatelessWidget {
  const KubeSection({required this.title, required this.child, super.key, this.trailing});
  final String title;
  final Widget child;
  final Widget? trailing;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    return Padding(
      padding: const EdgeInsets.only(bottom: GlassSpacing.s16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Expanded(
                child: Text(
                  title.toUpperCase(),
                  style: tokens.typography.caption.copyWith(
                    color: tokens.secondaryLabel,
                    fontWeight: FontWeight.w600,
                    letterSpacing: 0.6,
                  ),
                ),
              ),
              ?trailing,
            ],
          ),
          const SizedBox(height: GlassSpacing.s6),
          child,
        ],
      ),
    );
  }
}

/// Two-column key/value grid; values are selectable.
class KubeInfoGrid extends StatelessWidget {
  const KubeInfoGrid({required this.entries, super.key});
  final List<(String, String)> entries;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    return LayoutBuilder(
      builder: (context, constraints) {
        final columns = constraints.maxWidth >= 520 ? 2 : 1;
        final width = (constraints.maxWidth - (columns - 1) * GlassSpacing.s16) / columns;
        return Wrap(
          spacing: GlassSpacing.s16,
          runSpacing: GlassSpacing.s8,
          children: [
            for (final (label, value) in entries)
              SizedBox(
                width: width,
                child: Row(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    SizedBox(
                      width: 110,
                      child: Text(label, style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel)),
                    ),
                    Expanded(
                      child: SelectableText(
                        value.isEmpty ? '—' : value,
                        style: tokens.typography.callout.copyWith(color: tokens.palette.label),
                      ),
                    ),
                  ],
                ),
              ),
          ],
        );
      },
    );
  }
}

/// `key=value` capsules for labels and annotations.
class KubeLabelChips extends StatelessWidget {
  const KubeLabelChips({required this.values, super.key, this.limit = 24});
  final Map<Object?, Object?> values;
  final int limit;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final entries = values.entries.take(limit).toList();
    return Wrap(
      spacing: GlassSpacing.s4,
      runSpacing: GlassSpacing.s4,
      children: [
        for (final entry in entries)
          Tooltip(
            message: '${entry.key}=${entry.value}',
            waitDuration: const Duration(milliseconds: 500),
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 360),
              child: DecoratedBox(
                decoration: ShapeDecoration(color: tokens.surfaces.inset, shape: const StadiumBorder()),
                child: Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
                  child: Text.rich(
                    TextSpan(
                      children: [
                        TextSpan(
                          text: '${entry.key}',
                          style: TextStyle(color: tokens.secondaryLabel),
                        ),
                        TextSpan(text: '=${entry.value}'),
                      ],
                    ),
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: tokens.typography.caption.copyWith(color: tokens.palette.label),
                  ),
                ),
              ),
            ),
          ),
        if (values.length > limit)
          Text('+${values.length - limit}', style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel)),
      ],
    );
  }
}

/// Conditions as compact rows: status dot, type, reason and age.
class KubeConditions extends StatelessWidget {
  const KubeConditions({required this.conditions, super.key});
  final List<Map<Object?, Object?>> conditions;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    return Column(
      children: [
        for (final condition in conditions)
          Padding(
            padding: const EdgeInsets.symmetric(vertical: 3),
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                SizedBox(
                  width: 190,
                  child: Row(
                    children: [
                      Container(
                        width: 7,
                        height: 7,
                        decoration: BoxDecoration(
                          shape: BoxShape.circle,
                          color: condition['status'] == 'True' ? tokens.palette.success : tokens.palette.warning,
                        ),
                      ),
                      const SizedBox(width: 7),
                      Flexible(
                        child: Text(
                          _text(condition['type']) ?? '',
                          overflow: TextOverflow.ellipsis,
                          style: tokens.typography.callout.copyWith(color: tokens.palette.label),
                        ),
                      ),
                    ],
                  ),
                ),
                SizedBox(
                  width: 60,
                  child: Text(
                    _text(condition['status']) ?? '',
                    style: tokens.typography.callout.copyWith(
                      color: condition['status'] == 'True' ? tokens.palette.success : tokens.palette.warning,
                    ),
                  ),
                ),
                Expanded(
                  child: Text(
                    [condition['reason'], condition['message']].whereType<String>().join(' · '),
                    maxLines: 3,
                    overflow: TextOverflow.ellipsis,
                    style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel),
                  ),
                ),
                const SizedBox(width: 8),
                Text(
                  kubeAge(condition['lastTransitionTime']),
                  style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel),
                ),
              ],
            ),
          ),
      ],
    );
  }
}

/// Containers of a Pod (or a workload template): image, state, restarts,
/// ports and resource requests/limits. Environment values and commands are
/// deliberately not shown.
class KubeContainers extends StatelessWidget {
  const KubeContainers({required this.resource, required this.object, super.key});
  final KubeResourceRef resource;
  final KubeJson object;

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    final p = tokens.palette;
    final spec = _map(object['spec']), status = _map(object['status']);
    var pod = spec;
    if (!resource.api.isPod) {
      pod = _map(_map(spec['template'])['spec']);
      if (resource.api.kind == 'CronJob') {
        pod = _map(_map(_map(_map(spec['jobTemplate'])['spec'])['template'])['spec']);
      }
    }
    final states = {
      for (final state in [..._maps(status['initContainerStatuses']), ..._maps(status['containerStatuses'])])
        _text(state['name']): state,
    };
    final containers = [
      for (final c in _maps(pod['initContainers'])) (c, true),
      for (final c in _maps(pod['containers'])) (c, false),
    ];
    String quantities(Object? value) => _map(value).entries.map((e) => '${e.key} ${_text(e.value) ?? ''}').join(' · ');
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        for (final (container, init) in containers)
          Padding(
            padding: const EdgeInsets.only(bottom: GlassSpacing.s8),
            child: ContentSurface(
              kind: ContentSurfaceKind.inset,
              padding: const EdgeInsets.all(GlassSpacing.s12),
              child: Builder(
                builder: (context) {
                  final name = _text(container['name']) ?? '';
                  final state = states[name] ?? const {};
                  final current = _map(state['state']);
                  final phase = current.keys.firstOrNull?.toString() ?? '';
                  final reason = _text(_map(current[phase])['reason']);
                  final label = switch (phase) {
                    'running' => state['ready'] == true ? 'Running' : 'NotReady', // l10n-ignore: Kubernetes state.
                    'waiting' => reason ?? 'Waiting', // l10n-ignore: Kubernetes state.
                    'terminated' => reason ?? 'Terminated', // l10n-ignore: Kubernetes state.
                    _ => '',
                  };
                  final resources = _map(container['resources']);
                  final ports = _maps(container['ports'])
                      .map((port) => '${_text(port['containerPort']) ?? ''}/${_text(port['protocol']) ?? 'TCP'}')
                      .join(', ');
                  return Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      Row(
                        children: [
                          Icon(Icons.inventory_2_outlined, size: 16, color: tokens.secondaryLabel),
                          const SizedBox(width: 6),
                          Flexible(
                            child: Text(
                              name,
                              overflow: TextOverflow.ellipsis,
                              style: tokens.typography.bodyEmph.copyWith(color: p.label),
                            ),
                          ),
                          if (init) ...[
                            const SizedBox(width: 6),
                            const GlassBadge(label: 'init', dense: true), // l10n-ignore: Kubernetes term.
                          ],
                          const Spacer(),
                          if (label.isNotEmpty) Flexible(child: KubeStatusLabel(status: label)),
                        ],
                      ),
                      const SizedBox(height: 6),
                      SelectableText(
                        _text(container['image']) ?? '',
                        style: tokens.typography.mono.copyWith(color: tokens.secondaryLabel, fontSize: 12),
                      ),
                      const SizedBox(height: 6),
                      KubeInfoGrid(
                        entries: [
                          if (state.isNotEmpty) (l.kubeRestarts, _text(state['restartCount']) ?? '0'),
                          if (ports.isNotEmpty) (l.kubePort, ports),
                          if (_map(resources['requests']).isNotEmpty)
                            (l.kubeRequests, quantities(resources['requests'])),
                          if (_map(resources['limits']).isNotEmpty) (l.kubeLimits, quantities(resources['limits'])),
                        ],
                      ),
                    ],
                  );
                },
              ),
            ),
          ),
      ],
    );
  }
}
