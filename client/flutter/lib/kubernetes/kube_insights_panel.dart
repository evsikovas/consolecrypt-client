import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_gitops_graph.dart';
import 'package:consolecrypt/kubernetes/kube_insights_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_resource_details.dart';
import 'package:consolecrypt/kubernetes/kube_write_access.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class KubeInsightsPanel extends ConsumerStatefulWidget {
  const KubeInsightsPanel({
    required this.session,
    required this.contextName,
    required this.service,
    required this.browser,
    required this.problems,
    this.namespace,
    this.onTerminalOpened,
    this.onExplain,
    this.cliPanel,
    super.key,
  });
  final KubeSession session;
  final String contextName;
  final String? namespace;
  final KubeInsightsService service;
  final KubeBrowserService browser;
  final bool problems;
  final VoidCallback? onTerminalOpened;
  final void Function(KubeResourceRef?, String?)? onExplain;
  final Widget? cliPanel;
  @override
  ConsumerState<KubeInsightsPanel> createState() => _KubeInsightsPanelState();
}

class _KubeInsightsPanelState extends ConsumerState<KubeInsightsPanel> {
  late final KubeScope _scope;
  KubeInsights? _data;
  bool _loading = true, _failed = false;
  int _generation = 0;
  String _filter = '', _category = 'all', _controller = 'all', _section = 'all';
  bool _dependencies = false, _flow = false;
  final _busyActions = <String>{};
  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(kubeScopeProvider));

  @override
  void initState() {
    super.initState();
    _scope = ref.read(kubeScopeProvider);
    unawaited(_load());
  }

  @override
  void dispose() {
    _generation++;
    _data = null;
    super.dispose();
  }

  Future<void> _load() async {
    if (!_current) return;
    final generation = ++_generation;
    setState(() {
      _loading = true;
      _failed = false;
    });
    try {
      final data = await widget.service.insights(widget.session.id, namespace: widget.namespace);
      if (_current && generation == _generation) setState(() => _data = data);
    } catch (_) {
      if (_current && generation == _generation) setState(() => _failed = true);
    } finally {
      if (_current && generation == _generation) setState(() => _loading = false);
    }
  }

  Future<void> _open(KubeInsightResource resource) async {
    if (!_current) return;
    final changed = await showAppDialog<bool>(
      context,
      secure: true,
      builder: (_) => KubeResourceDetails(
        session: widget.session,
        contextName: widget.contextName,
        resource: resource.reference,
        service: widget.browser,
        onTerminalOpened: widget.onTerminalOpened,
        onExplain: widget.onExplain,
      ),
    );
    if (_current && changed == true) await _load();
  }

  Future<void> _act(KubeInsightResource resource, String action) async {
    if (!_current ||
        _busyActions.contains(resource.reference.id) ||
        !(ref.read(kubeWriteAccessProvider(widget.session)).value?.canWrite ?? false)) {
      return;
    }
    final reason = await showAppDialog<String>(
      context,
      secure: true,
      barrierDismissible: false,
      builder: (_) => _GitOpsConfirmation(
        resource: resource.resource.name,
        title: _actionLabel(context.l10n, action),
        reasonRequired: action == 'suspend' || action == 'resume',
        help: action == 'reconcile_with_source' ? context.l10n.kubeGitOpsReconcileWithSourceHelp : null,
      ),
    );
    if (!_current || reason == null || !(ref.read(kubeWriteAccessProvider(widget.session)).value?.canWrite ?? false)) {
      return;
    }
    setState(() {
      _busyActions.add(resource.reference.id);
      _failed = false;
    });
    try {
      if (resource.isArgo) {
        await widget.service.argoAction(widget.session.id, resource.reference, switch (action) {
          'sync' => KubeArgoAction.sync,
          'hard_refresh' => KubeArgoAction.hardRefresh,
          _ => KubeArgoAction.refresh,
        });
      } else {
        await widget.service.fluxAction(widget.session.id, resource.reference, switch (action) {
          'reconcile_with_source' => KubeFluxAction.reconcileWithSource,
          'suspend' => KubeFluxAction.suspend,
          'resume' => KubeFluxAction.resume,
          'reset' => KubeFluxAction.resetFailures,
          'force' => KubeFluxAction.forceUpgrade,
          _ => KubeFluxAction.reconcile,
        }, reason: reason.isEmpty ? null : reason);
      }
      if (_current) await _load();
    } catch (_) {
      if (_current) setState(() => _failed = true);
    } finally {
      if (_current) setState(() => _busyActions.remove(resource.reference.id));
    }
  }

  KubeInsightResource? _find(KubeObjectIdentity identity) => _data?.resources
      .where(
        (entry) =>
            entry.api.kind == identity.kind &&
            entry.resource.name == identity.name &&
            (entry.resource.namespace ?? '') == identity.namespace,
      )
      .firstOrNull;

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    if (!_current) return const SizedBox.shrink();
    final data = _data;
    final canWrite = ref.watch(kubeWriteAccessProvider(widget.session)).value?.canWrite ?? false;
    return LayoutBuilder(
      builder: (context, bounds) => Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          ConstrainedBox(
            constraints: BoxConstraints(maxHeight: bounds.maxHeight * .58),
            child: SingleChildScrollView(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  if (widget.cliPanel != null)
                    ConstrainedBox(
                      constraints: const BoxConstraints(maxHeight: 240),
                      child: SingleChildScrollView(
                        child: ExpansionTile(
                          title: const Text(/* l10n-ignore: Official tool/controller product name. */ 'Flux CLI'),
                          children: [widget.cliPanel!],
                        ), // l10n-ignore: Official tool/controller product name.
                      ),
                    ), // l10n-ignore: tool name
                  Row(
                    children: [
                      Expanded(
                        child: TextField(
                          key: const ValueKey('kube-insights-search'),
                          decoration: InputDecoration(labelText: l.commonSearch, prefixIcon: const Icon(Icons.search)),
                          onChanged: (value) => setState(() => _filter = value.toLowerCase()),
                        ),
                      ),
                      GlassIconButton(
                        key: const ValueKey('kube-insights-refresh'),
                        tooltip: l.kubeRefreshResources,
                        icon: Icons.refresh,
                        onPressed: _loading ? null : _load,
                      ),
                    ],
                  ),
                  if (_loading) const LinearProgressIndicator(),
                  if (_failed) Text(l.kubeRequestFailed, style: TextStyle(color: Theme.of(context).colorScheme.error)),
                  if (data?.partial == true)
                    Padding(padding: const EdgeInsets.symmetric(vertical: 8), child: Text(l.kubeInsightsPartial)),
                  if (data != null) ...[
                    if (!widget.problems) ...[
                      SingleChildScrollView(
                        scrollDirection: Axis.horizontal,
                        child: Row(
                          children: [
                            if (data.resources.any((r) => r.isFlux) && data.resources.any((r) => r.isArgo))
                              for (final controller in [
                                ('all', l.kubeGitOpsAll),
                                ('flux', 'Flux'),
                                ('argo', 'Argo CD'), // l10n-ignore: Official tool/controller product name.
                              ])
                                Padding(
                                  padding: const EdgeInsets.only(right: 6),
                                  child: ChoiceChip(
                                    label: Text(controller.$2),
                                    selected: _controller == controller.$1,
                                    onSelected: (_) => setState(() => _controller = controller.$1),
                                  ),
                                ),
                            for (final section in [
                              ('all', l.kubeOverview),
                              ('sources', l.kubeGitOpsSources),
                              ('kustomization', 'Kustomizations'),
                              ('helmrelease', 'HelmReleases'),
                              ('notifications', l.kubeGitOpsNotifications),
                              ('images', l.kubeGitOpsImages),
                            ])
                              Padding(
                                padding: const EdgeInsets.only(right: 6),
                                child: ChoiceChip(
                                  label: Text(section.$2),
                                  selected: _section == section.$1,
                                  onSelected: (_) => setState(() {
                                    _section = section.$1;
                                    _flow = false;
                                    _dependencies = false;
                                  }),
                                ),
                              ),
                          ],
                        ),
                      ),
                      if (_section == 'all') _recentReconciles(data),
                      SingleChildScrollView(
                        scrollDirection: Axis.horizontal,
                        child: Row(
                          children: [
                            for (final category in [
                              ('all', l.kubeGitOpsAll),
                              ('ready', '${l.kubeGitOpsReady} ${data.gitops.ready}'),
                              ('attention', '${l.kubeGitOpsAttention} ${data.gitops.notReady + data.gitops.stalled}'),
                              ('suspended', '${l.kubeGitOpsSuspended} ${data.gitops.suspended}'),
                            ])
                              Padding(
                                padding: const EdgeInsets.only(right: 8),
                                child: ChoiceChip(
                                  label: Text(category.$2),
                                  selected: _category == category.$1,
                                  onSelected: (_) => setState(() {
                                    _category = category.$1;
                                    _dependencies = false;
                                    _flow = false;
                                  }),
                                ),
                              ),
                            FilterChip(
                              key: const ValueKey('kube-gitops-flow-chip'),
                              label: Text(l.kubeGitOpsFlow),
                              selected: _flow,
                              onSelected: (value) => setState(() {
                                _flow = value;
                                _dependencies = false;
                              }),
                            ),
                            FilterChip(
                              key: const ValueKey('kube-gitops-dependencies'),
                              label: Text(l.kubeDependencies),
                              selected: _dependencies,
                              onSelected: (value) => setState(() {
                                _dependencies = value;
                                _flow = false;
                              }),
                            ),
                          ],
                        ),
                      ),
                      const SizedBox(height: 8),
                    ],
                  ],
                ],
              ),
            ),
          ),
          if (data != null)
            Expanded(
              child: widget.problems
                  ? _problems(data)
                  : _flow
                  ? KubeGitOpsGraph(
                      flow: data.flow,
                      canOpen: (id) => _find(id) != null,
                      open: (id) {
                        final resource = _find(id);
                        if (resource != null) unawaited(_open(resource));
                      },
                    )
                  : _dependencies
                  ? _dependencyView(data)
                  : _resources(data, canWrite),
            ),
        ],
      ),
    );
  }

  Widget _recentReconciles(KubeInsights data) {
    final entries = <({DateTime time, String revision, KubeInsightResource resource})>[];
    for (final resource in data.resources.where((r) => r.isFlux || r.isArgo)) {
      final status = resource.object['status'] as Map? ?? const {};
      final times = <DateTime>[];
      for (final condition in (status['conditions'] as List? ?? const []).whereType<Map<String, dynamic>>()) {
        final time = DateTime.tryParse(condition['lastTransitionTime']?.toString() ?? '');
        if (time != null) times.add(time);
      }
      final operation = status['operationState'] as Map?;
      final finished = DateTime.tryParse(operation?['finishedAt']?.toString() ?? '');
      if (finished != null) times.add(finished);
      if (times.isEmpty) continue;
      times.sort();
      final revision =
          status['lastAppliedRevision'] ??
          (status['artifact'] as Map?)?['revision'] ??
          (status['sync'] as Map?)?['revision'];
      entries.add((time: times.last, revision: revision is String ? revision : '', resource: resource));
    }
    entries.sort((a, b) => b.time.compareTo(a.time));
    return ConstrainedBox(
      constraints: const BoxConstraints(maxHeight: 220),
      child: SingleChildScrollView(
        child: ExpansionTile(
          title: Text(context.l10n.kubeGitOpsRecent),
          children: [
            if (entries.isEmpty) Text(context.l10n.kubeGitOpsHistoryEmpty),
            for (final entry in entries.take(20))
              ListTile(
                leading: Icon(_ready(entry.resource) ? Icons.check_circle_outline : Icons.error_outline),
                title: Text('${entry.resource.resource.namespace}/${entry.resource.resource.name}'),
                subtitle: Text('${entry.time.toLocal()} · ${entry.revision}'),
                onTap: () => _open(entry.resource),
              ),
          ],
        ),
      ),
    );
  }

  Widget _problems(KubeInsights data) {
    final l = context.l10n;
    final issues =
        data.issues
            .where(
              (issue) => '${issue.name} ${issue.namespace ?? ''} ${issue.kind} ${_issueLabel(l, issue.code)}'
                  .toLowerCase()
                  .contains(_filter),
            )
            .toList()
          ..sort((a, b) {
            int rank(String value) => switch (value) {
              'error' => 0,
              'warning' => 1,
              _ => 2,
            };
            final order = rank(a.severity).compareTo(rank(b.severity));
            return order == 0 ? a.name.compareTo(b.name) : order;
          });
    if (issues.isEmpty) {
      return Padding(
        padding: const EdgeInsets.all(12),
        child: Text(data.partial ? l.kubeNoVisibleProblems : l.kubeNoProblems),
      );
    }
    return ListView.builder(
      key: const ValueKey('kube-problems-list'),
      itemCount: issues.length,
      itemBuilder: (context, index) {
        final issue = issues[index];
        final resource = _find(
          KubeObjectIdentity(kind: issue.kind, namespace: issue.namespace ?? '', name: issue.name),
        );
        return ContentSurface(
          child: ListTile(
            leading: Icon(issue.severity == 'error' ? Icons.error_outline : Icons.warning_amber_outlined),
            title: Text(
              /* l10n-ignore: Localized issue/status labels, resource identifiers and numeric counts. */ '${_issueLabel(l, issue.code)}${issue.count > 1 ? ' ×${issue.count}' : ''}',
            ), // l10n-ignore: Localized issue/status labels, resource identifiers and numeric counts.
            subtitle: Text(
              [
                issue.owner?.label ?? '${issue.kind} · ${issue.namespace ?? ''}/${issue.name}',
                if (issue.exitCode != null) '${l.kubeExitCode}: ${issue.exitCode}',
                if (issue.subject != null) issue.subject!,
              ].join('\n'),
            ),
            trailing: widget.onExplain == null
                ? null
                : GlassIconButton(
                    tooltip: l.kubeAiExplain,
                    icon: Icons.auto_awesome,
                    onPressed: () => widget.onExplain?.call(
                      resource?.reference,
                      l.kubeAiExplainQuestion(
                        _issueLabel(l, issue.code),
                        '${issue.kind} ${issue.namespace ?? ''}/${issue.name}',
                      ),
                    ),
                  ),
            onTap: resource == null ? null : () => _open(resource),
          ),
        );
      },
    );
  }

  Widget _resources(KubeInsights data, bool canWrite) {
    final l = context.l10n;
    final resources = data.resources.where((resource) {
      if (!resource.isFlux && !resource.isArgo) return false;
      if ((_controller == 'flux' && !resource.isFlux) || (_controller == 'argo' && !resource.isArgo)) return false;
      final group = resource.api.group, kind = resource.api.kind;
      if (!switch (_section) {
        'sources' => group == 'source.toolkit.fluxcd.io',
        'kustomization' => kind == 'Kustomization',
        'helmrelease' => kind == 'HelmRelease',
        'notifications' => group == 'notification.toolkit.fluxcd.io',
        'images' => group == 'image.toolkit.fluxcd.io',
        _ => true,
      }) {
        return false;
      }
      if (!'${resource.api.kind} ${resource.resource.namespace ?? ''} ${resource.resource.name}'.toLowerCase().contains(
        _filter,
      )) {
        return false;
      }
      final ready = _ready(resource);
      return switch (_category) {
        'ready' => ready && !resource.suspended,
        'attention' => !ready && !resource.suspended,
        'suspended' => resource.suspended,
        _ => true,
      };
    }).toList()..sort((a, b) => a.resource.name.compareTo(b.resource.name));
    if (resources.isEmpty) return Padding(padding: const EdgeInsets.all(12), child: Text(l.kubeNoGitOpsResources));
    return ListView.builder(
      key: const ValueKey('kube-gitops-resources'),
      itemCount: resources.length,
      itemBuilder: (context, index) {
        final resource = resources[index];
        final status = resource.suspended
            ? l.kubeGitOpsSuspended
            : _ready(resource)
            ? l.kubeGitOpsReady
            : l.kubeGitOpsAttention;
        final actions = _actions(resource);
        return ContentSurface(
          child: ListTile(
            key: ValueKey('kube-gitops-${resource.resource.id}'),
            leading: Icon(
              resource.suspended
                  ? Icons.pause_circle_outline
                  : _ready(resource)
                  ? Icons.check_circle_outline
                  : Icons.error_outline,
            ),
            title: Text(resource.resource.name),
            subtitle: Text(
              /* l10n-ignore: Localized issue/status labels, resource identifiers and numeric counts. */ '${resource.api.kind} · ${resource.resource.namespace ?? ''} · $status',
            ), // l10n-ignore: Localized issue/status labels, resource identifiers and numeric counts.
            onTap: () => _open(resource),
            trailing: actions.isEmpty
                ? null
                : PopupMenuButton<String>(
                    key: ValueKey('kube-gitops-action-${resource.resource.id}'),
                    enabled: canWrite && !_busyActions.contains(resource.reference.id),
                    tooltip: l.kubeGitOpsActions,
                    itemBuilder: (_) => actions
                        .map((action) => PopupMenuItem(value: action, child: Text(_actionLabel(l, action))))
                        .toList(),
                    onSelected: (action) => unawaited(_act(resource, action)),
                  ),
          ),
        );
      },
    );
  }

  Widget _dependencyView(KubeInsights data) {
    final l = context.l10n;
    final edges = data.gitops.dependencies
        .where((edge) => '${edge.from.label} ${edge.to.label}'.toLowerCase().contains(_filter))
        .toList();
    return ListView.builder(
      key: const ValueKey('kube-dependency-list'),
      itemCount: edges.length + (data.gitops.cycles.isEmpty ? 0 : 1),
      itemBuilder: (context, index) {
        if (data.gitops.cycles.isNotEmpty && index == 0) {
          return ContentSurface(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(l.kubeDependencyCycle, style: TextStyle(color: Theme.of(context).colorScheme.error)),
                ...data.gitops.cycles.take(100).map((identity) => Text(identity.label)),
              ],
            ),
          );
        }
        final edge = edges[index - (data.gitops.cycles.isEmpty ? 0 : 1)];
        final source = _find(edge.from), target = _find(edge.to);
        return ContentSurface(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              TextButton(onPressed: source == null ? null : () => _open(source), child: Text(edge.from.label)),
              Row(
                children: [
                  Icon(edge.missing ? Icons.link_off : Icons.arrow_downward),
                  const SizedBox(width: 8),
                  Expanded(
                    child: Text(
                      edge.missing
                          ? l.kubeDependencyMissing
                          : edge.ready
                          ? l.kubeDependencyReady
                          : l.kubeDependencyWaiting,
                    ),
                  ),
                ],
              ),
              TextButton(onPressed: target == null ? null : () => _open(target), child: Text(edge.to.label)),
            ],
          ),
        );
      },
    );
  }
}

bool _ready(KubeInsightResource resource) {
  final status = resource.object['status'] as Map? ?? const {};
  if (resource.isArgo) {
    return (status['health'] as Map?)?['status'] == 'Healthy' && (status['sync'] as Map?)?['status'] == 'Synced';
  }
  return (status['conditions'] as List? ?? const []).whereType<Map<String, dynamic>>().any(
    (condition) => condition['type'] == 'Ready' && condition['status'] == 'True',
  );
}

List<String> _actions(KubeInsightResource resource) {
  if (resource.isArgo) return ['refresh', 'hard_refresh', 'sync'];
  if (!resource.isFlux || resource.api.group == 'notification.toolkit.fluxcd.io') return [];
  return [
    'reconcile',
    if (const ['Kustomization', 'HelmRelease', 'HelmChart'].contains(resource.api.kind)) 'reconcile_with_source',
    if (resource.api.kind != 'ImagePolicy') resource.suspended ? 'resume' : 'suspend',
    if (resource.api.kind == 'HelmRelease') ...['reset', 'force'],
  ];
}

String _actionLabel(AppLocalizations l, String action) => switch (action) {
  'reconcile_with_source' => l.kubeGitOpsReconcileWithSource,
  'suspend' => l.kubeGitOpsSuspend,
  'resume' => l.kubeGitOpsResume,
  'reset' => l.kubeGitOpsReset,
  'force' => l.kubeGitOpsForce,
  'sync' => l.kubeArgoSync,
  'refresh' => l.kubeArgoRefresh,
  'hard_refresh' => l.kubeArgoHardRefresh,
  _ => l.kubeGitOpsReconcile,
};

String _issueLabel(AppLocalizations l, String code) => switch (code) {
  'certificate_expiring' => l.kubeIssueCertificateExpiring,
  'certificate_expired' => l.kubeIssueCertificateExpired,
  'pod_evicted' => l.kubeIssueEvicted,
  'pod_pending_resources' => l.kubeIssuePendingResources,
  'pod_pending_taints' => l.kubeIssuePendingTaints,
  'pod_pending_volume' => l.kubeIssuePendingVolume,
  'flux_suspended_long' => l.kubeIssueSuspendedLong,
  'flux_source_stale' => l.kubeIssueSourceStale,
  'flux_helm_drift' => l.kubeIssueHelmDrift,
  'flux_dependency_not_ready' => l.kubeIssueDependencyNotReady,
  'flux_dependency_missing' => l.kubeIssueDependencyMissing,
  'pod_crash_loop' => l.kubeIssueCrashLoop,
  'pod_image_pull' => l.kubeIssueImagePull,
  'pod_container_config' => l.kubeIssueContainerConfig,
  'pod_oom_killed' => l.kubeIssueOom,
  'pod_restarts' => l.kubeIssueRestarts,
  'pod_unschedulable' => l.kubeIssueScheduling,
  'pod_not_ready' => l.kubeIssuePodNotReady,
  'node_not_ready' => l.kubeIssueNodeNotReady,
  'node_pressure' => l.kubeIssueNodePressure,
  'rollout_deadline' => l.kubeIssueRolloutDeadline,
  'workload_replica_failure' => l.kubeIssueReplicaFailure,
  'workload_replicas_unavailable' => l.kubeIssueReplicasUnavailable,
  'volume_claim_pending' => l.kubeIssueVolumePending,
  'volume_claim_lost' => l.kubeIssueVolumeLost,
  'job_failed' => l.kubeIssueJobFailed,
  'argo_degraded' => l.kubeIssueArgoDegraded,
  'argo_out_of_sync' => l.kubeIssueArgoOutOfSync,
  'flux_generation_pending' => l.kubeIssueFluxPending,
  'flux_not_ready' => l.kubeIssueFluxNotReady,
  'flux_stalled' => l.kubeIssueFluxStalled,
  'flux_suspended' => l.kubeIssueFluxSuspended,
  _ => l.kubeIssueCondition,
};

class _GitOpsConfirmation extends ConsumerStatefulWidget {
  const _GitOpsConfirmation({required this.resource, required this.title, required this.reasonRequired, this.help});
  final String resource, title;
  final String? help;
  final bool reasonRequired;
  @override
  ConsumerState<_GitOpsConfirmation> createState() => _GitOpsConfirmationState();
}

class _GitOpsConfirmationState extends ConsumerState<_GitOpsConfirmation> {
  final _name = TextEditingController(), _reason = TextEditingController();
  late final KubeScope _scope;
  @override
  void initState() {
    super.initState();
    _scope = ref.read(kubeScopeProvider);
  }

  @override
  void dispose() {
    _name.clear();
    _reason.clear();
    _name.dispose();
    _reason.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(kubeScopeProvider, (_, next) {
      if (identical(next, _scope)) return;
      _name.clear();
      _reason.clear();
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted) return;
        final route = ModalRoute.of(context);
        if (route?.isActive == true) Navigator.of(context).removeRoute(route!);
      });
    });
    final l = context.l10n;
    return GlassDialog(
      key: const ValueKey('kube-gitops-confirm'),
      title: widget.title,
      content: SingleChildScrollView(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(widget.help ?? l.kubeGitOpsChangeHelp),
            const SizedBox(height: 12),
            SelectableText(widget.resource),
            TextField(
              key: const ValueKey('kube-gitops-confirm-name'),
              controller: _name,
              decoration: InputDecoration(labelText: l.kubeTypeName),
              autocorrect: false,
              enableSuggestions: false,
              onChanged: (_) => setState(() {}),
            ),
            if (widget.reasonRequired)
              TextField(
                key: const ValueKey('kube-gitops-reason'),
                controller: _reason,
                decoration: InputDecoration(labelText: l.kubeGitOpsReason),
                maxLength: 1024,
                maxLines: 2,
                onChanged: (_) => setState(() {}),
              ),
          ],
        ),
      ),
      primaryAction: GlassButton.destructive(
        key: const ValueKey('kube-gitops-confirm-action'),
        label: widget.title,
        onPressed:
            _name.text == widget.resource &&
                widget.resource.isNotEmpty &&
                (!widget.reasonRequired ||
                    (_reason.text.trim().isNotEmpty && !RegExp(r'[\x00-\x1f\x7f]').hasMatch(_reason.text)))
            ? () => closeDialog(context, _reason.text.trim())
            : null,
      ),
      secondaryActions: [GlassButton(label: l.commonCancel, onPressed: () => closeDialog<String>(context))],
    );
  }
}
