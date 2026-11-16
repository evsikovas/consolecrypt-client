import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/monitoring/job_context_focus.dart';
import 'package:consolecrypt/monitoring/job_context_navigation.dart';
import 'package:consolecrypt/monitoring/job_context_project_guard.dart';
import 'package:consolecrypt/monitoring/job_metric_correlation.dart';
import 'package:consolecrypt/monitoring/job_metric_timeline_flow.dart';
import 'package:consolecrypt/monitoring/job_metric_timeline_view.dart';
import 'package:consolecrypt/monitoring/monitoring_binding_models.dart';
import 'package:consolecrypt/monitoring/monitoring_binding_reader.dart';
import 'package:consolecrypt/monitoring/monitoring_errors.dart';
import 'package:consolecrypt/monitoring/monitoring_exporter_dialog.dart';
import 'package:consolecrypt/monitoring/monitoring_exporter_review.dart';
import 'package:consolecrypt/monitoring/monitoring_host_context.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window_controls.dart';
import 'package:material_ui/material_ui.dart';

class _LabelFields {
  _LabelFields(this.id, [String key = '', String value = ''])
    : label = TextEditingController(text: key),
      value = TextEditingController(text: value);
  final int id;
  final TextEditingController label, value;
  void dispose() {
    label.dispose();
    value.dispose();
  }
}

/// Metadata-only editor. Native owns identities, CAS and reference validation.
class MonitoringBindings extends StatefulWidget {
  const MonitoringBindings({
    super.key,
    required this.store,
    required this.sourceStore,
    required this.source,
    required this.sourceIds,
    required this.hosts,
    required this.onChanged,
    this.chooseHost,
    this.reader,
    this.timeWindow,
    this.active = true,
    this.onBeforeRead,
    this.onBeginPanelRead,
    this.isPanelTransferred,
    this.correlationScope,
    this.correlationCurrent,
    this.correlationAutomation,
    this.acceptedPanelForCorrelation,
    this.onContextNavigation,
    this.requestedContext,
    this.onContextDismissed,
    this.onTimelineChanged,
  });
  final void Function(JobMetricTimelineFlow owner, MonitoringCorrelationTimeline? value)? onTimelineChanged;
  final JobCorrelationScope? correlationScope;
  final bool Function()? correlationCurrent;
  final AutomationService? correlationAutomation;
  final MonitoringCorrelationPanel? Function(MonitoringBinding binding, MonitoringPanel panel)?
  acceptedPanelForCorrelation;
  final ValueChanged<JobContextFocus>? onContextNavigation;
  final JobContextFocus? requestedContext;
  final VoidCallback? onContextDismissed;
  final MonitoringBindingStore store;
  final MonitoringSourceStore sourceStore;
  final MonitoringSource? source;
  final Set<String> sourceIds;
  final List<MonitoringHostRef> hosts;
  final Future<MonitoringHostRef?> Function()? chooseHost;
  final MonitoringBindingReader? reader;
  final MonitoringTimeWindowController? timeWindow;
  final Future<void> Function() onChanged;
  final bool active;
  final VoidCallback? onBeforeRead;
  final bool Function(MonitoringBinding binding, MonitoringPanel panel)? isPanelTransferred;
  final ValueChanged<BindingReadResult>? Function(
    MonitoringBinding binding,
    MonitoringPanel panel,
    MonitoringTimeWindow window,
  )?
  onBeginPanelRead;
  @override
  State<MonitoringBindings> createState() => _MonitoringBindingsState();
}

class _MonitoringBindingsState extends State<MonitoringBindings> {
  final name = TextEditingController();
  final rows = <_LabelFields>[];
  MonitoringBinding? initial;
  String? hostId, confirmDelete, selectedBinding;
  bool editing = false, busy = false;
  int generation = 0, rowId = 0;
  Object? error;
  VoidCallback? dismissExporter;
  JobContextFocus? handledContext;
  String? proposedExporterBinding;
  bool get writable => widget.active && !widget.sourceStore.readOnly && widget.source?.readOnly != true;
  bool current(int token) => mounted && widget.active && token == generation;

  @override
  void didUpdateWidget(MonitoringBindings oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.source?.fingerprint != widget.source?.fingerprint ||
        !identical(oldWidget.reader, widget.reader) ||
        !identical(oldWidget.store, widget.store) ||
        !identical(oldWidget.correlationScope, widget.correlationScope) ||
        !identical(oldWidget.correlationAutomation, widget.correlationAutomation) ||
        oldWidget.active && !widget.active) {
      dismissExporter?.call();
      dismissExporter = null;
      proposedExporterBinding = null;
      generation++;
      busy = false;
      error = null;
    }
    if (oldWidget.active && !widget.active) {
      generation++;
      editing = false;
      confirmDelete = null;
      selectedBinding = null;
      busy = false;
      error = null;
    }
  }

  @override
  void dispose() {
    dismissExporter?.call();
    dismissExporter = null;
    generation++;
    name.dispose();
    for (final row in rows) {
      row.dispose();
    }
    super.dispose();
  }

  void edit([MonitoringBinding? binding]) {
    widget.onContextDismissed?.call();
    if (!writable || widget.source == null) return;
    generation++;
    for (final row in rows) {
      row.dispose();
    }
    rows.clear();
    setState(() {
      initial = binding;
      editing = true;
      selectedBinding = null;
      error = null;
      confirmDelete = null;
      name.text = binding?.name ?? '';
      hostId = binding?.hostId;
      for (final entry in binding?.selector.entries ?? <MapEntry<String, String>>[]) {
        rows.add(_LabelFields(rowId++, entry.key, entry.value));
      }
      if (rows.isEmpty) rows.add(_LabelFields(rowId++));
    });
  }

  void applyRequestedContext() {
    final focus = widget.requestedContext, source = widget.source;
    if (focus == null || identical(focus, handledContext) || source == null || !widget.active || busy || editing) return;
    handledContext = focus;
    final m = focus.metadata;
    final matching = widget.store.bindings.where((b) => b.id == m.bindingId && b.revision == m.bindingRevision &&
      b.hostId == m.hostId && b.sourceId == m.sourceId && b.sourceRevision == m.sourceRevision &&
      b.state == MonitoringBindingState.current).toList();
    if (!focus.metadataOnly || focus.selection.destination != JobContextDestination.monitoringBinding ||
        !identical(focus.scope, widget.correlationScope) || widget.correlationCurrent?.call() == false ||
        source.id != m.sourceId || source.revision != m.sourceRevision || matching.length != 1) {
      selectedBinding = null;
      error = const MonitoringFailure.local(MonitoringErrorCode.bindingChanged);
      return;
    }
    selectedBinding = matching.single.id;
    confirmDelete = null;
  }

  Future<bool> recheckRequestedContext() async {
    final focus = widget.requestedContext, reader = widget.reader, jobs = widget.correlationAutomation;
    if (focus == null) return true;
    if (reader is! MonitoringHostContextReader || jobs is! AutomationJobService) return false;
    final metadata = reader as MonitoringHostContextReader, history = jobs as AutomationJobService;
    bool original() => mounted && widget.active && widget.correlationCurrent?.call() != false &&
      identical(widget.requestedContext, focus) && identical(widget.correlationScope, focus.scope) &&
      identical(widget.reader, reader) && identical(widget.correlationAutomation, jobs) &&
      selectedBinding == focus.metadata.bindingId && widget.source?.id == focus.metadata.sourceId &&
      widget.source?.revision == focus.metadata.sourceRevision;
    if (!original()) return false;
    try {
      final catalog = await history.jobs();
      if (!original() || !jobContextMonitoringJobMatches(focus, catalog)) return false;
      final checked = await metadata.recheckHostContext(focus.metadata);
      return original() && sameJobContextMetadata(focus.metadata, checked);
    } catch (_) {
      return false;
    }
  }

  Future<void> exporterReview() async {
    final source = widget.source, reader = widget.reader, store = widget.store;
    final scope = widget.correlationScope, automation = widget.correlationAutomation;
    if (!writable ||
        busy ||
        source == null ||
        scope == null ||
        reader is! MonitoringExporterReader ||
        store is! MonitoringExporterBindingStore ||
        automation is! AutomationJobService) {
      return;
    }
    final exporterReader = reader as MonitoringExporterReader;
    final exporterStore = store as MonitoringExporterBindingStore;
    final jobs = automation as AutomationJobService;
    final token = ++generation;
    bool original() =>
        current(token) &&
        widget.correlationCurrent?.call() != false &&
        identical(widget.correlationScope, scope) &&
        identical(widget.correlationAutomation, automation) &&
        identical(widget.reader, reader) &&
        identical(widget.store, store) &&
        widget.source?.fingerprint == source.fingerprint;
    setState(() => busy = true);
    MonitoringBinding? saved;
    try {
      await showAppDialog<void>(
        context,
        builder: (dialogContext) {
          final route = ModalRoute.of(dialogContext), navigator = Navigator.of(dialogContext);
          dismissExporter = () => WidgetsBinding.instance.addPostFrameCallback((_) {
            if (navigator.mounted && route?.isActive == true) navigator.removeRoute(route!);
          });
          return MonitoringExporterDialog(
            profileId: scope.profileId,
            source: source,
            reader: exporterReader,
            store: exporterStore,
            jobs: jobs,
            hosts: widget.hosts,
            isCurrent: original,
            onSaved: (binding) {
              if (!original()) return;
              saved = binding;
              dismissExporter?.call();
            },
          );
        },
      );
      if (!original() || saved == null) return;
      await widget.onChanged();
      if (original()) {
        setState(() {
          proposedExporterBinding = saved!.id;
          selectedBinding = null;
          editing = false;
        });
      }
    } finally {
      dismissExporter = null;
      if (current(token)) setState(() => busy = false);
    }
  }

  Future<void> pickHost() async {
    final token = generation, callback = widget.chooseHost;
    if (callback == null || busy || !writable) return;
    final chosen = await callback();
    if (current(token) && chosen != null && widget.hosts.any((h) => h.id == chosen.id)) {
      setState(() => hostId = chosen.id);
    }
  }

  Future<void> save() async {
    final source = widget.source;
    if (!writable || busy || source == null) return;
    final token = ++generation;
    setState(() {
      busy = true;
      error = null;
    });
    try {
      final selector = <String, String>{};
      for (final row in rows) {
        if (selector.containsKey(row.label.text)) {
          throw const MonitoringFailure.local(MonitoringErrorCode.invalidBinding);
        }
        selector[row.label.text] = row.value.text;
      }
      if (!widget.hosts.any((h) => h.id == hostId)) {
        throw const MonitoringFailure.local(MonitoringErrorCode.invalidBinding);
      }
      final draft = MonitoringBinding(
        id: initial?.id ?? '',
        name: name.text.trim(),
        hostId: hostId!,
        sourceId: source.id,
        revision: initial?.revision ?? 0,
        sourceRevision: source.revision,
        selector: selector,
        state: MonitoringBindingState.current,
      );
      draft.validate(create: initial == null);
      await widget.store.saveBinding(draft, create: initial == null);
      if (!current(token)) return;
      await widget.onChanged();
      if (current(token)) {
        setState(() {
          editing = false;
          initial = null;
        });
      }
    } catch (e) {
      if (current(token)) setState(() => error = e);
    } finally {
      if (current(token)) setState(() => busy = false);
    }
  }

  Future<void> delete(MonitoringBinding binding) async {
    if (!widget.active || widget.sourceStore.readOnly || busy) return;
    final token = ++generation;
    setState(() {
      busy = true;
      error = null;
    });
    try {
      await widget.store.deleteBinding(binding);
      if (!current(token)) return;
      await widget.onChanged();
      if (current(token)) setState(() => confirmDelete = null);
    } catch (e) {
      if (current(token)) setState(() => error = e);
    } finally {
      if (current(token)) setState(() => busy = false);
    }
  }

  Future<void> refresh() async {
    if (busy || !widget.active) return;
    final token = ++generation;
    setState(() {
      busy = true;
      editing = false;
      selectedBinding = null;
      confirmDelete = null;
      error = null;
    });
    try {
      await widget.onChanged();
    } catch (e) {
      if (current(token)) setState(() => error = e);
    } finally {
      if (current(token)) setState(() => busy = false);
    }
  }

  String hostName(String id) =>
      widget.hosts.where((h) => h.id == id).firstOrNull?.name ?? context.l10n.monitoringBindingHost;
  String stateLabel(MonitoringBindingState state) {
    final l = context.l10n;
    return switch (state) {
      MonitoringBindingState.current => l.monitoringBindingCurrent,
      MonitoringBindingState.staleSource => l.monitoringBindingStale,
      MonitoringBindingState.missingHost => l.monitoringBindingMissingHost,
      MonitoringBindingState.missingSource => l.monitoringBindingMissingSource,
    };
  }

  Widget field(String label, TextEditingController controller, String key) => TextField(
    key: ValueKey(key),
    controller: controller,
    enabled: !busy && writable,
    autocorrect: false,
    enableSuggestions: false,
    decoration: InputDecoration(labelText: label),
  );

  @override
  Widget build(BuildContext context) {
    applyRequestedContext();
    final l = context.l10n, source = widget.source;
    final visible = widget.store.bindings
        .where((b) => b.sourceId == source?.id || !widget.sourceIds.contains(b.sourceId))
        .toList();
    final inspected = visible
        .where((b) => b.id == selectedBinding && b.state == MonitoringBindingState.current && b.sourceId == source?.id)
        .firstOrNull;
    return ContentSurface(
      key: const ValueKey('monitoring-bindings'),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Wrap(
            spacing: 8,
            runSpacing: 4,
            crossAxisAlignment: WrapCrossAlignment.center,
            children: [
              const Icon(Icons.link, size: 20),
              Text(l.monitoringBindings, style: Theme.of(context).textTheme.titleMedium),
              TextButton.icon(
                key: const ValueKey('monitoring-bindings-refresh'),
                onPressed: busy || !widget.active ? null : refresh,
                icon: const Icon(Icons.refresh, size: 18),
                label: Text(l.monitoringBindingRefresh),
              ),
              if (source != null &&
                  widget.reader is MonitoringExporterReader &&
                  widget.store is MonitoringExporterBindingStore &&
                  widget.correlationScope != null &&
                  widget.correlationAutomation is AutomationJobService)
                TextButton.icon(
                  key: const ValueKey('monitoring-exporter-review'),
                  onPressed: writable && !busy ? exporterReview : null,
                  icon: const Icon(Icons.monitor_heart_outlined, size: 18),
                  label: Text(l.monitoringExporterTitle),
                ),
              if (source != null)
                TextButton.icon(
                  key: const ValueKey('monitoring-binding-add'),
                  onPressed: writable && !busy && widget.store.bindings.length < 256 ? () => edit() : null,
                  icon: const Icon(Icons.add, size: 18),
                  label: Text(l.monitoringBindingAdd),
                ),
            ],
          ),
          const SizedBox(height: 8),
          Text(l.monitoringBindingHelp),
          const SizedBox(height: 8),
          Text(l.monitoringBindingCurrentHelp),
          if (widget.sourceStore.readOnly) Text(l.monitoringReadOnly),
          if (widget.store.bindings.length >= 256) Text(l.monitoringErrorLimit),
          if (error != null)
            Padding(
              padding: const EdgeInsets.only(top: 8),
              child: SelectableText(
                error is MonitoringFailure
                    ? monitoringFailureMessage(l, error as MonitoringFailure)
                    : errorMessage(l, error!),
                key: const ValueKey('monitoring-binding-error'),
                style: TextStyle(color: Theme.of(context).colorScheme.error),
              ),
            ),
          if (busy) const LinearProgressIndicator(),
          if (visible.isEmpty)
            Padding(padding: const EdgeInsets.symmetric(vertical: 12), child: Text(l.monitoringBindingEmpty)),
          for (final binding in visible)
            Padding(
              key: ValueKey('monitoring-binding-${binding.id}'),
              padding: const EdgeInsets.symmetric(vertical: 8),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  LayoutBuilder(
                    builder: (context, constraints) {
                      final names = Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(binding.name, style: Theme.of(context).textTheme.titleSmall),
                          Text(hostName(binding.hostId)),
                        ],
                      );
                      // The shared capsule intentionally never truncates or
                      // wraps. On narrow/enlarged layouts keep the complete
                      // status as ordinary wrapping text below the names.
                      if (constraints.maxWidth < 480 || MediaQuery.textScalerOf(context).scale(1) > 1.25) {
                        return Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            names,
                            if (binding.state == MonitoringBindingState.current) Text(stateLabel(binding.state)),
                          ],
                        );
                      }
                      return Row(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Expanded(child: names),
                          const SizedBox(width: 8),
                          if (binding.state == MonitoringBindingState.current)
                            GlassBadge(label: stateLabel(binding.state)),
                        ],
                      );
                    },
                  ),
                  if (binding.state != MonitoringBindingState.current)
                    Text(stateLabel(binding.state), style: TextStyle(color: GlassTokens.of(context).palette.warning)),
                  if (!widget.sourceIds.contains(binding.sourceId) &&
                      binding.state != MonitoringBindingState.missingSource)
                    Text(
                      l.monitoringBindingMissingSource,
                      style: TextStyle(color: GlassTokens.of(context).palette.warning),
                    ),
                  Text(
                    binding.selector.entries.map((e) => '${e.key} = ${e.value}').join(' · '),
                    maxLines: 2,
                    overflow: TextOverflow.ellipsis,
                  ),
                  if (binding.state == MonitoringBindingState.staleSource)
                    Text(
                      '${l.monitoringBindingPinnedRevision}: ${binding.sourceRevision} · ${l.monitoringBindingRevision}: ${source?.revision}',
                    ),
                  Wrap(
                    spacing: 8,
                    children: [
                      if (widget.reader != null && binding.sourceId == source?.id)
                        TextButton(
                          key: ValueKey('monitoring-binding-inspect-${binding.id}'),
                          onPressed: writable && !busy && binding.state == MonitoringBindingState.current
                              ? () => setState(() {
                                  if (widget.requestedContext case final focus? when focus.metadata.bindingId != binding.id) {
                                    widget.onContextDismissed?.call();
                                  }
                                  selectedBinding = binding.id;
                                  proposedExporterBinding = null;
                                  editing = false;
                                  error = null;
                                })
                              : null,
                          child: Text(
                            binding.id == proposedExporterBinding
                                ? l.monitoringExporterOpenPanel
                                : l.monitoringBindingInspect,
                          ),
                        ),
                      if (binding.sourceId == source?.id)
                        TextButton(
                          key: ValueKey('monitoring-binding-edit-${binding.id}'),
                          onPressed: writable && !busy ? () => edit(binding) : null,
                          child: Text(l.monitoringBindingEdit),
                        ),
                      TextButton(
                        key: ValueKey('monitoring-binding-delete-${binding.id}'),
                        onPressed: widget.active && !widget.sourceStore.readOnly && !busy
                            ? () => setState(() => confirmDelete = binding.id)
                            : null,
                        child: Text(l.monitoringBindingDelete),
                      ),
                    ],
                  ),
                  if (confirmDelete == binding.id) ...[
                    Text(l.monitoringBindingDeleteHint),
                    Wrap(
                      spacing: 8,
                      children: [
                        TextButton(
                          onPressed: busy ? null : () => setState(() => confirmDelete = null),
                          child: Text(l.commonCancel),
                        ),
                        TextButton(
                          key: ValueKey('monitoring-binding-delete-confirm-${binding.id}'),
                          onPressed: busy ? null : () => delete(binding),
                          child: Text(l.monitoringBindingDelete),
                        ),
                      ],
                    ),
                  ],
                ],
              ),
            ),
          if (editing && source != null) ...[
            const Divider(),
            Text(
              initial == null ? l.monitoringBindingAdd : l.monitoringBindingEdit,
              style: Theme.of(context).textTheme.titleSmall,
            ),
            const SizedBox(height: 12),
            Text('${source.name} · ${l.monitoringBindingRevision}: ${source.revision}'),
            if (initial != null) Text('${l.monitoringBindingPinnedRevision}: ${initial!.sourceRevision}'),
            const SizedBox(height: 12),
            field(l.monitoringBindingName, name, 'monitoring-binding-name'),
            const SizedBox(height: 8),
            Wrap(
              spacing: 12,
              crossAxisAlignment: WrapCrossAlignment.center,
              children: [
                Text(hostId == null ? l.monitoringBindingHost : hostName(hostId!)),
                TextButton.icon(
                  key: const ValueKey('monitoring-binding-host'),
                  onPressed: writable && !busy ? pickHost : null,
                  icon: const Icon(Icons.dns, size: 18),
                  label: Text(l.monitoringBindingChooseHost),
                ),
              ],
            ),
            const SizedBox(height: 8),
            Text(l.monitoringBindingSelector),
            for (final row in rows)
              Padding(
                key: ValueKey('monitoring-binding-label-row-${row.id}'),
                padding: const EdgeInsets.only(top: 12),
                child: Row(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Expanded(
                      flex: 2,
                      child: field(l.monitoringBindingLabel, row.label, 'monitoring-binding-label-${row.id}'),
                    ),
                    const SizedBox(width: 8),
                    Expanded(
                      flex: 3,
                      child: field(l.monitoringBindingValue, row.value, 'monitoring-binding-value-${row.id}'),
                    ),
                    IconButton(
                      tooltip: l.monitoringBindingRemoveLabel,
                      onPressed: busy || !writable || rows.length == 1
                          ? null
                          : () {
                              setState(() => rows.remove(row));
                              row.dispose();
                            },
                      icon: const Icon(Icons.remove_circle_outline),
                    ),
                  ],
                ),
              ),
            Align(
              alignment: Alignment.centerLeft,
              child: TextButton.icon(
                key: const ValueKey('monitoring-binding-add-label'),
                onPressed: busy || !writable || rows.length >= 32
                    ? null
                    : () => setState(() => rows.add(_LabelFields(rowId++))),
                icon: const Icon(Icons.add, size: 18),
                label: Text(l.monitoringBindingAddLabel),
              ),
            ),
            Text(l.monitoringBindingReviewHint),
            const SizedBox(height: 12),
            Wrap(
              spacing: 8,
              children: [
                GlassButton(
                  key: const ValueKey('monitoring-binding-save'),
                  label: l.monitoringBindingSave,
                  onPressed: busy || !writable ? null : save,
                ),
                TextButton(
                  onPressed: busy
                      ? null
                      : () => setState(() {
                          editing = false;
                          error = null;
                        }),
                  child: Text(l.commonCancel),
                ),
              ],
            ),
          ],
          if (inspected != null && source != null && widget.reader != null) ...[
            const SizedBox(height: 16),
            MonitoringBindingReaderView(
              correlationScope: widget.correlationScope,
              correlationCurrent: widget.correlationCurrent,
              correlationAutomation: widget.correlationAutomation,
              acceptedPanelForCorrelation: widget.acceptedPanelForCorrelation,
              onContextNavigation: widget.onContextNavigation,
              onTimelineChanged: widget.onTimelineChanged,
              key: ValueKey('${inspected.id}:${inspected.revision}:${inspected.sourceRevision}'),
              reader: widget.reader!,
              source: source,
              binding: inspected,
              timeWindow: widget.timeWindow,
              requestedContext: widget.requestedContext?.metadata.bindingId == inspected.id ? widget.requestedContext : null,
              onBeforeContextRead: widget.requestedContext?.metadata.bindingId == inspected.id ? recheckRequestedContext : null,
              onBeforeRead: widget.onBeforeRead,
              onBeginPanelRead: widget.onBeginPanelRead,
              isPanelTransferred: (panel) => widget.isPanelTransferred?.call(inspected, panel) ?? false,
              active: widget.active,
              onClose: () => setState(() => selectedBinding = null),
            ),
          ],
        ],
      ),
    );
  }
}
