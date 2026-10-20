import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_helm_cli_service.dart';
import 'package:consolecrypt/kubernetes/kube_helm_comparison.dart';
import 'package:consolecrypt/kubernetes/kube_helm_comparison_service.dart';
import 'package:consolecrypt/kubernetes/kube_helm_rollback.dart';
import 'package:consolecrypt/kubernetes/kube_insights_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_resource_table_view.dart';
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
      // Browsing view: regular glass in the app palette.
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
    final tokens = GlassTokens.of(context);
    final p = tokens.palette;
    final header = tokens.typography.caption.copyWith(
      color: tokens.secondaryLabel,
      fontWeight: FontWeight.w600,
      letterSpacing: 0.3,
    );
    const widths = [240.0, 150.0, 80.0, 130.0, 260.0, 130.0, 100.0];
    String updated(KubeHelmSummary release) {
      final time = DateTime.tryParse(release.updatedAt ?? '');
      if (time == null) return '—';
      final age = DateTime.now().toUtc().difference(time.toUtc());
      if (age.inDays > 0) return '${age.inDays}d'; // l10n-ignore: Kubernetes duration.
      if (age.inHours > 0) return '${age.inHours}h'; // l10n-ignore: Kubernetes duration.
      return '${age.inMinutes.clamp(0, 59)}m'; // l10n-ignore: Kubernetes duration.
    }

    Widget cell(double width, Widget child) => SizedBox(
      width: width,
      child: Padding(padding: const EdgeInsets.symmetric(horizontal: 10), child: child),
    );
    Text text(String value, {bool strong = false}) => Text(
      value,
      maxLines: 1,
      overflow: TextOverflow.ellipsis,
      style: (strong ? tokens.typography.body : tokens.typography.callout).copyWith(
        color: strong ? p.label : tokens.secondaryLabel,
        fontWeight: strong ? FontWeight.w500 : null,
      ),
    );
    if (AppPlatform.isMobile || MediaQuery.sizeOf(context).width < 600) return _compactList(context, releases);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(
          children: [
            SizedBox(
              width: 260,
              child: GlassField(
                key: const ValueKey('kube-helm-search'),
                fieldKey: const ValueKey('kube-helm-search-input'),
                search: true,
                placeholder: l.commonSearch,
                onChanged: (value) => setState(() => _filter = value.toLowerCase()),
              ),
            ),
            const SizedBox(width: GlassSpacing.s8),
            GlassButton.plain(
              key: const ValueKey('kube-helm-history'),
              label: l.kubeHelmAllRevisions,
              icon: _allRevisions ? Icons.check_box_rounded : Icons.check_box_outline_blank_rounded,
              onPressed: () => setState(() => _allRevisions = !_allRevisions),
            ),
            const Spacer(),
            GlassBadge(label: '${releases.length}', dense: true),
            GlassIconButton(
              key: const ValueKey('kube-helm-refresh'),
              tooltip: l.kubeRefreshResources,
              icon: Icons.refresh,
              onPressed: _loading ? null : _load,
            ),
          ],
        ),
        const SizedBox(height: GlassSpacing.s8),
        if (_failed)
          Padding(
            padding: const EdgeInsets.only(bottom: GlassSpacing.s6),
            child: Text(l.kubeRequestFailed, style: tokens.typography.callout.copyWith(color: p.danger)),
          ),
        Expanded(
          child: ContentSurface(
            padding: const EdgeInsets.symmetric(vertical: 4),
            child: LayoutBuilder(
              builder: (context, constraints) => SingleChildScrollView(
                scrollDirection: Axis.horizontal,
                child: SizedBox(
                  width: constraints.maxWidth > widths.fold(24.0, (a, b) => a + b)
                      ? constraints.maxWidth
                      : widths.fold<double>(24, (a, b) => a + b),
                  child: Column(
                    children: [
                      SizedBox(
                        height: 32,
                        child: Row(
                          children: [
                            const SizedBox(width: 12),
                            for (final (index, label) in [
                              l.kubeName,
                              l.kubeNamespace,
                              l.kubeRevision,
                              l.kubeStatus,
                              'Chart', // l10n-ignore: Helm term.
                              l.kubeAppVersion,
                              l.kubeUpdated,
                            ].indexed)
                              cell(widths[index], Text(label.toUpperCase(), style: header)),
                          ],
                        ),
                      ),
                      Divider(height: 1, color: tokens.surfaces.separator),
                      if (_loading) const LinearProgressIndicator(minHeight: 2),
                      if (!_loading && !_failed && releases.isEmpty)
                        Padding(
                          padding: const EdgeInsets.all(24),
                          child: Text(
                            l.kubeHelmEmpty,
                            style: tokens.typography.body.copyWith(color: tokens.secondaryLabel),
                          ),
                        ),
                      Expanded(
                        child: ListView.builder(
                          key: const ValueKey('kube-helm-list'),
                          itemExtent: 34,
                          itemCount: releases.length,
                          itemBuilder: (context, index) {
                            final release = releases[index];
                            return GlassInteractive(
                              key: ValueKey('kube-helm-${release.id}'),
                              semanticLabel: release.name,
                              onPressed: () => _open(release),
                              builder: (context, state) => ColoredBox(
                                color: state.hovered ? tokens.surfaces.fillHover : Colors.transparent,
                                child: SizedBox(
                                  height: 34,
                                  child: Row(
                                    children: [
                                      const SizedBox(width: 12),
                                      cell(widths[0], text(release.name, strong: true)),
                                      cell(widths[1], text(release.namespace)),
                                      cell(widths[2], text('${release.revision}')),
                                      cell(widths[3], KubeStatusLabel(status: release.status)),
                                      cell(widths[4], text('${release.chart}-${release.chartVersion}')),
                                      cell(widths[5], text(release.appVersion)),
                                      cell(widths[6], text(updated(release))),
                                    ],
                                  ),
                                ),
                              ),
                            );
                          },
                        ),
                      ),
                    ],
                  ),
                ),
              ),
            ),
          ),
        ),
      ],
    );
  }

  /// Phones: one card per release with the essential facts.
  Widget _compactList(BuildContext context, List<KubeHelmSummary> releases) {
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(
          children: [
            Expanded(
              child: GlassField(
                key: const ValueKey('kube-helm-search'),
                fieldKey: const ValueKey('kube-helm-search-input'),
                search: true,
                placeholder: l.commonSearch,
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
          child: GlassButton.plain(
            key: const ValueKey('kube-helm-history'),
            label: l.kubeHelmAllRevisions,
            icon: _allRevisions ? Icons.check_box_rounded : Icons.check_box_outline_blank_rounded,
            onPressed: () => setState(() => _allRevisions = !_allRevisions),
          ),
        ),
        if (_loading) const LinearProgressIndicator(minHeight: 2),
        if (_failed) Text(l.kubeRequestFailed, style: tokens.typography.callout.copyWith(color: tokens.palette.danger)),
        if (!_loading && !_failed && releases.isEmpty) Text(l.kubeHelmEmpty),
        Expanded(
          child: ContentList(
            key: const ValueKey('kube-helm-list'),
            itemCount: releases.length,
            itemBuilder: (context, index) {
              final release = releases[index];
              return ListTile(
                key: ValueKey('kube-helm-${release.id}'),
                title: Text(release.name),
                subtitle: Text(
                  '${release.namespace} · ${release.status}\n${release.chart} ${release.chartVersion} · ${l.kubeHelmRevision(release.revision)}',
                ),
                trailing: const Icon(Icons.chevron_right),
                onTap: () => _open(release),
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
    _tabs = TabController(length: widget.service is KubeHelmComparisonService ? 6 : 5, vsync: this);
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
                        if (widget.service is KubeHelmComparisonService) Tab(text: l.kubeHelmCompareLive),
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
                          if (widget.service is KubeHelmComparisonService)
                            KubeHelmComparisonPanel(
                              session: widget.session,
                              namespace: _selected.namespace,
                              secretName: _selected.secretName,
                              service: widget.service as KubeHelmComparisonService,
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
