import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/monitoring/monitoring_binding_models.dart';
import 'package:consolecrypt/monitoring/monitoring_binding_reader.dart' show monitoringMetricLabel;
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_result_view.dart';
import 'package:consolecrypt/monitoring/monitoring_saved_panel_models.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window.dart';
import 'package:flutter/foundation.dart' show listEquals;
import 'package:material_ui/material_ui.dart';

/// The primary view renders built-in host graphs and saved custom graphs.
/// Configuration stays in the advanced workspace; every read keeps its source.
class MonitoringDashboard extends StatefulWidget {
  const MonitoringDashboard({
    super.key,
    required this.service,
    required this.store,
    required this.isCurrent,
    this.requestedSourceId,
    this.requestedHostId,
    this.requestedBindingIds = const [],
    this.active = true,
    this.onConfigure,
    this.onLinkHost,
    this.onBrowseSource,
    this.keepHostSelection = false,
  });
  final MonitoringService service;
  final MonitoringSourceStore store;
  final bool Function() isCurrent;
  final String? requestedSourceId;
  final String? requestedHostId;
  final List<String> requestedBindingIds;
  final bool active;
  final VoidCallback? onConfigure, onLinkHost;
  final ValueChanged<MonitoringSource>? onBrowseSource;
  final bool keepHostSelection;
  @override
  State<MonitoringDashboard> createState() => _MonitoringDashboardState();
}

class _MonitoringDashboardState extends State<MonitoringDashboard> {
  List<MonitoringSource> sources = [];
  List<SavedMonitoringPanel> panels = [];
  List<MonitoringBinding> bindings = [];
  final builtinResults = <String, List<BindingPanelQuery>>{};
  final builtinFailures = <String, Object>{};
  final dockerBindings = <String, bool>{};
  final results = <String, SavedPanelRead>{};
  final failures = <String, Object>{};
  MonitoringTimeWindow? window;
  String? selected;
  Object? error;
  bool loading = true, reading = false;
  int generation = 0;
  Timer? timer;
  MonitoringSource? get source => sources.where((s) => s.id == selected).firstOrNull;
  bool get focusedHost =>
      widget.requestedHostId != null &&
      (widget.keepHostSelection || widget.requestedSourceId == null || selected == widget.requestedSourceId);
  bool bindingMatches(MonitoringBinding binding, MonitoringSource original) =>
      binding.sourceId == original.id &&
      binding.sourceRevision == original.revision &&
      binding.state == MonitoringBindingState.current &&
      (!focusedHost ||
          binding.hostId == widget.requestedHostId &&
              (widget.requestedBindingIds.isEmpty || widget.requestedBindingIds.contains(binding.id)));
  bool current(int token) => mounted && token == generation && widget.active && widget.isCurrent();
  @override
  void initState() {
    super.initState();
    unawaited(load());
  }

  @override
  void didUpdateWidget(MonitoringDashboard oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!widget.active) {
      generation++;
      timer?.cancel();
      widget.service.cancel();
      reading = false;
      return;
    }
    if (!identical(oldWidget.service, widget.service) ||
        !identical(oldWidget.store, widget.store) ||
        oldWidget.requestedSourceId != widget.requestedSourceId ||
        oldWidget.requestedHostId != widget.requestedHostId ||
        oldWidget.keepHostSelection != widget.keepHostSelection ||
        !listEquals(oldWidget.requestedBindingIds, widget.requestedBindingIds) ||
        !oldWidget.active) {
      unawaited(load());
    }
  }

  Future<void> load() async {
    final token = ++generation;
    timer?.cancel();
    widget.service.cancel();
    setState(() {
      loading = true;
      reading = false;
      results.clear();
      failures.clear();
      builtinResults.clear();
      builtinFailures.clear();
      dockerBindings.clear();
      error = null;
    });
    try {
      final loaded = await widget.store.load();
      if (!current(token)) return;
      final store = widget.store;
      final saved = store is MonitoringSavedPanelStore
          ? (store as MonitoringSavedPanelStore).panels
          : <SavedMonitoringPanel>[];
      setState(() {
        panels = List.of(saved);
        bindings = store is MonitoringBindingStore ? List.of((store as MonitoringBindingStore).bindings) : [];
        sources = widget.keepHostSelection
            ? loaded.where((candidate) => bindings.any((b) => bindingMatches(b, candidate))).toList()
            : loaded;
        selected = widget.requestedSourceId != null
            ? sources.where((s) => s.id == widget.requestedSourceId).firstOrNull?.id
            : sources
                      .where(
                        (s) =>
                            (widget.requestedHostId == null && saved.any((p) => p.currentFor(s))) ||
                            bindings.any(
                              (b) =>
                                  b.sourceId == s.id &&
                                  b.sourceRevision == s.revision &&
                                  (widget.requestedHostId == null || b.hostId == widget.requestedHostId) &&
                                  b.state == MonitoringBindingState.current,
                            ),
                      )
                      .firstOrNull
                      ?.id ??
                  sources.firstOrNull?.id;
        if (widget.requestedSourceId != null && selected == null) {
          error = const MonitoringFailure.local(MonitoringErrorCode.sourceChanged);
        }
        loading = false;
      });
      await refresh();
    } catch (failure) {
      if (current(token)) {
        setState(() {
          loading = false;
          error = failure;
        });
      }
    }
  }

  Future<void> refresh() async {
    final original = source, service = widget.service;
    if (reading ||
        original == null ||
        original.readOnly ||
        !original.allowsBackgroundObservation ||
        !widget.active ||
        !widget.isCurrent()) {
      return;
    }
    final token = generation;
    final selectedPanels = focusedHost
        ? <SavedMonitoringPanel>[]
        : panels.where((p) => p.currentFor(original)).take(16).toList();
    final selectedBindings = bindings.where((b) => bindingMatches(b, original)).take(4).toList();
    if ((selectedPanels.isEmpty || service is! MonitoringSavedPanelReader) &&
        (selectedBindings.isEmpty || service is! MonitoringBindingReader)) {
      return;
    }
    timer?.cancel();
    final end = DateTime.now().toUtc().millisecondsSinceEpoch / 1000;
    final range = SavedPanelRange(start: end - 3600, end: end, stepSeconds: 15);
    final next = <String, SavedPanelRead>{}, errors = <String, Object>{};
    final builtin = <String, List<BindingPanelQuery>>{}, bindingErrors = <String, Object>{};
    setState(() {
      reading = true;
    });
    for (final panel in service is MonitoringSavedPanelReader ? selectedPanels : <SavedMonitoringPanel>[]) {
      if (!current(token) || source?.fingerprint != original.fingerprint) return;
      try {
        next[panel.id] = await (service as MonitoringSavedPanelReader).readSavedPanel(original, panel, window: range);
      } catch (failure) {
        errors[panel.id] = failure;
      }
    }
    if (service is MonitoringBindingReader) {
      final reader = service as MonitoringBindingReader;
      for (final binding in selectedBindings) {
        if (!current(token) || source?.fingerprint != original.fingerprint) return;
        try {
          Future<BindingReadResult> read(MonitoringPanel panel) => reader.readPanel(
            original,
            binding,
            panel,
            start: range.start,
            end: range.end,
            stepSeconds: range.stepSeconds,
          );
          final cachedDocker = dockerBindings[binding.id];
          var first = await read(cachedDocker == true ? MonitoringPanel.docker : MonitoringPanel.hostOverview);
          if (!current(token) || source?.fingerprint != original.fingerprint) return;
          if (cachedDocker == null && first.preparationReason == BindingPreparationReason.missingMetrics) {
            first = await read(MonitoringPanel.docker);
          }
          if (!current(token) || source?.fingerprint != original.fingerprint) return;
          final rows = <BindingPanelQuery>[...first.queries];
          if (first.error != null || first.preparationError != null) {
            bindingErrors[binding.id] = first.error ?? first.preparationError!;
          }
          if (first.queries.isNotEmpty) {
            final docker = first.panel == MonitoringPanel.docker;
            dockerBindings[binding.id] = docker;
            if (!docker) {
              for (final panel in [MonitoringPanel.filesystem, MonitoringPanel.diskIo, MonitoringPanel.network]) {
                if (!current(token) || source?.fingerprint != original.fingerprint) return;
                final extra = await read(panel);
                rows.addAll(extra.queries);
                if (extra.error != null || extra.preparationError != null) {
                  bindingErrors[binding.id] = extra.error ?? extra.preparationError!;
                }
              }
            }
          }
          builtin[binding.id] = rows;
        } catch (failure) {
          bindingErrors[binding.id] = failure;
        }
      }
    }
    if (!current(token) || source?.fingerprint != original.fingerprint) return;
    setState(() {
      results
        ..clear()
        ..addAll(next);
      failures
        ..clear()
        ..addAll(errors);
      builtinResults
        ..clear()
        ..addAll(builtin);
      builtinFailures
        ..clear()
        ..addAll(bindingErrors);
      window = MonitoringTimeWindow.fromEpoch(range.start, range.end, range.stepSeconds);
      reading = false;
    });
    timer = Timer(const Duration(seconds: 15), () {
      if (current(token)) unawaited(refresh());
    });
  }

  void choose(String? id) {
    if (id == null || id == selected || !widget.isCurrent() || !sources.any((s) => s.id == id)) return;
    generation++;
    timer?.cancel();
    widget.service.cancel();
    setState(() {
      selected = id;
      error = null;
      reading = false;
      results.clear();
      failures.clear();
      builtinResults.clear();
      builtinFailures.clear();
    });
    unawaited(refresh());
  }

  @override
  void dispose() {
    generation++;
    timer?.cancel();
    widget.service.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n, original = source;
    final automatic = original?.allowsBackgroundObservation ?? false;
    final visible = original == null || !automatic || focusedHost
        ? <SavedMonitoringPanel>[]
        : panels.where((p) => p.currentFor(original)).take(16).toList();
    final visibleBindings = original == null || !automatic
        ? <MonitoringBinding>[]
        : bindings.where((b) => bindingMatches(b, original)).take(4).toList();
    return SingleChildScrollView(
      key: const ValueKey('monitoring-dashboard'),
      padding: const EdgeInsets.all(20),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (loading || reading) const LinearProgressIndicator(),
          if (error != null) ...[
            InfoBanner(message: errorMessage(l, error!), tone: BannerTone.danger),
            Align(
              alignment: Alignment.centerLeft,
              child: TextButton(onPressed: load, child: Text(l.commonRetry)),
            ),
          ],
          if (sources.isNotEmpty)
            Row(
              children: [
                Expanded(
                  child: DropdownButtonFormField<String>(
                    key: const ValueKey('monitoring-dashboard-source'),
                    initialValue: selected,
                    isExpanded: true,
                    items: [
                      for (final s in sources)
                        DropdownMenuItem(
                          value: s.id,
                          child: Text(s.name, overflow: TextOverflow.ellipsis),
                        ),
                    ],
                    onChanged: choose,
                  ),
                ),
                const SizedBox(width: 8),
                IconButton(
                  onPressed: reading || !automatic || original?.readOnly == true ? null : refresh,
                  icon: const Icon(Icons.refresh),
                  tooltip: l.devicesRefresh,
                ),
                if (widget.onLinkHost != null)
                  IconButton(
                    key: const ValueKey('monitoring-host-link-edit'),
                    onPressed: widget.onLinkHost,
                    icon: const Icon(Icons.link),
                    tooltip: l.monitoringChooseTarget,
                  ),
              ],
            ),
          const SizedBox(height: 16),
          if (original != null && automatic && !original.readOnly && widget.onBrowseSource != null)
            Padding(
              padding: const EdgeInsets.only(bottom: 12),
              child: Align(
                alignment: Alignment.centerLeft,
                child: FilledButton.icon(
                  key: const ValueKey('monitoring-explore-source'),
                  onPressed: () => widget.onBrowseSource!(original),
                  icon: const Icon(Icons.show_chart),
                  label: Text(l.monitoringBrowseTitle),
                ),
              ),
            ),
          if (original != null && !automatic) InfoBanner(message: l.monitoringAgentRouteHint, tone: BannerTone.info),
          if (widget.store.readOnly || original?.readOnly == true)
            InfoBanner(message: l.monitoringReadOnly, tone: BannerTone.warning),
          if (!loading && visible.isEmpty && visibleBindings.isEmpty && (original == null || automatic))
            ContentSurface(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(focusedHost
                    ? l.monitoringNoHostMetrics
                    : original != null && widget.onBrowseSource != null
                      ? l.monitoringBrowseEmpty
                      : l.monitoringSetupNoGraphs),
                  const SizedBox(height: 8),
                  if (focusedHost && widget.onLinkHost != null)
                    FilledButton.icon(
                      key: const ValueKey('monitoring-host-link-open'),
                      onPressed: widget.onLinkHost,
                      icon: const Icon(Icons.link),
                      label: Text(l.monitoringChooseTarget),
                    )
                  else if (original == null && widget.onConfigure != null)
                    FilledButton.icon(
                      key: const ValueKey('monitoring-connect-source'),
                      onPressed: widget.onConfigure,
                      icon: const Icon(Icons.add_chart),
                      label: Text(l.monitoringConnectSource),
                    ),
                ],
              ),
            ),
          for (final binding in visibleBindings) ...[
            Text(binding.name, style: Theme.of(context).textTheme.titleLarge),
            const SizedBox(height: 8),
            if (builtinFailures[binding.id] case final failure?)
              InfoBanner(message: errorMessage(l, failure), tone: BannerTone.danger),
            if ((builtinResults[binding.id] ?? []).isEmpty && !builtinFailures.containsKey(binding.id))
              Padding(
                padding: const EdgeInsets.only(bottom: 16),
                child: ContentSurface(child: reading ? const LinearProgressIndicator() : Text(l.monitoringNoData)),
              ),
            for (final query in builtinResults[binding.id] ?? <BindingPanelQuery>[])
              Padding(
                padding: const EdgeInsets.only(bottom: 16),
                child: ContentSurface(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      Text(monitoringMetricLabel(l, query.metric), style: Theme.of(context).textTheme.titleMedium),
                      if (query.error case final failure?)
                        InfoBanner(message: errorMessage(l, failure), tone: BannerTone.danger)
                      else if (query.outcome case final outcome?) ...[
                        if (outcome.error == null && outcome.status == 'success' && (outcome.data?.empty ?? true))
                          Text(l.monitoringNoData),
                        MonitoringResultView(
                          key: ValueKey('dashboard-builtin-${binding.id}-${query.metric.name}'),
                          result: outcome,
                          requestedWindow: window,
                          unit: query.metric.unit,
                        ),
                      ],
                    ],
                  ),
                ),
              ),
          ],
          for (final panel in visible)
            Padding(
              padding: const EdgeInsets.only(bottom: 16),
              child: ContentSurface(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    Text(panel.definition.title, style: Theme.of(context).textTheme.titleMedium),
                    const SizedBox(height: 8),
                    if (failures[panel.id] case final failure?)
                      InfoBanner(message: errorMessage(l, failure), tone: BannerTone.danger)
                    else if (results[panel.id] case final value?) ...[
                      if (value.outcome.error == null &&
                          value.outcome.status == 'success' &&
                          (value.outcome.data?.empty ?? true))
                        Text(l.monitoringNoData),
                      MonitoringResultView(
                        key: ValueKey('dashboard-chart-${panel.id}'),
                        result: value.outcome,
                        requestedWindow: window,
                      ),
                    ] else
                      const LinearProgressIndicator(),
                  ],
                ),
              ),
            ),
        ],
      ),
    );
  }
}
