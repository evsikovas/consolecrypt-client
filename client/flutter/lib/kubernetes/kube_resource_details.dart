import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_forward_panel.dart';
import 'package:consolecrypt/kubernetes/kube_forward_service.dart';
import 'package:consolecrypt/kubernetes/kube_forward_targets_dialog.dart';
import 'package:consolecrypt/kubernetes/kube_forward_targets_service.dart';
import 'package:consolecrypt/kubernetes/kube_gitops_actions.dart';
import 'package:consolecrypt/kubernetes/kube_gitops_details.dart';
import 'package:consolecrypt/kubernetes/kube_gitops_graph.dart';
import 'package:consolecrypt/kubernetes/kube_gitops_monitor.dart';
import 'package:consolecrypt/kubernetes/kube_helm_panel.dart';
import 'package:consolecrypt/kubernetes/kube_insights_service.dart';
import 'package:consolecrypt/kubernetes/kube_logs_panel.dart';
import 'package:consolecrypt/kubernetes/kube_manifest_service.dart';
import 'package:consolecrypt/kubernetes/kube_mutation_service.dart';
import 'package:consolecrypt/kubernetes/kube_operations_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_relations_service.dart';
import 'package:consolecrypt/kubernetes/kube_resource_operations.dart';
import 'package:consolecrypt/kubernetes/kube_resource_relations.dart';
import 'package:consolecrypt/kubernetes/kube_resource_route.dart';
import 'package:consolecrypt/kubernetes/kube_resource_summary.dart';
import 'package:consolecrypt/kubernetes/kube_rollout_panel.dart';
import 'package:consolecrypt/kubernetes/kube_rollout_service.dart';
import 'package:consolecrypt/kubernetes/kube_secret_dialog.dart';
import 'package:consolecrypt/kubernetes/kube_terminal_panel.dart';
import 'package:consolecrypt/kubernetes/kube_write_access.dart';
import 'package:consolecrypt/kubernetes/kube_yaml_editor.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class KubeResourceDetails extends ConsumerStatefulWidget {
  const KubeResourceDetails({
    required this.session,
    required this.resource,
    required this.service,
    required this.contextName,
    this.onTerminalOpened,
    this.onExplain,
    this.onPinLogs,
    this.onOutput,
    this.ancestors = const [],
    this.openedResources = const [],
    this.onSelectResource,
    this.onCloseResource,
    this.gitOpsMonitor,
    this.initialAction,
    this.initialLogs = false,
    this.initialControllerLogs = false,
    super.key,
  });
  final KubeSession session;
  final KubeGitOpsMonitor? gitOpsMonitor;
  final bool initialLogs, initialControllerLogs;
  final KubeResourceOpenAction? initialAction;
  final List<KubeResourceRef> ancestors, openedResources;
  final ValueChanged<KubeResourceRef>? onSelectResource, onCloseResource;
  final KubeResourceRef resource;
  final KubeBrowserService service;
  final String contextName;
  final VoidCallback? onTerminalOpened;
  final void Function(String title, String text)? onOutput;
  final void Function(KubeResourceRef, String?)? onExplain;
  final void Function(KubeResourceRef, List<String>)? onPinLogs;
  @override
  ConsumerState<KubeResourceDetails> createState() => _KubeResourceDetailsState();
}

class _KubeResourceDetailsState extends ConsumerState<KubeResourceDetails> with SingleTickerProviderStateMixin {
  late final KubeScope _scope;
  late final TabController _tabs;
  final _gitOpsDetailsKey = GlobalKey<KubeGitOpsDetailsState>();
  KubeResourceDetail? _detail;
  bool _loading = true, _failed = false, _openedInitialControllerLogs = false, _openedInitialAction = false;
  int _generation = 0;
  bool _mutating = false, _mutationFailed = false, _changed = false;
  bool get _hasLogs =>
      widget.resource.api.isPod ||
      const ['Deployment', 'StatefulSet', 'DaemonSet', 'ReplicaSet', 'Job'].contains(widget.resource.api.kind);

  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(kubeScopeProvider));

  @override
  void initState() {
    super.initState();
    _scope = ref.read(kubeScopeProvider);
    _tabs = TabController(length: _hasLogs ? 5 : 4, initialIndex: widget.initialLogs && _hasLogs ? 2 : 0, vsync: this)
      ..addListener(() {
        if (mounted) setState(() {});
      });
    unawaited(_load());
  }

  @override
  void dispose() {
    _generation++;
    _detail = null;
    _tabs.dispose();
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
      final detail = await widget.service.getResource(widget.session.id, widget.resource);
      if (!_current || generation != _generation) return;
      setState(() {
        _detail = detail;
      });
    } catch (_) {
      if (_current && generation == _generation) setState(() => _failed = true);
    } finally {
      if (_current && generation == _generation) {
        setState(() => _loading = false);
        if (_detail != null && widget.initialAction != null && !_openedInitialAction) {
          _openedInitialAction = true;
          WidgetsBinding.instance.addPostFrameCallback((_) {
            if (_current) unawaited(_reviewInitialAction());
          });
        }
        if (_detail != null && widget.initialControllerLogs && !_openedInitialControllerLogs) {
          _openedInitialControllerLogs = true;
          WidgetsBinding.instance.addPostFrameCallback((_) {
            if (_current) unawaited(_gitOpsDetailsKey.currentState?.openControllerLogs());
          });
        }
      }
    }
  }

  Future<void> _reviewInitialAction() async {
    if (!_current || _detail == null) return;
    if (widget.initialAction == KubeResourceOpenAction.shell && widget.resource.api.isPod) {
      await _terminal();
    } else if (widget.initialAction == KubeResourceOpenAction.restart && widget.service is KubeOperationsService) {
      setState(() => _mutating = true);
      try {
        final changed = await reviewKubeRestart(
          context,
          session: widget.session,
          resource: widget.resource,
          detail: _detail!,
          service: widget.service as KubeOperationsService,
        );
        if (_current && changed) {
          _changed = true;
          await _load();
        }
      } catch (_) {
        if (_current) setState(() => _mutationFailed = true);
      } finally {
        if (_current) setState(() => _mutating = false);
      }
    }
  }

  Future<void> _terminal() async {
    if (!_current || _detail == null || !widget.session.canExec) return;
    final opened = await openKubePodTerminal(
      context,
      session: widget.session,
      contextName: widget.contextName,
      resource: widget.resource,
      containers: _detail!.containers,
    );
    if (!mounted || !_current || !opened) return;
    _terminalOpened();
  }

  void _terminalOpened() {
    if (!_current) return;
    final callback = widget.onTerminalOpened;
    closeDialog(context, _changed);
    if (callback != null) WidgetsBinding.instance.addPostFrameCallback((_) => callback());
  }

  void _explain(KubeResourceRef resource, String? question) {
    if (!_current || widget.onExplain == null) return;
    final callback = widget.onExplain!;
    closeDialog(context, _changed);
    WidgetsBinding.instance.addPostFrameCallback((_) => callback(resource, question));
  }

  Future<void> _related(KubeResourceRef resource) async {
    if (!_current) return;
    final changed = await showKubeResourceDetails<bool>(
      context,
      builder: (_) => KubeResourceDetails(
        session: widget.session,
        contextName: widget.contextName,
        resource: resource,
        service: widget.service,
        gitOpsMonitor: widget.gitOpsMonitor,
        onTerminalOpened: _terminalOpened,
        onExplain: widget.onExplain == null ? null : _explain,
        onPinLogs: widget.onPinLogs,
        onOutput: widget.onOutput,
        ancestors: [...widget.ancestors, widget.resource],
      ),
    );
    if (_current && changed == true) {
      _changed = true;
      await _load();
    }
  }

  KubeInsightResource? get _gitOpsResource {
    final detail = _detail;
    if (detail == null) return null;
    final metadata = detail.object['metadata'] as Map?;
    final resource = KubeInsightResource(
      api: widget.resource.api,
      resource: KubeResourceRow(
        name: widget.resource.name,
        namespace: widget.resource.namespace,
        uid: metadata?['uid'] as String? ?? '',
        resourceVersion: metadata?['resourceVersion'] as String? ?? '',
        status: '',
        object: detail.object,
      ),
    );
    return resource.isFlux || resource.isArgo ? resource : null;
  }

  Future<void> _gitOpsAction(String action) async {
    final resource = _gitOpsResource;
    if (!_current || _mutating || resource == null || widget.service is! KubeInsightsService) return;
    setState(() {
      _mutating = true;
      _mutationFailed = false;
    });
    try {
      final changed = await runKubeGitOpsAction(
        context,
        ref,
        session: widget.session,
        resource: resource,
        service: widget.service as KubeInsightsService,
        action: action,
        monitor: widget.gitOpsMonitor,
      );
      if (_current && changed) {
        _changed = true;
        await _load();
        unawaited(widget.gitOpsMonitor?.refresh());
      }
    } catch (_) {
      if (_current) setState(() => _mutationFailed = true);
    } finally {
      if (_current) setState(() => _mutating = false);
    }
  }

  Future<void> _gitOpsGraph() async {
    if (!_current || widget.service is! KubeInsightsService) return;
    try {
      final data =
          widget.gitOpsMonitor?.data ?? await (widget.service as KubeInsightsService).insights(widget.session.id);
      if (!mounted || !_current) return;
      KubeResourceRef? find(KubeObjectIdentity id) => data.resources
          .where(
            (r) =>
                r.api.kind == id.kind &&
                r.api.group == id.group &&
                r.resource.name == id.name &&
                (r.resource.namespace ?? '') == id.namespace,
          )
          .firstOrNull
          ?.reference;
      await showAppDialog<void>(
        context,
        secure: true,
        builder: (dialogContext) => GlassDialog(
          title: context.l10n.kubeGitOpsFlow,
          width: 1000,
          content: SizedBox(
            height: 520,
            child: KubeGitOpsGraph(
              flow: data.flow,
              resources: data.resources,
              canOpen: (id) => find(id) != null,
              open: (id) {
                final resource = find(id);
                if (resource != null) unawaited(_related(resource));
              },
            ),
          ),
          primaryAction: GlassButton(
            label: context.l10n.commonClose,
            onPressed: () => closeDialog<void>(dialogContext),
          ),
        ),
      );
    } catch (_) {
      if (_current) setState(() => _failed = true);
    }
  }

  Future<void> _controllerLogs(KubeResourceRef controller, String filter, [DateTime? since]) async {
    if (!_current) return;
    await showAppDialog<void>(
      context,
      secure: true,
      builder: (dialogContext) => GlassDialog(
        title: '${context.l10n.kubeGitOpsControllerLogs} · ${controller.namespace}/${controller.name}',
        width: 980,
        content: SizedBox(
          height: 580,
          child: KubeLogsPanel(
            session: widget.session,
            resource: controller,
            service: widget.service,
            initialFilter: filter,
            initialSinceSeconds: since == null
                ? null
                : (DateTime.now().difference(since).inSeconds + 60).clamp(0, 604800),
          ),
        ),
        primaryAction: GlassButton(label: context.l10n.commonClose, onPressed: () => closeDialog<void>(dialogContext)),
      ),
    );
  }

  Future<void> _helm(String namespace, String releaseName) async {
    if (!_current || widget.service is! KubeInsightsService) return;
    await showAppDialog<void>(
      context,
      secure: true,
      builder: (dialogContext) => GlassDialog(
        title: '${context.l10n.kubeHelm} · $namespace/$releaseName',
        width: 980,
        content: SizedBox(
          height: 580,
          child: KubeHelmPanel(
            session: widget.session,
            contextName: widget.contextName,
            service: widget.service as KubeInsightsService,
            namespace: namespace,
            releaseName: releaseName,
          ),
        ),
        primaryAction: GlassButton(label: context.l10n.commonClose, onPressed: () => closeDialog<void>(dialogContext)),
      ),
    );
  }

  Future<void> _edit() async {
    final detail = _detail;
    final service = widget.service;
    if (!_current ||
        detail == null ||
        service is! KubeMutationService ||
        widget.resource.api.isSecret ||
        detail.yaml.contains('[redacted]')) {
      return;
    }
    final changed = await showAppDialog<bool>(
      context,
      secure: true,
      barrierDismissible: false,
      builder: (_) => KubeYamlEditor(
        session: widget.session,
        contextName: widget.contextName,
        resource: widget.resource,
        original: detail,
        service: service as KubeMutationService,
        onOutput: widget.onOutput,
      ),
    );
    if (_current && changed == true) {
      _changed = true;
      await _load();
    }
  }

  Future<void> _delete() async {
    final service = widget.service;
    final detail = _detail;
    if (!_current || _mutating || detail == null || service is! KubeMutationService) return;
    final metadata = detail.object['metadata'] as Map?;
    final uid = metadata?['uid'] as String?;
    final version = metadata?['resourceVersion'] as String?;
    if (uid == null || uid.isEmpty || version == null || version.isEmpty) return;
    final approved = await showTypedKubeConfirmation(
      context,
      expected: widget.resource.name,
      title: context.l10n.kubeDeleteResource,
      message: context.l10n.kubeDeleteResourceHelp,
      confirmLabel: context.l10n.commonDelete,
    );
    if (!_current || !approved || !(ref.read(kubeWriteAccessProvider(widget.session)).value?.canWrite ?? false)) return;
    setState(() {
      _mutating = true;
      _mutationFailed = false;
    });
    try {
      await (service as KubeMutationService).deleteResource(
        widget.session.id,
        widget.resource,
        uid: uid,
        resourceVersion: version,
      );
      if (mounted && _current) closeDialog(context, true);
    } catch (_) {
      if (_current) setState(() => _mutationFailed = true);
    } finally {
      if (_current) setState(() => _mutating = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(kubeScopeProvider, (_, next) {
      if (identical(next, _scope)) return;
      _generation++;
      _detail = null;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted) return;
        final route = ModalRoute.of(context);
        if (route?.isActive == true) Navigator.of(context).removeRoute(route!);
      });
    });
    final l = context.l10n;
    final detail = _detail;
    final canWrite = ref.watch(kubeWriteAccessProvider(widget.session)).value?.canWrite ?? false;
    final metadata = detail?.object['metadata'] as Map?;
    final masked = detail?.yaml.contains('[redacted]') ?? false;
    void shortcut(VoidCallback action) {
      final focused = FocusManager.instance.primaryFocus?.context;
      if (!_current ||
          ModalRoute.of(context)?.isCurrent != true ||
          focused?.widget is EditableText ||
          focused?.findAncestorStateOfType<EditableTextState>() != null) {
        return;
      }
      action();
    }

    return CallbackShortcuts(
      bindings: {
        const SingleActivator(LogicalKeyboardKey.keyY): () => shortcut(() => _tabs.animateTo(1)),
        const SingleActivator(LogicalKeyboardKey.keyD): () => shortcut(() => _tabs.animateTo(0)),
        const SingleActivator(LogicalKeyboardKey.keyL): () => shortcut(() {
          if (_hasLogs) {
            _tabs.animateTo(2);
          } else {
            unawaited(_gitOpsDetailsKey.currentState?.openControllerLogs());
          }
        }),
        const SingleActivator(LogicalKeyboardKey.keyS): () => shortcut(() {
          if (widget.resource.api.isPod) unawaited(_terminal());
        }),
        const SingleActivator(LogicalKeyboardKey.keyR): () => shortcut(() => unawaited(_gitOpsAction('reconcile'))),
        const SingleActivator(LogicalKeyboardKey.keyR, shift: true): () =>
            shortcut(() => unawaited(_gitOpsAction('reconcile_with_source'))),
        const SingleActivator(LogicalKeyboardKey.keyP): () =>
            shortcut(() => unawaited(_gitOpsAction(_gitOpsResource?.suspended == true ? 'resume' : 'suspend'))),
        const SingleActivator(LogicalKeyboardKey.keyG): () => shortcut(() => unawaited(_gitOpsGraph())),
        const SingleActivator(LogicalKeyboardKey.delete): () => shortcut(() {
          if (canWrite && !_mutating) unawaited(_delete());
        }),
      },
      child: Focus(
        autofocus: true,
        child: GlassDialog(
          key: const ValueKey('kube-resource-details'),
          width: 980,
          title: '${widget.resource.api.kind} · ${widget.resource.name}',
          content: SizedBox(
            height: AppPlatform.isMobile && MediaQuery.sizeOf(context).width < 720 ? double.infinity : 580,
            child: !_current
                ? const SizedBox.shrink()
                : Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      if (widget.openedResources.isNotEmpty && widget.onSelectResource != null)
                        GlassTabStrip(
                          tabs: widget.openedResources
                              .map(
                                (resource) => GlassTab(
                                  title: resource.name,
                                  subtitle: '${resource.api.kind} · ${resource.namespace ?? ''}', // l10n-ignore: Kubernetes kind, namespace, resource name or API condition status.
                                  key: ValueKey('kube-object-tab-${resource.id}'),
                                ),
                              )
                              .toList(),
                          activeIndex: widget.openedResources.indexWhere(
                            (resource) => resource.id == widget.resource.id,
                          ),
                          onSelect: (index) {
                            final target = widget.openedResources[index];
                            if (target.id == widget.resource.id || !_current) return;
                            closeDialog(context, _changed);
                            WidgetsBinding.instance.addPostFrameCallback((_) => widget.onSelectResource?.call(target));
                          },
                          onClose: (index) {
                            final target = widget.openedResources[index];
                            widget.onCloseResource?.call(target);
                            final remaining = widget.openedResources.where((ref) => ref.id != target.id).toList();
                            closeDialog(context, _changed);
                            final next = target.id == widget.resource.id ? remaining.lastOrNull : widget.resource;
                            if (next != null) {
                              WidgetsBinding.instance.addPostFrameCallback((_) => widget.onSelectResource?.call(next));
                            }
                          },
                        ),
                      if (widget.ancestors.isNotEmpty)
                        SingleChildScrollView(
                          scrollDirection: Axis.horizontal,
                          child: Row(
                            children: [
                              for (final (index, ancestor) in widget.ancestors.indexed)
                                TextButton(
                                  onPressed: () {
                                    if (!_current) return;
                                    final navigator = Navigator.of(context);
                                    for (var n = index; n < widget.ancestors.length && navigator.canPop(); n++) {
                                      navigator.pop(_changed);
                                    }
                                  },
                                  child: Text('${ancestor.api.kind} ${ancestor.name} ›'),
                                ),
                              Text(widget.resource.name),
                            ],
                          ),
                        ),
                      KubeWriteBanner(session: widget.session, contextName: widget.contextName),
                      if (widget.resource.api.isSecret) Text(l.kubeSecretRedacted),
                      if (masked) Text(l.kubeMaskedEditHelp),
                      if (_loading) const LinearProgressIndicator(),
                      if (_failed)
                        Text(l.kubeRequestFailed, style: TextStyle(color: Theme.of(context).colorScheme.error)),
                      if (_mutationFailed)
                        Text(l.kubeApplyFailed, style: TextStyle(color: Theme.of(context).colorScheme.error)),
                      if (!AppPlatform.isMobile && _gitOpsResource != null) _gitOpsActionBar(canWrite),
                      if (widget.service is KubeMutationService && detail != null)
                        Wrap(
                          spacing: 8,
                          runSpacing: 8,
                          children: [
                            if (!widget.resource.api.isSecret)
                              GlassButton(
                                key: const ValueKey('kube-edit-yaml'),
                                label: l.kubeEditYaml,
                                icon: Icons.edit_outlined,
                                onPressed: canWrite && !_mutating && !masked ? _edit : null,
                              ),
                            if (metadata?['uid'] is String && metadata?['resourceVersion'] is String)
                              GlassButton.destructive(
                                key: const ValueKey('kube-delete-resource'),
                                label: l.commonDelete,
                                icon: Icons.delete_outline,
                                onPressed: canWrite && !_mutating ? _delete : null,
                              ),
                          ],
                        ),
                      if (detail != null) ...[
                        TabBar(
                          controller: _tabs,
                          isScrollable: true,
                          tabAlignment: TabAlignment.start,
                          tabs: [
                            Tab(text: l.kubeOverview),
                            Tab(key: const ValueKey('kube-yaml-tab'), text: l.kubeObjectYaml),
                            if (_hasLogs) Tab(key: const ValueKey('kube-logs-tab'), text: l.kubeLogs),
                            Tab(key: const ValueKey('kube-events-tab'), text: l.kubeEvents),
                            Tab(key: const ValueKey('kube-related-tab'), text: l.kubeRelatedObjects),
                          ],
                        ),
                        const SizedBox(height: 8),
                        Expanded(
                          child: TabBarView(
                            controller: _tabs,
                            children: [
                              _overview(detail),
                              SingleChildScrollView(
                                key: const ValueKey('kube-resource-yaml'),
                                child: SelectableText(detail.yaml, style: GlassTokens.of(context).typography.mono),
                              ),
                              if (_hasLogs)
                                Column(
                                  crossAxisAlignment: CrossAxisAlignment.stretch,
                                  children: [
                                    if (widget.onPinLogs != null)
                                      Align(
                                        alignment: Alignment.centerLeft,
                                        child: GlassButton(
                                          key: const ValueKey('kube-pin-logs'),
                                          label: l.kubePinLogs,
                                          icon: Icons.push_pin_outlined,
                                          onPressed: () {
                                            final callback = widget.onPinLogs!;
                                            closeDialog(context, _changed);
                                            WidgetsBinding.instance.addPostFrameCallback((_) {
                                              callback(widget.resource, detail.containers);
                                            });
                                          },
                                        ),
                                      ),
                                    Expanded(
                                      child: KubeLogsPanel(
                                        session: widget.session,
                                        resource: widget.resource,
                                        service: widget.service,
                                        containers: detail.containers,
                                        active: _tabs.index == 2,
                                      ),
                                    ),
                                  ],
                                ),
                              SingleChildScrollView(
                                child: _relations(detail, events: true, links: false, metrics: false),
                              ),
                              SingleChildScrollView(
                                child: _relations(detail, events: false, links: true, metrics: false),
                              ),
                            ],
                          ),
                        ),
                      ],
                      if (AppPlatform.isMobile && _gitOpsResource != null) _gitOpsActionBar(canWrite),
                    ],
                  ),
          ),
          primaryAction: GlassButton(
            label: l.commonClose,
            onPressed: _mutating ? null : () => closeDialog(context, _changed),
          ),
          secondaryActions: [
            GlassIconButton(
              key: const ValueKey('kube-refresh-details'),
              icon: Icons.refresh,
              tooltip: l.kubeRefreshDetails,
              onPressed: _loading ? null : _load,
            ),
          ],
        ),
      ),
    );
  }

  Widget _gitOpsActionBar(bool canWrite) {
    final l = context.l10n, resource = _gitOpsResource!;
    return SingleChildScrollView(
      key: ValueKey(AppPlatform.isMobile ? 'kube-gitops-bottom-actions' : 'kube-gitops-top-actions'),
      scrollDirection: Axis.horizontal,
      child: Row(
        children: [
          for (final action in kubeGitOpsActions(resource))
            Padding(
              padding: const EdgeInsets.only(right: 8),
              child: GlassButton(
                key: ValueKey('kube-gitops-detail-$action'),
                label: kubeGitOpsActionLabel(l, action),
                onPressed: canWrite && !_mutating ? () => _gitOpsAction(action) : null,
              ),
            ),
          GlassButton(
            label: l.kubeLogs,
            icon: Icons.article_outlined,
            onPressed: () {
              if (_current) unawaited(_gitOpsDetailsKey.currentState?.openControllerLogs());
            },
          ),
          const SizedBox(width: 8),
          GlassButton(label: l.kubeGitOpsFlow, icon: Icons.account_tree_outlined, onPressed: _gitOpsGraph),
        ],
      ),
    );
  }

  Widget _relations(KubeResourceDetail detail, {required bool events, required bool links, required bool metrics}) =>
      KubeResourceRelations(
        session: widget.session,
        resource: widget.resource,
        detail: detail,
        service: widget.service,
        showEvents: events,
        showRelations: links,
        showMetrics: metrics,
        openResource: _related,
        openPod: (row) => _related(
          KubeResourceRef(
            api: const KubeResourceKind(
              group: '',
              version: 'v1',
              resource: 'pods',
              kind: 'Pod',
              namespaced: true,
              verbs: ['get', 'list', 'watch'],
            ),
            name: row.name,
            namespace: row.namespace,
          ),
        ),
      );

  Widget _overview(KubeResourceDetail detail) {
    final l = context.l10n;
    final metadata = detail.object['metadata'] as Map? ?? const {};
    final status = detail.object['status'] as Map? ?? const {};
    final conditions = status['conditions'] as List? ?? const [];
    final labels = metadata['labels'] as Map? ?? const {};
    final owners = metadata['ownerReferences'] as List? ?? const [];
    Widget entry(String label, Object? value) => Padding(
      padding: const EdgeInsets.only(bottom: 12),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(label, style: Theme.of(context).textTheme.labelLarge),
          SelectableText(value?.toString() ?? l.kubeNotReported),
        ],
      ),
    );
    return ListView(
      key: const ValueKey('kube-resource-overview'),
      children: [
        if (widget.onExplain != null)
          Align(
            alignment: Alignment.centerLeft,
            child: GlassButton(
              key: const ValueKey('kube-resource-ai'),
              label: l.kubeAiExplain,
              icon: Icons.auto_awesome,
              onPressed: () => _explain(widget.resource, null),
            ),
          ),
        if (widget.service is KubeRolloutService &&
            widget.resource.api.group == 'apps' &&
            const ['Deployment', 'StatefulSet', 'DaemonSet'].contains(widget.resource.api.kind))
          Align(
            alignment: Alignment.centerLeft,
            child: GlassButton(
              key: const ValueKey('kube-rollout-history'),
              label: l.kubeRolloutHistory,
              icon: Icons.history,
              onPressed: () => showAppDialog<void>(
                context,
                secure: true,
                builder: (_) => KubeRolloutDialog(
                  sessionId: widget.session.id,
                  resource: widget.resource,
                  service: widget.service as KubeRolloutService,
                ),
              ),
            ),
          ),
        if (widget.resource.api.isPod) ...[
          Align(
            alignment: Alignment.centerLeft,
            child: GlassButton(
              key: const ValueKey('kube-pod-terminal'),
              label: l.kubePodTerminal,
              icon: Icons.terminal,
              onPressed: widget.session.canExec ? _terminal : null,
            ),
          ),
          if (!widget.session.canExec) Text(l.kubeExecProductionBlocked),
          if (widget.service is KubeForwardService) ...[
            Align(
              alignment: Alignment.centerLeft,
              child: GlassButton(
                key: const ValueKey('kube-pod-forward'),
                label: l.kubePortForward,
                icon: Icons.swap_horiz,
                onPressed: widget.session.canPortForward
                    ? () => showKubeForwardDialog(
                        context,
                        session: widget.session,
                        resource: widget.resource,
                        service: widget.service as KubeForwardService,
                        detail: detail,
                      )
                    : null,
              ),
            ),
            if (!widget.session.canPortForward) Text(l.kubeForwardProductionBlocked),
          ],
          const SizedBox(height: 12),
        ],
        if (!widget.resource.api.isPod &&
            widget.service is KubeForwardTargetsService &&
            widget.service is KubeForwardService &&
            const [
              'Service',
              'Deployment',
              'StatefulSet',
              'DaemonSet',
              'ReplicaSet',
              'Job',
            ].contains(widget.resource.api.kind))
          Align(
            alignment: Alignment.centerLeft,
            child: GlassButton(
              key: const ValueKey('kube-resource-forward'),
              label: l.kubePortForward,
              icon: Icons.swap_horiz,
              onPressed: widget.session.canPortForward
                  ? () => showKubeResourceForwardDialog(
                      context,
                      session: widget.session,
                      resource: widget.resource,
                      service: widget.service,
                      detail: detail,
                    )
                  : null,
            ),
          ),
        if (widget.resource.api.group.endsWith('.toolkit.fluxcd.io') || widget.resource.api.group == 'argoproj.io')
          KubeGitOpsDetails(
            key: _gitOpsDetailsKey,
            session: widget.session,
            resource: widget.resource,
            detail: detail,
            service: widget.service,
            onOpen: _related,
            onLogs: _controllerLogs,
            onLogsAt: (controller, filter, since) => _controllerLogs(controller, filter, since),
            onHelm: widget.service is KubeInsightsService ? _helm : null,
            onChanged: () {
              _changed = true;
              unawaited(_load());
            },
          ),
        KubeResourceSummary(resource: widget.resource, detail: detail),
        if (widget.service is KubeOperationsService)
          KubeResourceOperations(
            session: widget.session,
            resource: widget.resource,
            detail: detail,
            service: widget.service as KubeOperationsService,
            onChanged: () {
              _changed = true;
              unawaited(_load());
            },
          ),
        if (widget.service is KubeOperationsService ||
            widget.service is KubeMetricsService ||
            widget.service is KubeRelationsService)
          _relations(detail, events: false, links: false, metrics: true),
        if (widget.resource.api.isSecret && widget.service is KubeManifestService)
          Align(
            alignment: Alignment.centerLeft,
            child: GlassButton(
              key: const ValueKey('kube-open-secret'),
              label: l.kubeRevealSecret,
              icon: Icons.visibility_outlined,
              onPressed: () => showAppDialog<void>(
                context,
                secure: true,
                builder: (_) => KubeSecretDialog(
                  session: widget.session,
                  resource: widget.resource,
                  service: widget.service as KubeManifestService,
                ),
              ),
            ),
          ),
        entry(l.kubeName, widget.resource.name),
        if (widget.resource.namespace != null) entry(l.kubeNamespace, widget.resource.namespace),
        entry(l.kubeCreatedAt, metadata['creationTimestamp']),
        entry(l.kubeStatus, status['phase']),
        if (labels.isNotEmpty) entry(l.kubeLabels, labels.entries.map((e) => '${e.key}: ${e.value}').join('\n')),
        if (conditions.isNotEmpty) ...[
          Text(l.kubeConditions, style: Theme.of(context).textTheme.titleSmall),
          ...conditions.whereType<Map<String, dynamic>>().map(
            (condition) => ListTile(
              contentPadding: EdgeInsets.zero,
              title: Text(
                /* l10n-ignore: Kubernetes kind, namespace, resource name or API condition status. */ '${condition['type'] ?? ''}: ${condition['status'] ?? ''}',
              ), // l10n-ignore: Kubernetes kind, namespace, resource name or API condition status.
              subtitle: Text([condition['reason'], condition['message']].whereType<String>().join('\n')),
            ),
          ),
        ],
        if (owners.isNotEmpty)
          entry(
            l.kubeOwners,
            owners.whereType<Map<String, dynamic>>().map((owner) => '${owner['kind']}: ${owner['name']}').join('\n'),
          ),
        if (detail.containers.isNotEmpty) entry(l.kubeContainer, detail.containers.join('\n')),
        if (status.isNotEmpty && conditions.isEmpty && status['phase'] == null)
          entry(l.kubeStatus, const JsonEncoder.withIndent('  ').convert(status)),
      ],
    );
  }
}
