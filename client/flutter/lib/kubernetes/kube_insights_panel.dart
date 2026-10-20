import 'dart:async';

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_gitops_actions.dart';
import 'package:consolecrypt/kubernetes/kube_gitops_graph.dart';
import 'package:consolecrypt/kubernetes/kube_gitops_monitor.dart';
import 'package:consolecrypt/kubernetes/kube_gitops_timeline.dart';
import 'package:consolecrypt/kubernetes/kube_insights_service.dart';
import 'package:consolecrypt/kubernetes/kube_operations_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_resource_details.dart';
import 'package:consolecrypt/kubernetes/kube_resource_route.dart';
import 'package:consolecrypt/kubernetes/kube_resource_table_view.dart';
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
    this.monitor,
    this.initialAction,
    this.initialResource,
    this.onInitialActionHandled,
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
  final KubeGitOpsMonitor? monitor;
  final String? initialAction;
  final KubeResourceRef? initialResource;
  final VoidCallback? onInitialActionHandled;
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
  final _selected = <String>{};
  bool _bulkBusy = false, _bulkCancel = false;
  bool _initialHandled = false;
  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(kubeScopeProvider));

  @override
  void initState() {
    super.initState();
    _scope = ref.read(kubeScopeProvider);
    final monitor = widget.monitor;
    if (monitor == null) {
      unawaited(_load());
    } else {
      monitor.addListener(_monitorChanged);
      scheduleMicrotask(() {
        if (!_current) return;
        monitor.show();
        _monitorChanged();
      });
    }
  }

  @override
  void dispose() {
    widget.monitor?.removeListener(_monitorChanged);
    widget.monitor?.hide();
    _generation++;
    _data = null;
    super.dispose();
  }

  Future<void> _load() async {
    if (!_current) return;
    if (widget.monitor != null) {
      await widget.monitor!.refresh();
      return;
    }
    final generation = ++_generation;
    setState(() {
      _loading = true;
      _failed = false;
    });
    try {
      final data = await widget.service.insights(widget.session.id, namespace: widget.namespace);
      if (_current && generation == _generation) {
        setState(() => _data = data);
        _handleInitial();
      }
    } catch (_) {
      if (_current && generation == _generation) setState(() => _failed = true);
    } finally {
      if (_current && generation == _generation) setState(() => _loading = false);
    }
  }

  void _monitorChanged() {
    if (!_current) return;
    final monitor = widget.monitor!;
    setState(() {
      _data = monitor.data;
      _loading = monitor.loading;
      _failed = monitor.failed;
      if (_data != null) {
        final keys = _data!.resources.map(_selectionKey).toSet();
        _selected.removeWhere((key) => !keys.contains(key));
      }
    });
    _handleInitial();
  }

  void _handleInitial() {
    if (!_current || _initialHandled || widget.initialAction == null || _data == null) return;
    _initialHandled = true;
    scheduleMicrotask(() {
      if (!_current) return;
      widget.onInitialActionHandled?.call();
      switch (widget.initialAction) {
        case 'flow':
          setState(() => _flow = true);
        case 'suspended':
          setState(() => _category = 'suspended');
        case 'reconcile':
          final resource = _data?.resources
              .where((r) => r.reference.id == widget.initialResource?.id && r.isFlux)
              .firstOrNull;
          if (resource != null) {
            unawaited(_act(resource, 'reconcile'));
          } else {
            setState(() => _failed = true);
          }
      }
    });
  }

  Future<void> _open(KubeInsightResource resource, {bool controllerLogs = false}) async {
    if (!_current) return;
    final changed = await showKubeResourceDetails<bool>(
      context,
      builder: (_) => KubeResourceDetails(
        session: widget.session,
        contextName: widget.contextName,
        resource: resource.reference,
        service: widget.browser,
        gitOpsMonitor: widget.monitor,
        initialControllerLogs: controllerLogs,
        onTerminalOpened: widget.onTerminalOpened,
        onExplain: widget.onExplain,
      ),
    );
    if (_current && changed == true) await _load();
  }

  Future<void> _act(KubeInsightResource resource, String action) async {
    if (!_current ||
        _bulkBusy ||
        _busyActions.contains(resource.reference.id) ||
        !(ref.read(kubeWriteAccessProvider(widget.session)).value?.canWrite ?? false)) {
      return;
    }
    setState(() {
      _busyActions.add(resource.reference.id);
      _failed = false;
    });
    try {
      final changed = await runKubeGitOpsAction(
        context,
        ref,
        session: widget.session,
        resource: resource,
        service: widget.service,
        action: action,
        monitor: widget.monitor,
      );
      if (_current && changed) await _load();
    } catch (_) {
      if (_current) setState(() => _failed = true);
    } finally {
      if (_current) setState(() => _busyActions.remove(resource.reference.id));
    }
  }

  static String _selectionKey(KubeInsightResource resource) => '${resource.reference.id}/${resource.resource.uid}';

  Future<void> _bulk(String action) async {
    if (!_current || _bulkBusy || widget.session.production) return;
    final resources = _data?.resources.where((r) => _selected.contains(_selectionKey(r))).toList() ?? [];
    setState(() {
      _bulkBusy = true;
      _bulkCancel = false;
      _failed = false;
    });
    try {
      await runKubeGitOpsBulkAction(
        context,
        ref,
        session: widget.session,
        contextName: widget.contextName,
        resources: resources,
        service: widget.service,
        action: action,
        shouldContinue: () => _current && !_bulkCancel,
        onCompleted: (resource) {
          if (_current) setState(() => _selected.remove(_selectionKey(resource)));
        },
      );
      if (_current) await _load();
    } catch (_) {
      if (_current) setState(() => _failed = true);
    } finally {
      if (_current) setState(() => _bulkBusy = false);
    }
  }

  KubeInsightResource? _find(KubeObjectIdentity identity) => _data?.resources
      .where(
        (entry) =>
            entry.api.kind == identity.kind &&
            entry.api.group == identity.group &&
            entry.resource.name == identity.name &&
            (entry.resource.namespace ?? '') == identity.namespace,
      )
      .firstOrNull;

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    if (!_current) return const SizedBox.shrink();
    final data = _data;
    final canWrite =
        !(AppPlatform.isMobile && widget.session.production) &&
        (ref.watch(kubeWriteAccessProvider(widget.session)).value?.canWrite ?? false);
    final scoped = data == null ? <KubeInsightResource>[] : _scopedResources(data).toList();
    final ready = scoped.where((r) => _ready(r) && !r.suspended).length;
    final attention = scoped.where((r) => !_ready(r) && !r.suspended).length;
    final suspended = scoped.where((r) => r.suspended).length;
    final stalled = scoped.where(kubeGitOpsStalled).length;
    final stale = scoped.where((r) => _issuesFor(data!, r).any((i) => i.code == 'flux_source_stale')).length;
    return LayoutBuilder(
      builder: (context, bounds) => Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          ConstrainedBox(
            constraints: BoxConstraints(maxHeight: bounds.maxHeight * (AppPlatform.isMobile ? .45 : .58)),
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
                  if (widget.monitor case final monitor?) _monitorSettings(monitor),
                  if (_failed) Text(l.kubeRequestFailed, style: TextStyle(color: Theme.of(context).colorScheme.error)),
                  if (data?.partial == true)
                    Padding(padding: const EdgeInsets.symmetric(vertical: 8), child: Text(l.kubeInsightsPartial)),
                  if (data != null) ...[
                    if (!widget.problems) ...[
                      if (_selected.isNotEmpty)
                        Wrap(
                          spacing: 8,
                          crossAxisAlignment: WrapCrossAlignment.center,
                          children: [
                            Text('${l.kubeBulkActions}: ${_selected.length}'),
                            if (_bulkBusy)
                              GlassButton(label: l.commonCancel, onPressed: () => setState(() => _bulkCancel = true))
                            else
                              for (final action in ['reconcile', 'suspend', 'resume'])
                                GlassButton(
                                  key: ValueKey('kube-gitops-bulk-$action'),
                                  label: kubeGitOpsActionLabel(l, action),
                                  onPressed:
                                      canWrite &&
                                          !widget.session.production &&
                                          data.resources
                                              .where((r) => _selected.contains(_selectionKey(r)))
                                              .every((r) => kubeGitOpsActions(r).contains(action))
                                      ? () => _bulk(action)
                                      : null,
                                ),
                          ],
                        ),
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
                      if (_section == 'all')
                        KubeGitOpsTimeline(
                          session: widget.session,
                          resources: data.resources,
                          service: widget.browser,
                          namespace: widget.namespace,
                          onOpen: (resource) => unawaited(_open(resource)),
                        ),
                      SingleChildScrollView(
                        scrollDirection: Axis.horizontal,
                        child: Row(
                          children: [
                            for (final category in [
                              ('all', l.kubeGitOpsAll),
                              ('ready', '${l.kubeGitOpsReady} $ready'),
                              ('attention', '${l.kubeGitOpsAttention} $attention'),
                              ('suspended', '${l.kubeGitOpsSuspended} $suspended'),
                              ('stalled', '${l.kubeGitOpsStalled} $stalled'),
                              ('stale', '${l.kubeGitOpsStale} $stale'),
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
                      resources: data.resources,
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

  Widget _monitorSettings(KubeGitOpsMonitor monitor) {
    final l = context.l10n;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Wrap(
          spacing: 8,
          crossAxisAlignment: WrapCrossAlignment.center,
          children: [
            if (monitor.live) Chip(label: Text(l.kubeLive), avatar: const Icon(Icons.sync, size: 16)),
            if (monitor.watchPartial) Text(l.kubeInsightsPartial),
            if (monitor.alertCount > 0)
              ActionChip(
                key: const ValueKey('kube-gitops-alerts'),
                label: Text(l.kubeGitOpsUnread(monitor.alertCount)),
                avatar: const Icon(Icons.notifications_active_outlined),
                onPressed: monitor.acknowledge,
              ),
          ],
        ),
        if (monitor.preferences != null)
          ExpansionTile(
            title: Text(l.kubeGitOpsLocalAlerts),
            children: [
              SwitchListTile(
                key: const ValueKey('kube-gitops-notifications'),
                value: monitor.notifications,
                title: Text(l.kubeGitOpsNotify),
                subtitle: Text(l.kubeGitOpsNotifyHelp),
                onChanged: (value) => unawaited(monitor.setNotifications(value)),
              ),
              if (monitor.preferencesFailed) Text(l.kubePreferencesFailed),
              for (final reminder in monitor.reminders.values)
                ListTile(
                  title: Text(
                    /* l10n-ignore: Kubernetes namespace and resource name. */ '${reminder.resource.namespace ?? ''}/${reminder.resource.name}',
                  ),
                  subtitle: Text(reminder.dueAt.toLocal().toString()),
                  trailing: GlassIconButton(
                    icon: Icons.alarm_off,
                    tooltip: l.kubeGitOpsCancelReminder,
                    onPressed: () => unawaited(monitor.cancelReminder(reminder.resource.id)),
                  ),
                ),
            ],
          ),
      ],
    );
  }

  Widget _problems(KubeInsights data) {
    final l = context.l10n;
    final issues =
        data.issues
            .where((issue) => widget.namespace == null || issue.namespace == widget.namespace)
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
        final matches = data.resources
            .where(
              (r) =>
                  r.api.kind == issue.kind && r.resource.name == issue.name && r.resource.namespace == issue.namespace,
            )
            .take(2)
            .toList();
        // Older issue DTOs lack an API group. Ambiguous kinds must not open an
        // arbitrary group's object.
        final resource = matches.length == 1 ? matches.single : null;
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

  Iterable<KubeInsightResource> _scopedResources(KubeInsights data) => data.resources.where((resource) {
    if (widget.namespace != null && resource.resource.namespace != widget.namespace) return false;
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
    return true;
  });

  Widget _resources(KubeInsights data, bool canWrite) {
    final l = context.l10n;
    final resources =
        _scopedResources(data).where((resource) {
          if (!'${resource.api.kind} ${resource.resource.namespace ?? ''} ${resource.resource.name}'
              .toLowerCase()
              .contains(_filter)) {
            return false;
          }
          final ready = _ready(resource);
          return switch (_category) {
            'ready' => ready && !resource.suspended,
            'attention' => !ready && !resource.suspended,
            'suspended' => resource.suspended,
            'stalled' => kubeGitOpsStalled(resource),
            'stale' => _issuesFor(data, resource).any((i) => i.code == 'flux_source_stale'),
            _ => true,
          };
        }).toList()..sort((a, b) {
          if (_section == 'all') {
            int rank(KubeInsightResource r) => _issuesFor(data, r).isNotEmpty || (!_ready(r) && !r.suspended) ? 0 : 1;
            final attention = rank(a).compareTo(rank(b));
            if (attention != 0) return attention;
          }
          return a.resource.name.compareTo(b.resource.name);
        });
    if (resources.isEmpty) return Padding(padding: const EdgeInsets.all(12), child: Text(l.kubeNoGitOpsResources));
    if (_section != 'all' && MediaQuery.sizeOf(context).width >= 900) {
      return _table(resources, canWrite);
    }
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
        final actions = kubeGitOpsActions(resource);
        final tile = ContentSurface(
          child: ListTile(
            key: ValueKey('kube-gitops-${resource.resource.id}'),
            leading: !widget.session.production && resource.isFlux && actions.isNotEmpty
                ? Checkbox(
                    key: ValueKey('kube-gitops-select-${resource.resource.id}'),
                    value: _selected.contains(_selectionKey(resource)),
                    onChanged: _bulkBusy
                        ? null
                        : (value) => setState(() {
                            if (value == true && _selected.length < 100) {
                              _selected.add(_selectionKey(resource));
                            } else {
                              _selected.remove(_selectionKey(resource));
                            }
                          }),
                  )
                : Icon(
                    resource.suspended
                        ? Icons.pause_circle_outline
                        : _ready(resource)
                        ? Icons.check_circle_outline
                        : Icons.error_outline,
                  ),
            title: Text(resource.resource.name),
            subtitle: Text(
              /* l10n-ignore: Localized issue/status labels, resource identifiers and numeric counts. */ '${resource.api.kind} · ${resource.resource.namespace ?? ''} · $status'
              /* l10n-ignore: Already localized diagnostics for the same resource. */ '${_issuesFor(data, resource).isEmpty ? '' : '\n${_issuesFor(data, resource).take(2).map((i) => _issueLabel(l, i.code)).join(' · ')}'}',
            ), // l10n-ignore: Localized issue/status labels, resource identifiers and numeric counts.
            onTap: () => _open(resource),
            trailing: actions.isEmpty
                ? null
                : PopupMenuButton<String>(
                    key: ValueKey('kube-gitops-action-${resource.resource.id}'),
                    enabled: canWrite && !_bulkBusy && !_busyActions.contains(resource.reference.id),
                    tooltip: l.kubeGitOpsActions,
                    itemBuilder: (_) => actions
                        .map((action) => PopupMenuItem(value: action, child: Text(kubeGitOpsActionLabel(l, action))))
                        .toList(),
                    onSelected: (action) => unawaited(_act(resource, action)),
                  ),
          ),
        );
        if (!AppPlatform.isMobile || !resource.isFlux) return tile;
        final reconcile =
            canWrite && actions.contains('reconcile') && !_bulkBusy && !_busyActions.contains(resource.reference.id);
        Widget swipeBackground(String label, IconData icon, Alignment alignment) => ColoredBox(
          color: Theme.of(context).colorScheme.secondaryContainer,
          child: Align(
            alignment: alignment,
            child: Padding(
              padding: const EdgeInsets.all(16),
              child: Row(mainAxisSize: MainAxisSize.min, children: [Icon(icon), const SizedBox(width: 8), Text(label)]),
            ),
          ),
        );
        return Dismissible(
          key: ValueKey('kube-gitops-swipe-${resource.reference.id}'),
          direction: reconcile ? DismissDirection.horizontal : DismissDirection.endToStart,
          background: swipeBackground(
            reconcile ? l.kubeGitOpsReconcile : l.kubeGitOpsControllerLogs,
            reconcile ? Icons.sync : Icons.article_outlined,
            Alignment.centerLeft,
          ),
          secondaryBackground: swipeBackground(
            l.kubeGitOpsControllerLogs,
            Icons.article_outlined,
            Alignment.centerRight,
          ),
          confirmDismiss: (direction) async {
            // Swiping opens the same explicit action review; it never removes a
            // row or submits a cluster mutation by itself.
            if (!_current) return false;
            if (direction == DismissDirection.startToEnd && reconcile) {
              await _act(resource, 'reconcile');
            } else {
              await _open(resource, controllerLogs: true);
            }
            return false;
          },
          child: tile,
        );
      },
    );
  }

  Widget _table(List<KubeInsightResource> resources, bool canWrite) {
    final l = context.l10n;
    final rows = <KubeResourceRow>[];
    final byId = <String, KubeInsightResource>{};
    for (final resource in resources) {
      final object = resource.object;
      final spec = object['spec'] as Map? ?? const {}, status = object['status'] as Map? ?? const {};
      final source = spec['sourceRef'] as Map? ?? spec['chartRef'] as Map? ?? const {};
      final chart = (spec['chart'] as Map?)?['spec'] as Map?;
      final revision =
          status['lastAppliedRevision'] ??
          (status['artifact'] as Map?)?['revision'] ??
          (status['sync'] as Map?)?['revision'];
      final times =
          ((status['conditions'] as List?) ?? const [])
              .whereType<Map<String, dynamic>>()
              .map((c) => DateTime.tryParse(c['lastTransitionTime']?.toString() ?? ''))
              .whereType<DateTime>()
              .toList()
            ..sort();
      final row = KubeResourceRow(
        name: resource.resource.name,
        namespace: resource.resource.namespace,
        uid: resource.resource.uid,
        resourceVersion: resource.resource.resourceVersion,
        createdAt: resource.resource.createdAt,
        status: resource.suspended
            ? l.kubeGitOpsSuspended
            : _ready(resource)
            ? l.kubeGitOpsReady
            : l.kubeGitOpsAttention,
        cells: [
          source['name']?.toString() ?? chart?['chart']?.toString() ?? '—',
          revision?.toString() ?? '—',
          times.lastOrNull?.toLocal().toString() ?? '—',
          spec['interval']?.toString() ?? '—',
          resource.suspended ? l.kubeGitOpsSuspended : '—',
        ],
      );
      rows.add(row);
      byId[row.id] = resource;
    }
    return KubeResourceTableView(
      key: ValueKey('kube-gitops-table-$_section'),
      contextId: widget.session.contextId,
      rows: rows,
      kind: resources.first.api,
      columns: [
        l.kubeGitOpsSource,
        l.kubeGitOpsRevision,
        l.kubeGitOpsRecent,
        l.kubeGitOpsInterval,
        l.kubeGitOpsSuspended,
      ].map((name) => KubeTableColumn(name: name, type: 'string', format: '', description: '', priority: 0)).toList(),
      selected: {
        for (final row in rows)
          if (_selected.contains(_selectionKey(byId[row.id]!))) row.id,
      },
      canSelect: canWrite && !widget.session.production && !_bulkBusy,
      onSelection: (row) => setState(() {
        final resource = byId[row.id]!;
        if (!resource.isFlux || kubeGitOpsActions(resource).isEmpty) return;
        final key = _selectionKey(resource);
        if (_selected.contains(key)) {
          _selected.remove(key);
        } else if (_selected.length < 100) {
          _selected.add(key);
        }
      }),
      onOpen: (row) => unawaited(_open(byId[row.id]!)),
      hasMore: false,
      loadingMore: false,
      onMore: () {},
    );
  }

  List<KubeIssue> _issuesFor(KubeInsights data, KubeInsightResource resource) => data.issues
      .where(
        (issue) =>
            issue.kind == resource.api.kind &&
            issue.name == resource.resource.name &&
            issue.namespace == resource.resource.namespace,
      )
      .toList();

  Widget _dependencyView(KubeInsights data) {
    final l = context.l10n;
    final nodes = <String, KubeFlowNode>{}, labels = <String, String>{};
    final edges = <KubeFlowEdge>[];
    for (final edge in data.gitops.dependencies) {
      if (widget.namespace != null && edge.from.namespace != widget.namespace) continue;
      if (!'${edge.from.label} ${edge.to.label}'.toLowerCase().contains(_filter)) continue;
      final source = _find(edge.from);
      nodes[edge.from.label] = KubeFlowNode(object: edge.from, ready: source == null ? null : _ready(source));
      nodes[edge.to.label] = KubeFlowNode(object: edge.to, ready: edge.ready && !edge.missing);
      // Dependencies appear before the objects waiting for them.
      edges.add(KubeFlowEdge(from: edge.to, to: edge.from, relation: 'dependency'));
      if (edge.missing) labels[edge.to.label] = l.kubeDependencyMissing;
      if (!edge.ready) {
        final conditions = (source?.object['status'] as Map?)?['conditions'] as List? ?? const [];
        final since = conditions
            .whereType<Map<String, dynamic>>()
            .where((c) => c['reason'] == 'DependencyNotReady')
            .map((c) => c['lastTransitionTime'])
            .whereType<String>()
            .firstOrNull;
        labels[edge.from.label] = [l.kubeDependencyWaiting, ?since].join(' · ');
      }
    }
    return Column(
      key: const ValueKey('kube-dependency-list'),
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (data.gitops.cycles.isNotEmpty)
          ConstrainedBox(
            constraints: const BoxConstraints(maxHeight: 100),
            child: SingleChildScrollView(
              child: Text(
                /* l10n-ignore: Localized cycle message followed by exact resource identities. */ '${l.kubeDependencyCycle}: ${data.gitops.cycles.take(100).map((c) => c.label).join(', ')}',
              ),
            ),
          ),
        Expanded(
          child: KubeGitOpsGraph(
            flow: KubeGitOpsFlow(nodes: nodes.values.toList(), edges: edges),
            resources: data.resources,
            dependencyLayout: true,
            statusLabel: (identity) => labels[identity.label],
            canOpen: (identity) => _find(identity) != null,
            open: (identity) {
              final resource = _find(identity);
              if (resource != null) unawaited(_open(resource));
            },
          ),
        ),
      ],
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
