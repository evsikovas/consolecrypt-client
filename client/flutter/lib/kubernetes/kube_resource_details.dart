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
import 'package:consolecrypt/kubernetes/kube_object_overview.dart';
import 'package:consolecrypt/kubernetes/kube_operations_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_relations_service.dart';
import 'package:consolecrypt/kubernetes/kube_resource_operations.dart';
import 'package:consolecrypt/kubernetes/kube_resource_relations.dart';
import 'package:consolecrypt/kubernetes/kube_resource_route.dart';
import 'package:consolecrypt/kubernetes/kube_resource_summary.dart';
import 'package:consolecrypt/kubernetes/kube_resource_table_view.dart';
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
    this.embedded = false,
    this.onEmbeddedClose,
    this.onChanged,
    super.key,
  });

  /// Desktop inspector: rendered in place instead of as a dialog. Closing
  /// calls [onEmbeddedClose] with whether the object changed; related objects
  /// open through [onSelectResource].
  final bool embedded;
  final ValueChanged<bool>? onEmbeddedClose;
  final VoidCallback? onChanged;
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

  void _close([bool? changed]) {
    if (widget.embedded) {
      widget.onEmbeddedClose?.call(changed ?? _changed);
    } else {
      closeDialog(context, changed ?? _changed);
    }
  }

  void _terminalOpened() {
    if (!_current) return;
    final callback = widget.onTerminalOpened;
    if (widget.embedded) {
      if (callback != null) WidgetsBinding.instance.addPostFrameCallback((_) => callback());
      return;
    }
    closeDialog(context, _changed);
    if (callback != null) WidgetsBinding.instance.addPostFrameCallback((_) => callback());
  }

  void _explain(KubeResourceRef resource, String? question) {
    if (!_current || widget.onExplain == null) return;
    final callback = widget.onExplain!;
    if (!widget.embedded) closeDialog(context, _changed);
    WidgetsBinding.instance.addPostFrameCallback((_) => callback(resource, question));
  }

  Future<void> _related(KubeResourceRef resource) async {
    if (!_current) return;
    if (widget.embedded) {
      widget.onSelectResource?.call(resource);
      return;
    }
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
      widget.onChanged?.call();
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
        widget.onChanged?.call();
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
        // Browsing view: regular glass in the app palette.
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
      // Browsing view: regular glass in the app palette.
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
      // Browsing view: regular glass in the app palette.
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
      widget.onChanged?.call();
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
      if (mounted && _current) _close(true);
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
      if (widget.embedded) return;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted) return;
        final route = ModalRoute.of(context);
        if (route?.isActive == true) Navigator.of(context).removeRoute(route!);
      });
    });
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    final p = tokens.palette;
    final detail = _detail;
    final canWrite = ref.watch(kubeWriteAccessProvider(widget.session)).value?.canWrite ?? false;
    final masked = detail?.yaml.contains('[redacted]') ?? false;
    void shortcut(VoidCallback action) {
      final focused = FocusManager.instance.primaryFocus?.context;
      if (!_current ||
          (!widget.embedded && ModalRoute.of(context)?.isCurrent != true) ||
          focused?.widget is EditableText ||
          focused?.findAncestorStateOfType<EditableTextState>() != null) {
        return;
      }
      action();
    }

    final status = detail == null ? '' : kubeObjectStatus(widget.resource, detail.object);
    final metadata = detail?.object['metadata'] as Map? ?? const {};
    final header = Padding(
      padding: const EdgeInsets.fromLTRB(GlassSpacing.s16, GlassSpacing.s12, GlassSpacing.s8, 0),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    GlassBadge(label: widget.resource.api.kind, dense: true),
                    const SizedBox(width: GlassSpacing.s8),
                    if (status.isNotEmpty) Flexible(child: KubeStatusLabel(status: status)),
                  ],
                ),
                const SizedBox(height: GlassSpacing.s6),
                SelectableText(
                  widget.resource.name,
                  maxLines: 2,
                  style: tokens.typography.title3.copyWith(color: p.label),
                ),
                const SizedBox(height: GlassSpacing.s2),
                Text(
                  [
                    if (widget.resource.namespace != null) widget.resource.namespace!,
                    widget.resource.api.apiVersion,
                    if (metadata['creationTimestamp'] != null) kubeAge(metadata['creationTimestamp']),
                  ].join(' · '),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel),
                ),
              ],
            ),
          ),
          GlassIconButton(
            key: const ValueKey('kube-refresh-details'),
            icon: Icons.refresh,
            tooltip: l.kubeRefreshDetails,
            style: GlassIconButtonStyle.plain,
            onPressed: _loading ? null : _load,
          ),
          GlassIconButton(
            key: const ValueKey('kube-close-details'),
            icon: Icons.close_rounded,
            tooltip: l.kubeCloseInspector,
            style: GlassIconButtonStyle.plain,
            onPressed: _mutating ? null : () => _close(),
          ),
        ],
      ),
    );
    final body = !_current
        ? const SizedBox.shrink()
        : Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (!widget.embedded && widget.openedResources.isNotEmpty && widget.onSelectResource != null)
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
                  activeIndex: widget.openedResources.indexWhere((resource) => resource.id == widget.resource.id),
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
              if (!widget.embedded && widget.ancestors.isNotEmpty)
                SingleChildScrollView(
                  scrollDirection: Axis.horizontal,
                  child: Row(
                    children: [
                      for (final (index, ancestor) in widget.ancestors.indexed)
                        GlassButton.plain(
                          label: '${ancestor.api.kind} ${ancestor.name} ›',
                          onPressed: () {
                            if (!_current) return;
                            final navigator = Navigator.of(context);
                            for (var n = index; n < widget.ancestors.length && navigator.canPop(); n++) {
                              navigator.pop(_changed);
                            }
                          },
                        ),
                    ],
                  ),
                ),
              if (!widget.embedded) KubeWriteBanner(session: widget.session, contextName: widget.contextName),
              if (detail != null) _actions(detail, canWrite: canWrite, masked: masked),
              for (final notice in [
                if (widget.resource.api.isSecret) (l.kubeSecretRedacted, false),
                if (masked) (l.kubeMaskedEditHelp, false),
                if (_failed) (l.kubeRequestFailed, true),
                if (_mutationFailed) (l.kubeApplyFailed, true),
              ])
                Padding(
                  padding: const EdgeInsets.fromLTRB(GlassSpacing.s16, GlassSpacing.s4, GlassSpacing.s16, 0),
                  child: Text(
                    notice.$1,
                    style: tokens.typography.callout.copyWith(color: notice.$2 ? p.danger : tokens.secondaryLabel),
                  ),
                ),
              if (_loading)
                const Padding(padding: EdgeInsets.only(top: 6), child: LinearProgressIndicator(minHeight: 2)),
              if (!AppPlatform.isMobile && _gitOpsResource != null)
                Padding(
                  padding: const EdgeInsets.fromLTRB(GlassSpacing.s16, GlassSpacing.s8, GlassSpacing.s16, 0),
                  child: _gitOpsActionBar(canWrite),
                ),
              if (detail != null) ...[
                const SizedBox(height: GlassSpacing.s8),
                TabBar(
                  controller: _tabs,
                  isScrollable: true,
                  tabAlignment: TabAlignment.start,
                  labelColor: p.accent,
                  unselectedLabelColor: tokens.secondaryLabel,
                  labelStyle: tokens.typography.bodyEmph,
                  unselectedLabelStyle: tokens.typography.body,
                  indicatorColor: p.accent,
                  indicatorSize: TabBarIndicatorSize.label,
                  dividerColor: tokens.surfaces.separator,
                  labelPadding: const EdgeInsets.symmetric(horizontal: 12),
                  padding: const EdgeInsets.symmetric(horizontal: 4),
                  tabs: [
                    Tab(height: 36, text: l.kubeOverview),
                    Tab(height: 36, key: const ValueKey('kube-yaml-tab'), text: l.kubeObjectYaml),
                    if (_hasLogs) Tab(height: 36, key: const ValueKey('kube-logs-tab'), text: l.kubeLogs),
                    Tab(height: 36, key: const ValueKey('kube-events-tab'), text: l.kubeEvents),
                    Tab(height: 36, key: const ValueKey('kube-related-tab'), text: l.kubeRelatedObjects),
                  ],
                ),
                Expanded(
                  child: TabBarView(
                    controller: _tabs,
                    children: [
                      _overview(detail),
                      SingleChildScrollView(
                        key: const ValueKey('kube-resource-yaml'),
                        padding: const EdgeInsets.all(GlassSpacing.s16),
                        child: SelectableText(detail.yaml, style: tokens.typography.mono.copyWith(fontSize: 12)),
                      ),
                      if (_hasLogs)
                        Padding(
                          padding: const EdgeInsets.fromLTRB(GlassSpacing.s12, GlassSpacing.s8, GlassSpacing.s12, 0),
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.stretch,
                            children: [
                              if (widget.onPinLogs != null)
                                Align(
                                  alignment: Alignment.centerLeft,
                                  child: GlassButton.plain(
                                    key: const ValueKey('kube-pin-logs'),
                                    label: l.kubePinLogs,
                                    icon: Icons.push_pin_outlined,
                                    onPressed: () {
                                      final callback = widget.onPinLogs!;
                                      if (!widget.embedded) closeDialog(context, _changed);
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
                        ),
                      SingleChildScrollView(
                        padding: const EdgeInsets.all(GlassSpacing.s16),
                        child: _relations(detail, events: true, links: false, metrics: false),
                      ),
                      SingleChildScrollView(
                        padding: const EdgeInsets.all(GlassSpacing.s16),
                        child: _relations(detail, events: false, links: true, metrics: false),
                      ),
                    ],
                  ),
                ),
              ],
              if (AppPlatform.isMobile && _gitOpsResource != null) _gitOpsActionBar(canWrite),
            ],
          );
    final shortcuts = <ShortcutActivator, VoidCallback>{
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
    };
    if (widget.embedded) {
      return CallbackShortcuts(
        bindings: shortcuts,
        child: Focus(
          child: ContentSurface(
            key: const ValueKey('kube-resource-details'),
            kind: ContentSurfaceKind.solid,
            padding: EdgeInsets.zero,
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                header,
                Expanded(child: body),
              ],
            ),
          ),
        ),
      );
    }
    return CallbackShortcuts(
      bindings: shortcuts,
      child: Focus(
        autofocus: true,
        child: GlassDialog(
          key: const ValueKey('kube-resource-details'),
          width: 980,
          title: '${widget.resource.api.kind} · ${widget.resource.name}',
          content: SizedBox(
            height: AppPlatform.isMobile && MediaQuery.sizeOf(context).width < 720 ? double.infinity : 600,
            child: body,
          ),
          primaryAction: GlassButton(label: l.commonClose, onPressed: _mutating ? null : () => _close()),
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

  /// One row of object actions (drawer toolbar).
  Widget _actions(KubeResourceDetail detail, {required bool canWrite, required bool masked}) {
    final l = context.l10n;
    final metadata = detail.object['metadata'] as Map?;
    final pod = widget.resource.api.isPod;
    final forwardable =
        !pod &&
        widget.service is KubeForwardTargetsService &&
        widget.service is KubeForwardService &&
        const [
          'Service',
          'Deployment',
          'StatefulSet',
          'DaemonSet',
          'ReplicaSet',
          'Job',
        ].contains(widget.resource.api.kind);
    Widget action(String key, String label, IconData icon, VoidCallback? onPressed, {String? tooltip}) => Tooltip(
      message: tooltip ?? label,
      child: GlassButton(key: ValueKey(key), label: label, icon: icon, size: GlassControlSize.md, onPressed: onPressed),
    );
    return Padding(
      padding: const EdgeInsets.fromLTRB(GlassSpacing.s16, GlassSpacing.s12, GlassSpacing.s16, 0),
      child: Wrap(
        spacing: GlassSpacing.s6,
        runSpacing: GlassSpacing.s6,
        crossAxisAlignment: WrapCrossAlignment.center,
        children: [
          if (_hasLogs) action('kube-open-logs', l.kubeLogs, Icons.article_outlined, () => _tabs.animateTo(2)),
          if (pod)
            action(
              'kube-pod-terminal',
              l.kubeShellShort,
              Icons.terminal,
              widget.session.canExec ? _terminal : null,
              tooltip: widget.session.canExec ? l.kubePodTerminal : l.kubeExecProductionBlocked,
            ),
          if (pod && widget.service is KubeForwardService)
            action(
              'kube-pod-forward',
              l.kubeForwardShort,
              Icons.swap_horiz,
              widget.session.canPortForward
                  ? () => showKubeForwardDialog(
                      context,
                      session: widget.session,
                      resource: widget.resource,
                      service: widget.service as KubeForwardService,
                      detail: detail,
                    )
                  : null,
              tooltip: widget.session.canPortForward ? l.kubePortForward : l.kubeForwardProductionBlocked,
            ),
          if (forwardable)
            action(
              'kube-resource-forward',
              l.kubeForwardShort,
              Icons.swap_horiz,
              widget.session.canPortForward
                  ? () => showKubeResourceForwardDialog(
                      context,
                      session: widget.session,
                      resource: widget.resource,
                      service: widget.service,
                      detail: detail,
                    )
                  : null,
              tooltip: widget.session.canPortForward ? l.kubePortForward : l.kubeForwardProductionBlocked,
            ),
          if (widget.onExplain != null)
            action('kube-resource-ai', l.kubeAiShort, Icons.auto_awesome, () => _explain(widget.resource, null)),
          if (widget.service is KubeRolloutService &&
              widget.resource.api.group == 'apps' &&
              const ['Deployment', 'StatefulSet', 'DaemonSet'].contains(widget.resource.api.kind))
            action(
              'kube-rollout-history',
              l.kubeHistoryShort,
              Icons.history,
              () => showAppDialog<void>(
                context,
                // Browsing view: regular glass in the app palette.
                builder: (_) => KubeRolloutDialog(
                  sessionId: widget.session.id,
                  resource: widget.resource,
                  service: widget.service as KubeRolloutService,
                ),
              ),
              tooltip: l.kubeRolloutHistory,
            ),
          if (widget.resource.api.isSecret && widget.service is KubeManifestService)
            action(
              'kube-open-secret',
              l.kubeRevealShort,
              Icons.visibility_outlined,
              () => showAppDialog<void>(
                context,
                secure: true,
                builder: (_) => KubeSecretDialog(
                  session: widget.session,
                  resource: widget.resource,
                  service: widget.service as KubeManifestService,
                ),
              ),
              tooltip: l.kubeRevealSecret,
            ),
          if (widget.service is KubeMutationService && !widget.resource.api.isSecret)
            action(
              'kube-edit-yaml',
              l.kubeEditShort,
              Icons.edit_outlined,
              canWrite && !_mutating && !masked ? _edit : null,
              tooltip: canWrite ? l.kubeEditYaml : l.kubeReadOnly,
            ),
          if (widget.service is KubeMutationService &&
              metadata?['uid'] is String &&
              metadata?['resourceVersion'] is String)
            GlassIconButton(
              key: const ValueKey('kube-delete-resource'),
              icon: Icons.delete_outline,
              tooltip: canWrite ? l.commonDelete : l.kubeReadOnly,
              color: canWrite ? GlassTokens.of(context).palette.danger : null,
              onPressed: canWrite && !_mutating ? _delete : null,
            ),
        ],
      ),
    );
  }

  Widget _gitOpsActionBar(bool canWrite) {
    final l = context.l10n, resource = _gitOpsResource!;
    return Wrap(
      key: ValueKey(AppPlatform.isMobile ? 'kube-gitops-bottom-actions' : 'kube-gitops-top-actions'),
      spacing: GlassSpacing.s6,
      runSpacing: GlassSpacing.s6,
      children: [
        for (final action in kubeGitOpsActions(resource))
          GlassButton(
            key: ValueKey('kube-gitops-detail-$action'),
            label: kubeGitOpsActionLabel(l, action),
            onPressed: canWrite && !_mutating ? () => _gitOpsAction(action) : null,
          ),
        GlassButton(
          label: l.kubeLogs,
          icon: Icons.article_outlined,
          onPressed: () {
            if (_current) unawaited(_gitOpsDetailsKey.currentState?.openControllerLogs());
          },
        ),
        GlassButton(label: l.kubeGitOpsFlow, icon: Icons.account_tree_outlined, onPressed: _gitOpsGraph),
      ],
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
    final object = detail.object;
    final metadata = object['metadata'] as Map? ?? const {};
    final spec = object['spec'] as Map? ?? const {};
    final status = object['status'] as Map? ?? const {};
    final conditions = (status['conditions'] as List? ?? const []).whereType<Map<Object?, Object?>>().toList();
    final labels = metadata['labels'] as Map? ?? const {};
    final annotations = Map<Object?, Object?>.fromEntries(
      (metadata['annotations'] as Map? ?? const {}).entries.where(
        (entry) => entry.key != 'kubectl.kubernetes.io/last-applied-configuration',
      ),
    );
    final owners = (metadata['ownerReferences'] as List? ?? const []).whereType<Map<String, dynamic>>().toList();
    String text(Object? value) => value is String || value is num || value is bool ? value.toString() : '';
    final pod = widget.resource.api.isPod;
    final restarts = (status['containerStatuses'] as List? ?? const [])
        .whereType<Map<Object?, Object?>>()
        .map((c) => c['restartCount'])
        .whereType<num>()
        .fold<num>(0, (sum, count) => sum + count);
    final workload =
        widget.resource.api.group == 'apps' ||
        const ['Job', 'CronJob', 'ReplicationController'].contains(widget.resource.api.kind);
    final info = <(String, String)>[
      if (widget.resource.namespace != null) (l.kubeNamespace, widget.resource.namespace!),
      (l.kubeCreatedAt, text(metadata['creationTimestamp'])),
      if (pod) ...[
        (l.kubeNode, text(spec['nodeName'])),
        (l.kubePodIp, text(status['podIP'])),
        (l.kubeHostIp, text(status['hostIP'])),
        (l.kubeQos, text(status['qosClass'])),
        (l.kubeServiceAccount, text(spec['serviceAccountName'])),
        (l.kubeRestarts, '$restarts'),
      ],
      if (!pod && status['replicas'] is num)
        (l.kubeReplicasReady, '${status['readyReplicas'] ?? 0}/${spec['replicas'] ?? status['replicas']}'),
      if (!pod && text(status['phase']).isNotEmpty) (l.kubeStatus, text(status['phase'])),
      if (owners.isNotEmpty) (l.kubeControlledBy, owners.map((o) => '${o['kind']}/${o['name']}').join(', ')),
    ];
    return ListView(
      key: const ValueKey('kube-resource-overview'),
      padding: const EdgeInsets.all(GlassSpacing.s16),
      children: [
        KubeSection(
          title: l.kubeDetailsSection,
          child: KubeInfoGrid(entries: info),
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
              widget.onChanged?.call();
              unawaited(_load());
            },
          ),
        if (pod || workload)
          KubeSection(
            title: l.kubeContainers,
            child: KubeContainers(resource: widget.resource, object: object),
          ),
        if (widget.service is KubeOperationsService ||
            widget.service is KubeMetricsService ||
            widget.service is KubeRelationsService)
          KubeSection(title: l.kubeUsage, child: _relations(detail, events: false, links: false, metrics: true)),
        if (labels.isNotEmpty)
          KubeSection(
            title: l.kubeLabels,
            child: KubeLabelChips(values: labels),
          ),
        if (annotations.isNotEmpty)
          KubeSection(
            title: l.kubeAnnotations,
            child: KubeLabelChips(values: annotations, limit: 12),
          ),
        if (conditions.isNotEmpty)
          KubeSection(
            title: l.kubeConditions,
            child: KubeConditions(conditions: conditions),
          ),
        if (!pod)
          KubeSection(
            title: l.kubeSpecSection,
            child: KubeResourceSummary(resource: widget.resource, detail: detail),
          ),
        if (widget.service is KubeOperationsService)
          KubeSection(
            title: l.kubeOperations,
            child: KubeResourceOperations(
              session: widget.session,
              resource: widget.resource,
              detail: detail,
              service: widget.service as KubeOperationsService,
              onChanged: () {
                _changed = true;
                widget.onChanged?.call();
                unawaited(_load());
              },
            ),
          ),
        if (status.isNotEmpty && conditions.isEmpty && status['phase'] == null && !workload)
          KubeSection(
            title: l.kubeStatus,
            child: SelectableText(
              const JsonEncoder.withIndent('  ').convert(status),
              style: GlassTokens.of(context).typography.mono.copyWith(fontSize: 12),
            ),
          ),
      ],
    );
  }
}
