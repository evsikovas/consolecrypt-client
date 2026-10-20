import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_helm_cli_service.dart';
import 'package:consolecrypt/kubernetes/kube_helm_rollback.dart';
import 'package:consolecrypt/kubernetes/kube_insights_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_write_access.dart';
import 'package:consolecrypt/kubernetes/kube_yaml_editor.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class KubeHelmPanel extends ConsumerStatefulWidget {
  const KubeHelmPanel({
    required this.session,
    required this.contextName,
    required this.service,
    this.namespace,
    this.releaseName,
    super.key,
  });
  final KubeSession session;
  final String contextName;
  final KubeInsightsService service;
  final String? namespace, releaseName;
  @override
  ConsumerState<KubeHelmPanel> createState() => _KubeHelmPanelState();
}

class _KubeHelmPanelState extends ConsumerState<KubeHelmPanel> {
  late final KubeScope _scope;
  List<KubeHelmSummary> _releases = const [];
  bool _loading = true, _failed = false, _allRevisions = false;
  String _filter = '';
  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(kubeScopeProvider));
  @override
  void initState() {
    super.initState();
    _scope = ref.read(kubeScopeProvider);
    unawaited(_load());
  }

  @override
  void dispose() {
    _releases = const [];
    super.dispose();
  }

  Future<void> _load() async {
    if (!_current) return;
    setState(() {
      _loading = true;
      _failed = false;
    });
    try {
      final releases = await widget.service.helmHistory(widget.session.id, namespace: widget.namespace);
      if (_current) {
        setState(
          () => _releases = widget.releaseName == null
              ? releases
              : releases.where((release) => release.name == widget.releaseName).toList(),
        );
      }
    } catch (_) {
      if (_current) setState(() => _failed = true);
    } finally {
      if (_current) setState(() => _loading = false);
    }
  }

  Future<void> _open(KubeHelmSummary release) async {
    if (!_current) return;
    await showAppDialog<void>(
      context,
      secure: true,
      builder: (_) => _HelmDetails(
        session: widget.session,
        contextName: widget.contextName,
        service: widget.service,
        selected: release,
        history: _releases.where((other) => other.namespace == release.namespace && other.name == release.name).toList()
          ..sort((a, b) => b.revision.compareTo(a.revision)),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    if (!_current) return const SizedBox.shrink();
    final latest = <String, KubeHelmSummary>{};
    for (final release in _releases) {
      final key = '${release.namespace}/${release.name}';
      if ((latest[key]?.revision ?? 0) < release.revision) latest[key] = release;
    }
    final releases =
        (_allRevisions ? _releases : latest.values)
            .where(
              (release) => '${release.name} ${release.namespace} ${release.chart} ${release.status}'
                  .toLowerCase()
                  .contains(_filter),
            )
            .toList()
          ..sort((a, b) {
            final order = '${a.namespace}/${a.name}'.compareTo('${b.namespace}/${b.name}');
            return order == 0 ? b.revision.compareTo(a.revision) : order;
          });
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(
          children: [
            Expanded(
              child: TextField(
                key: const ValueKey('kube-helm-search'),
                decoration: InputDecoration(labelText: l.commonSearch, prefixIcon: const Icon(Icons.search)),
                onChanged: (value) => setState(() => _filter = value.toLowerCase()),
              ),
            ),
            GlassIconButton(
              key: const ValueKey('kube-helm-refresh'),
              tooltip: l.kubeRefreshResources,
              icon: Icons.refresh,
              onPressed: _loading ? null : _load,
            ),
          ],
        ),
        Align(
          alignment: Alignment.centerLeft,
          child: FilterChip(
            key: const ValueKey('kube-helm-history'),
            label: Text(l.kubeHelmAllRevisions),
            selected: _allRevisions,
            onSelected: (value) => setState(() => _allRevisions = value),
          ),
        ),
        if (_loading) const LinearProgressIndicator(),
        if (_failed) Text(l.kubeRequestFailed, style: TextStyle(color: Theme.of(context).colorScheme.error)),
        if (!_loading && !_failed && releases.isEmpty) Text(l.kubeHelmEmpty),
        Expanded(
          child: ListView.builder(
            key: const ValueKey('kube-helm-list'),
            itemCount: releases.length,
            itemBuilder: (context, index) {
              final release = releases[index];
              return ContentSurface(
                child: ListTile(
                  key: ValueKey('kube-helm-${release.id}'),
                  title: Text(release.name),
                  subtitle: Text(
                    '${release.namespace} · ${release.status}\n${release.chart} ${release.chartVersion} · ${l.kubeHelmRevision(release.revision)}',
                  ),
                  trailing: const Icon(Icons.chevron_right),
                  onTap: () => _open(release),
                ),
              );
            },
          ),
        ),
      ],
    );
  }
}

class _HelmDetails extends ConsumerStatefulWidget {
  const _HelmDetails({
    required this.session,
    required this.contextName,
    required this.service,
    required this.selected,
    required this.history,
  });
  final KubeSession session;
  final String contextName;
  final KubeInsightsService service;
  final KubeHelmSummary selected;
  final List<KubeHelmSummary> history;
  @override
  ConsumerState<_HelmDetails> createState() => _HelmDetailsState();
}

class _HelmDetailsState extends ConsumerState<_HelmDetails> with SingleTickerProviderStateMixin {
  late final KubeScope _scope;
  late final TabController _tabs;
  late KubeHelmSummary _selected;
  KubeHelmRelease? _release, _comparison;
  KubeHelmSummary? _compare;
  bool _loading = true, _failed = false;
  int _generation = 0;
  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(kubeScopeProvider));
  @override
  void initState() {
    super.initState();
    _scope = ref.read(kubeScopeProvider);
    _selected = widget.selected;
    _tabs = TabController(length: 5, vsync: this);
    unawaited(_load());
  }

  @override
  void dispose() {
    _generation++;
    _release = null;
    _comparison = null;
    _tabs.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    if (!_current) return;
    final generation = ++_generation;
    setState(() {
      _loading = true;
      _failed = false;
      _release = null;
      _comparison = null;
    });
    try {
      final release = await widget.service.helmRelease(widget.session.id, _selected.namespace, _selected.secretName);
      final compare = _compare;
      final comparison = compare == null
          ? null
          : await widget.service.helmRelease(widget.session.id, compare.namespace, compare.secretName);
      if (_current && generation == _generation) {
        setState(() {
          _release = release;
          _comparison = comparison;
        });
      }
    } catch (_) {
      if (_current && generation == _generation) setState(() => _failed = true);
    } finally {
      if (_current && generation == _generation) setState(() => _loading = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(kubeScopeProvider, (_, next) {
      if (identical(next, _scope)) return;
      _generation++;
      _release = null;
      _comparison = null;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted) return;
        final route = ModalRoute.of(context);
        if (route?.isActive == true) Navigator.of(context).removeRoute(route!);
      });
    });
    final l = context.l10n;
    final release = _release;
    Widget text(String value) =>
        SingleChildScrollView(child: SelectableText(value, style: GlassTokens.of(context).typography.mono));
    return GlassDialog(
      key: const ValueKey('kube-helm-details'),
      width: 1000,
      title: '${_selected.name} · ${_selected.namespace}',
      content: SizedBox(
        height: 620,
        child: !_current
            ? const SizedBox.shrink()
            : Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  KubeWriteBanner(session: widget.session, contextName: widget.contextName),
                  Text(l.kubeHelmSecretsMasked),
                  const SizedBox(height: 8),
                  DropdownButtonFormField<String>(
                    key: const ValueKey('kube-helm-revision'),
                    initialValue: _selected.id,
                    isExpanded: true,
                    decoration: InputDecoration(labelText: l.kubeHelmHistory),
                    items: widget.history
                        .map(
                          (entry) => DropdownMenuItem(
                            value: entry.id,
                            child: Text(
                              '${l.kubeHelmRevision(entry.revision)} · ${entry.status}',
                              overflow: TextOverflow.ellipsis,
                            ),
                          ),
                        )
                        .toList(),
                    onChanged: _loading
                        ? null
                        : (id) {
                            if (id == null) return;
                            _selected = widget.history.firstWhere((entry) => entry.id == id);
                            if (_compare?.id == id) _compare = null;
                            unawaited(_load());
                          },
                  ),
                  if (_loading) const LinearProgressIndicator(),
                  if (_failed) Text(l.kubeRequestFailed, style: TextStyle(color: Theme.of(context).colorScheme.error)),
                  if (release != null) ...[
                    TabBar(
                      controller: _tabs,
                      isScrollable: true,
                      tabAlignment: TabAlignment.start,
                      tabs: [
                        Tab(text: l.kubeHelmValues),
                        Tab(text: l.kubeHelmChartValues),
                        Tab(text: l.kubeHelmComputedValues),
                        Tab(text: l.kubeHelmManifest),
                        Tab(key: const ValueKey('kube-helm-diff-tab'), text: l.kubeReviewChanges),
                      ],
                    ),
                    const SizedBox(height: 8),
                    Expanded(
                      child: TabBarView(
                        controller: _tabs,
                        children: [
                          text(const JsonEncoder.withIndent('  ').convert(release.values)),
                          text(const JsonEncoder.withIndent('  ').convert(release.chartValues)),
                          Column(
                            crossAxisAlignment: CrossAxisAlignment.stretch,
                            children: [
                              if (release.computedValuesPartial) Text(l.kubeHelmComputedPartial),
                              Expanded(child: text(const JsonEncoder.withIndent('  ').convert(release.computedValues))),
                            ],
                          ),
                          text(release.manifest),
                          Column(
                            crossAxisAlignment: CrossAxisAlignment.stretch,
                            children: [
                              DropdownButtonFormField<String>(
                                key: const ValueKey('kube-helm-compare'),
                                initialValue: _compare?.id ?? '',
                                isExpanded: true,
                                decoration: InputDecoration(labelText: l.kubeHelmCompare),
                                items: [
                                  DropdownMenuItem(value: '', child: Text(l.kubeHelmChooseRevision)),
                                  ...widget.history
                                      .where((entry) => entry.id != _selected.id)
                                      .map(
                                        (entry) => DropdownMenuItem(
                                          value: entry.id,
                                          child: Text(l.kubeHelmRevision(entry.revision)),
                                        ),
                                      ),
                                ],
                                onChanged: (id) {
                                  _compare = id == null || id.isEmpty
                                      ? null
                                      : widget.history.firstWhere((entry) => entry.id == id);
                                  unawaited(_load());
                                },
                              ),
                              const SizedBox(height: 8),
                              if (_comparison != null)
                                Expanded(
                                  child: KubeYamlDiff(
                                    before: _comparison!.manifest,
                                    after: release.manifest,
                                    explanation: l.kubeHelmDiffHelp,
                                  ),
                                ),
                            ],
                          ),
                        ],
                      ),
                    ),
                  ],
                ],
              ),
      ),
      primaryAction: GlassButton(label: l.commonClose, onPressed: () => closeDialog<void>(context)),
      secondaryActions: [
        if (!AppPlatform.isMobile &&
            widget.service is KubeHelmCliService &&
            widget.history.any((revision) => revision.revision > _selected.revision))
          GlassButton.destructive(
            key: const ValueKey('kube-helm-rollback'),
            label: l.kubeHelmRollback,
            onPressed: !_loading && (ref.watch(kubeWriteAccessProvider(widget.session)).value?.canWrite ?? false)
                ? () async {
                    final changed = await showAppDialog<bool>(
                      context,
                      secure: true,
                      barrierDismissible: false,
                      builder: (_) => KubeHelmRollbackDialog(
                        session: widget.session,
                        release: _selected,
                        service: widget.service as KubeHelmCliService,
                      ),
                    );
                    if (_current && changed == true) await _load();
                  }
                : null,
          ),
      ],
    );
  }
}
