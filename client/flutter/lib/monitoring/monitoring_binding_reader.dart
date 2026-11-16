import 'dart:convert';

import 'package:consolecrypt/app/gate.dart' show AppRoutes;

import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/monitoring/job_context_focus.dart';
import 'package:consolecrypt/monitoring/job_context_navigation.dart';
import 'package:consolecrypt/monitoring/job_metric_correlation.dart';
import 'package:consolecrypt/monitoring/job_metric_timeline_flow.dart';
import 'package:consolecrypt/monitoring/job_metric_timeline_service.dart';
import 'package:consolecrypt/monitoring/job_metric_timeline_view.dart';
import 'package:consolecrypt/monitoring/monitoring_binding_models.dart';
import 'package:consolecrypt/monitoring/monitoring_errors.dart';
import 'package:consolecrypt/monitoring/monitoring_host_context.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_observation_plan.dart';
import 'package:consolecrypt/monitoring/monitoring_panel_collection_guidance.dart';
import 'package:consolecrypt/monitoring/monitoring_result_view.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window_controls.dart';
import 'package:consolecrypt/monitoring/monitoring_unit_labels.dart';
import 'package:go_router/go_router.dart';
import 'package:material_ui/material_ui.dart';

export 'package:consolecrypt/monitoring/monitoring_unit_labels.dart' show monitoringUnitLabel;

typedef MonitoringCorrelationPanel = ({BindingReadResult read, MonitoringTimeWindow window});

String monitoringPanelLabel(AppLocalizations l, MonitoringPanel panel) => switch (panel) {
  MonitoringPanel.hostOverview => l.monitoringPanelHostOverview,
  MonitoringPanel.cpu => l.monitoringPanelCpu,
  MonitoringPanel.memory => l.monitoringPanelMemory,
  MonitoringPanel.filesystem => l.monitoringPanelFilesystem,
  MonitoringPanel.diskIo => l.monitoringPanelDiskIo,
  MonitoringPanel.network => l.monitoringPanelNetwork,
  MonitoringPanel.docker => l.monitoringPanelDocker,
  MonitoringPanel.postgreSql => l.monitoringPanelPostgreSql,
  MonitoringPanel.redis => l.monitoringPanelRedis,
  MonitoringPanel.nginx => l.monitoringPanelNginx,
};
String monitoringMetricLabel(AppLocalizations l, MonitoringMetric metric) => switch (metric) {
  MonitoringMetric.cpuBusy => l.monitoringMetricCpuBusy,
  MonitoringMetric.memoryUsedPercent => l.monitoringMetricMemoryUsedPercent,
  MonitoringMetric.memoryAvailable => l.monitoringMetricMemoryAvailable,
  MonitoringMetric.filesystemUnavailablePercent => l.monitoringMetricFilesystemUnavailablePercent,
  MonitoringMetric.diskRead => l.monitoringMetricDiskRead,
  MonitoringMetric.diskWrite => l.monitoringMetricDiskWrite,
  MonitoringMetric.networkReceive => l.monitoringMetricNetworkReceive,
  MonitoringMetric.networkTransmit => l.monitoringMetricNetworkTransmit,
  MonitoringMetric.containerCpu => l.monitoringMetricContainerCpu,
  MonitoringMetric.containerMemory => l.monitoringMetricContainerMemory,
  MonitoringMetric.postgresConnections => l.monitoringMetricPostgresConnections,
  MonitoringMetric.postgresCommits => l.monitoringMetricPostgresCommits,
  MonitoringMetric.redisMemory => l.monitoringMetricRedisMemory,
  MonitoringMetric.redisClients => l.monitoringMetricRedisClients,
  MonitoringMetric.redisCommands => l.monitoringMetricRedisCommands,
  MonitoringMetric.nginxConnections => l.monitoringMetricNginxConnections,
  MonitoringMetric.nginxRequests => l.monitoringMetricNginxRequests,
};

String monitoringBindingStateLabel(AppLocalizations l, BindingReadState state) => switch (state) {
  BindingReadState.resolved => l.monitoringBindingResolved,
  BindingReadState.missing => l.monitoringBindingMissing,
  BindingReadState.ambiguous => l.monitoringBindingAmbiguous,
  BindingReadState.unavailable => l.monitoringBindingUnavailable,
  BindingReadState.ready => l.monitoringBindingPanelReady,
  BindingReadState.partial => l.monitoringBindingPanelPartial,
  BindingReadState.noData => l.monitoringBindingPanelNoData,
};

/// An explicitly selected saved binding. Only Native can resolve it and prepare
/// a panel; this widget never renders or executes a PromQL template.
class MonitoringBindingReaderView extends StatefulWidget {
  const MonitoringBindingReaderView({
    super.key,
    required this.reader,
    required this.source,
    required this.binding,
    this.active = true,
    this.onClose,
    this.timeWindow,
    this.onBeforeRead,
    this.onBeforeContextRead,
    this.requestedContext,
    this.onBeginPanelRead,
    this.isPanelTransferred,
    this.correlationScope,
    this.correlationCurrent,
    this.correlationAutomation,
    this.acceptedPanelForCorrelation,
    this.onContextNavigation,
    this.onTimelineChanged,
  });
  final void Function(JobMetricTimelineFlow owner, MonitoringCorrelationTimeline? value)? onTimelineChanged;
  final JobCorrelationScope? correlationScope;
  final bool Function()? correlationCurrent;
  final AutomationService? correlationAutomation;
  final MonitoringCorrelationPanel? Function(MonitoringBinding binding, MonitoringPanel panel)? acceptedPanelForCorrelation;
  final ValueChanged<JobContextFocus>? onContextNavigation;
  final MonitoringBindingReader reader;
  final MonitoringSource source;
  final MonitoringBinding binding;
  final bool active;
  final VoidCallback? onClose;
  final MonitoringTimeWindowController? timeWindow;
  final VoidCallback? onBeforeRead;
  final Future<bool> Function()? onBeforeContextRead;
  final JobContextFocus? requestedContext;
  final bool Function(MonitoringPanel panel)? isPanelTransferred;
  final ValueChanged<BindingReadResult>? Function(
    MonitoringBinding binding,
    MonitoringPanel panel,
    MonitoringTimeWindow window,
  )?
  onBeginPanelRead;
  @override
  State<MonitoringBindingReaderView> createState() => _MonitoringBindingReaderViewState();
}

class _MonitoringBindingReaderViewState extends State<MonitoringBindingReaderView> {
  late MonitoringTimeWindowController timeWindow;
  late bool ownsTimeWindow;
  MonitoringTimeWindow? resultWindow;
  MonitoringPanel panel = MonitoringPanel.hostOverview;
  BindingReadResult? result;
  bool busy = false, contextBusy = false;
  final contextNavigation = JobContextNavigation();
  Object? contextError;
  Object? error;
  int generation = 0;
  JobMetricTimelineFlow? timeline;
  String? timelineIdentity;
  int timelineEpoch = 0;
  bool timelineLocalTime = false;
  MonitoringCorrelationTimeline? get chartTimeline {
    final owner = timeline, snapshot = timeline?.snapshot;
    final accepted = currentCorrelationPanel();
    return owner == null || snapshot == null || accepted == null ? null :
      (owner: owner, snapshot: snapshot, window: accepted.window, openProject: navigateProject, localTime: timelineLocalTime);
  }
  void publishTimeline(JobMetricTimelineFlow owner, MonitoringCorrelationTimeline? value) {
    final callback = widget.onTimelineChanged;
    // Clearing can happen during child reconciliation/disposal. Notify the
    // parent after layout, retaining exact old owner identity for late clears.
    WidgetsBinding.instance.addPostFrameCallback((_) => callback?.call(owner, value));
  }
  bool get correlationCurrent => widget.correlationCurrent?.call() ?? true;
  MonitoringCorrelationPanel? currentCorrelationPanel() {
    if (!correlationCurrent) return null;
    final accepted = widget.acceptedPanelForCorrelation?.call(widget.binding, panel);
    final local = result, window = resultWindow;
    final value = accepted ?? (widget.isPanelTransferred?.call(panel) == true || local == null || window == null
      ? null : (read: local, window: window));
    if (value == null || value.read.kind != 'bindingPanel' || value.read.bindingId != widget.binding.id ||
        value.read.bindingRevision != widget.binding.revision || value.read.panel != panel ||
        value.read.error != null || value.read.preparationError != null ||
        !const {BindingReadState.ready, BindingReadState.partial, BindingReadState.noData}.contains(value.read.state)) {
      return null;
    }
    return value;
  }
  String? correlationIdentity() {
    final accepted = currentCorrelationPanel();
    return accepted == null ? null : jsonEncode([widget.source.fingerprint,
      monitoringBindingObservationIdentity(widget.binding), panel.name,
      accepted.window.startSeconds, accepted.window.endSeconds, accepted.window.stepSeconds]);
  }
  void clearTimeline() {
    timelineEpoch++;
    final owner = timeline;
    owner?.dispose();
    if (owner != null) publishTimeline(owner, null);
    timeline = null; timelineIdentity = null;
  }
  Future<void> readTimeline() async {
    final scope = widget.correlationScope, source = widget.source, binding = widget.binding;
    final reader = widget.reader, automation = widget.correlationAutomation;
    final accepted = currentCorrelationPanel(), identity = correlationIdentity();
    if (!mounted || !correlationCurrent || !widget.active || busy || contextBusy || timeline?.busy == true || timeWindow.dirty || scope == null || accepted == null ||
        identity == null || automation is! AutomationJobService || reader is! MonitoringHostContextReader) {
      return;
    }
    final metadata = reader as MonitoringHostContextReader;
    if (!metadata.hostContextSupported) return;
    if (timelineIdentity != identity) {
      clearTimeline(); timelineIdentity = identity;
      final epoch = timelineEpoch;
      bool original() => mounted && correlationCurrent && widget.active && epoch == timelineEpoch && !timeWindow.dirty &&
        identical(widget.reader, reader) && identical(widget.correlationAutomation, automation) &&
        identical(widget.correlationScope, scope) && correlationIdentity() == identity;
      timeline = monitoringJobTimeline(selection: JobMetricTimelineSelection(scope: scope,
        hostId: binding.hostId, sourceId: source.id, sourceRevision: source.revision,
        bindingId: binding.id, bindingRevision: binding.revision,
        startUtc: accepted.window.startUtc, endUtc: accepted.window.endUtc), monitoring: metadata,
        jobs: automation as AutomationJobService, source: source, binding: binding,
        isCurrent: original, nowUtc: () => DateTime.now().toUtc());
    }
    final owner = timeline!;
    final pending = owner.load();
    publishTimeline(owner, null);
    setState(() {});
    await pending;
    if (mounted && identical(timeline, owner)) {
      setState(() {});
      publishTimeline(owner, chartTimeline);
    }
  }
  Future<void> navigateProject(JobMetricMarker marker) async {
    final owner = timeline, open = widget.onContextNavigation, automation = widget.correlationAutomation;
    final reader = widget.reader, originalSnapshot = timeline?.snapshot;
    if (!mounted || !correlationCurrent || !widget.active || busy || contextBusy || owner == null || originalSnapshot == null || open == null ||
        automation == null || reader is! MonitoringHostContextReader) {
      return;
    }
    final choice = owner.choose(originalSnapshot, marker);
    if (choice == null) return;
    final metadata = reader as MonitoringHostContextReader;
    final epoch = timelineEpoch, identity = timelineIdentity;
    bool original() => mounted && correlationCurrent && widget.active && epoch == timelineEpoch && identical(timeline, owner) &&
      identical(owner.snapshot, originalSnapshot) && correlationIdentity() == identity &&
      identical(widget.correlationAutomation, automation) && identical(widget.reader, reader);
    setState(() { contextBusy = true; contextError = null; });
    try {
      final catalog = await automation.projects();
      if (!original()) return;
      final selected = catalog.version == 1 && !catalog.readOnly ? catalog.projects.where((project) =>
        project.id == marker.projectId && !project.readOnly && project.revision != null && project.revision! > 0 &&
        (project.importState == null || project.importState == 'ready')).toList() : <AutomationProject>[];
      if (selected.length != 1) throw const MonitoringHostContextException(MonitoringHostContextIssue.changed);
      final project = selected.single;
      final files = await automation.files(project.id);
      if (!original()) return;
      if (files.digest != marker.revisionDigest) throw const MonitoringHostContextException(MonitoringHostContextIssue.changed);
      final frame = JobContextNavigationFrame(context: originalSnapshot.jobs.context!,
        notificationScopeId: originalSnapshot.metadata.notificationScopeId, metadata: originalSnapshot.metadata,
        jobs: originalSnapshot.jobs, project: JobContextProject(project.id, project.revision!, files.digest));
      contextNavigation.bind(frame);
      final decision = contextNavigation.propose(frame, JobContextDestination.ansibleProject, job: marker);
      if (decision.intent == null) throw const MonitoringHostContextException(MonitoringHostContextIssue.changed);
      final checked = await metadata.recheckHostContext(originalSnapshot.metadata);
      if (!original() || !sameJobContextMetadata(originalSnapshot.metadata, checked)) return;
      final selection = contextNavigation.take(decision.intent!);
      final packet = selection == null ? null : JobContextFocus.fromTaken(selection, frame);
      if (packet != null) open(packet);
    } catch (failure) {
      if (original()) setState(() => contextError = failure);
    } finally {
      if (mounted && epoch == timelineEpoch && identical(timeline, owner)) setState(() => contextBusy = false);
    }
  }
  AppLocalizations get l => context.l10n;
  bool current(int token) => mounted && widget.active && generation == token;
  @override
  void initState() {
    super.initState();
    attachTimeWindow();
  }

  void attachTimeWindow() {
    ownsTimeWindow = widget.timeWindow == null;
    timeWindow = widget.timeWindow ?? MonitoringTimeWindowController();
    timeWindow.addListener(invalidate);
  }

  void detachTimeWindow() {
    timeWindow.removeListener(invalidate);
    if (ownsTimeWindow) timeWindow.dispose();
  }

  @override
  void didUpdateWidget(MonitoringBindingReaderView oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!identical(oldWidget.requestedContext, widget.requestedContext)) {
      generation++;
      widget.reader.cancelBinding();
      busy = false; contextBusy = false; contextError = null; error = null;
    }
    if (timelineIdentity != null && timelineIdentity != correlationIdentity()) {
      clearTimeline();
      contextNavigation.bind(null);
      contextBusy = false;
      contextError = null;
    }
    if (oldWidget.timeWindow != widget.timeWindow) {
      detachTimeWindow();
      attachTimeWindow();
      invalidate();
    }
    if (oldWidget.active && !widget.active) {
      clearTimeline();
      contextNavigation.bind(null);
      contextBusy = false;
      generation++;
      widget.reader.cancelBinding();
      result = null;
      error = null;
      busy = false;
    }
  }

  @override
  void dispose() {
    clearTimeline();
    contextNavigation.dispose();
    generation++;
    widget.reader.cancelBinding();
    detachTimeWindow();
    super.dispose();
  }

  void invalidate() {
    clearTimeline();
    contextNavigation.bind(null);
    contextBusy = false;
    contextError = null;
    generation++;
    widget.reader.cancelBinding();
    if (mounted) {
      setState(() {
        result = null;
        error = null;
        busy = false;
      });
    }
  }

  Future<void> run({required bool discover}) async {
    if (busy || contextBusy || timeline?.busy == true || !widget.active || widget.binding.state != MonitoringBindingState.current) return;
    clearTimeline();
    final token = ++generation;
    final requestedPanel = panel;
    setState(() {
      busy = true;
      result = null;
      error = null;
    });
    try {
      final guard = widget.onBeforeContextRead;
      if (guard != null) {
        final allowed = await guard();
        if (!current(token)) return;
        if (!allowed) throw const MonitoringFailure.local(MonitoringErrorCode.bindingChanged);
      }
      if (!discover) widget.onBeforeRead?.call();
      final requestedWindow = timeWindow.requested;
      final selectedEnd = requestedWindow.endSeconds;
      final acceptPanel = discover
          ? null
          : widget.onBeginPanelRead?.call(widget.binding, requestedPanel, requestedWindow);
      final value = discover
          ? await widget.reader.discover(widget.source, widget.binding, selectedEnd)
          : await widget.reader.readPanel(
              widget.source,
              widget.binding,
              requestedPanel,
              start: requestedWindow.startSeconds,
              end: selectedEnd,
              stepSeconds: requestedWindow.stepSeconds,
            );
      if (current(token)) {
        setState(() {
          result = value;
          resultWindow = discover ? null : requestedWindow;
        });
        if (!discover) acceptPanel?.call(value);
      }
    } catch (e) {
      if (current(token)) {
        setState(
          () => error = e is MonitoringTimeWindowFailure
              ? const MonitoringFailure.local(MonitoringErrorCode.invalidQuery)
              : e,
        );
      }
    } finally {
      if (current(token)) setState(() => busy = false);
    }
  }

  Future<void> navigateHost(JobContextDestination destination) async {
    final source = widget.source, binding = widget.binding, scope = widget.correlationScope;
    final reader = widget.reader, open = widget.onContextNavigation;
    if (!mounted || !correlationCurrent || !widget.active || busy || contextBusy || timeline?.busy == true || timeWindow.dirty || scope == null || open == null ||
        reader is! MonitoringHostContextReader) {
      return;
    }
    final metadataReader = reader as MonitoringHostContextReader;
    if (!metadataReader.hostContextSupported) return;
    final window = resultWindow ?? timeWindow.requested;
    final token = ++generation;
    bool original() => current(token) && correlationCurrent && identical(widget.reader, reader) && identical(widget.correlationScope, scope) &&
        widget.source.fingerprint == source.fingerprint && widget.binding.id == binding.id &&
        widget.binding.revision == binding.revision && widget.binding.hostId == binding.hostId &&
        widget.binding.sourceId == source.id && widget.binding.sourceRevision == source.revision &&
        widget.binding.state == MonitoringBindingState.current;
    setState(() { contextBusy = true; contextError = null; });
    try {
      final metadata = await metadataReader.captureHostContext(profileId: scope.profileId, source: source, binding: binding);
      if (!original()) return;
      final correlated = JobCorrelationContext(scope: scope, hostId: metadata.hostId, hostRevision: metadata.hostRevision,
        sourceId: metadata.sourceId, sourceRevision: metadata.sourceRevision, bindingId: metadata.bindingId,
        bindingRevision: metadata.bindingRevision, bindingHostId: binding.hostId, bindingSourceId: binding.sourceId,
        bindingSourceRevision: binding.sourceRevision, bindingState: JobCorrelationBindingState.current,
        startUtc: window.startUtc, endUtc: window.endUtc);
      final frame = JobContextNavigationFrame(context: correlated, notificationScopeId: metadata.notificationScopeId, metadata: metadata);
      contextNavigation.bind(frame);
      final decision = contextNavigation.propose(frame, destination);
      if (decision.intent == null) throw const MonitoringHostContextException(MonitoringHostContextIssue.unsupported);
      final checked = await metadataReader.recheckHostContext(metadata);
      if (!sameJobContextMetadata(metadata, checked)) return;
      if (!original()) return;
      final selection = contextNavigation.take(decision.intent!);
      final packet = selection == null ? null : JobContextFocus.fromTaken(selection, frame);
      if (packet != null) open(packet); // Taken before navigation hides this page.
    } catch (failure) {
      if (original()) setState(() => contextError = failure);
    } finally {
      if (original()) setState(() => contextBusy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    return ContentSurface(
      key: const ValueKey('monitoring-binding-reader'),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Expanded(child: Text(widget.binding.name, style: Theme.of(context).textTheme.titleMedium)),
              if (widget.onClose != null)
                IconButton(tooltip: l.commonClose, onPressed: widget.onClose, icon: const Icon(Icons.close)),
            ],
          ),
          Text('${widget.source.name} · ${l.monitoringBindingRevision}: ${widget.source.revision}'),
          if (widget.correlationScope != null && widget.onContextNavigation != null &&
              widget.reader is MonitoringHostContextReader && (widget.reader as MonitoringHostContextReader).hostContextSupported) ...[
            Text(l.correlationHostActionsHint),
            const SizedBox(height: 8),
            Wrap(spacing: 8, runSpacing: 8, children: [
              GlassButton(key: const ValueKey('monitoring-context-ssh'), icon: Icons.terminal,
                label: l.correlationSshChooser, onPressed: busy || contextBusy || timeline?.busy == true || !widget.active || timeWindow.dirty ? null :
                  () => navigateHost(JobContextDestination.sshHost)),
              GlassButton(key: const ValueKey('monitoring-context-docker'), icon: Icons.view_in_ar_outlined,
                label: l.correlationDockerChooser, onPressed: busy || contextBusy || timeline?.busy == true || !widget.active || timeWindow.dirty ? null :
                  () => navigateHost(JobContextDestination.dockerHost)),
            ]),
            if (contextBusy) const LinearProgressIndicator(),
            if (contextError != null) InfoBanner(key: const ValueKey('monitoring-context-error'),
              message: l.correlationContextChanged, tone: BannerTone.danger),
            const SizedBox(height: 12),
          ],
          const SizedBox(height: 12),
          if (widget.correlationScope != null && widget.correlationAutomation is AutomationJobService &&
              widget.reader is MonitoringHostContextReader && (widget.reader as MonitoringHostContextReader).hostContextSupported)
            Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
            if (timeline?.snapshot != null)
            GlassSelect<bool>(key: const ValueKey('job-timeline-time-zone'),
              value: timelineLocalTime, expand: true, semanticLabel: l.correlationTimeZone,
              items: [GlassSelectItem(value: false, label: l.correlationUtcTime),
                GlassSelectItem(value: true, label: l.correlationLocalTime)],
              onChanged: (value) {
                setState(() => timelineLocalTime = value);
                final owner = timeline;
                if (owner != null) publishTimeline(owner, chartTimeline);
              }),
            JobMetricTimelineView(snapshot: timeline?.snapshot, busy: timeline?.busy ?? false,
              enabled: widget.active && !busy && !contextBusy && !timeWindow.dirty && currentCorrelationPanel() != null,
              failure: timeline?.failure,
              onRead: readTimeline,
              onCancel: () => setState(clearTimeline),
              onChoose: navigateProject,
              formatTime: (value) => jobMetricDisplayTime(value, timelineLocalTime),
              timeZoneLabel: timelineLocalTime ? l.correlationLocalTime : l.correlationUtcTime),
            ]),
          Text(l.monitoringBindingWindowHelp),
          const SizedBox(height: 12),
          GlassSelect<MonitoringPanel>(
            key: const ValueKey('monitoring-binding-panel'),
            value: panel,
            expand: true,
            semanticLabel: l.monitoringBindingPanel,
            items: [
              for (final kind in MonitoringPanel.values)
                GlassSelectItem(value: kind, label: monitoringPanelLabel(l, kind)),
            ],
            onChanged: !widget.active
                ? null
                : (value) {
                    invalidate();
                    setState(() => panel = value);
                  },
          ),
          const SizedBox(height: 12),
          MonitoringTimeWindowControls(controller: timeWindow, enabled: widget.active, keyPrefix: 'monitoring-binding'),
          const SizedBox(height: 12),
          Text(l.monitoringBindingLimits),
          const SizedBox(height: 12),
          Wrap(
            spacing: 8,
            runSpacing: 8,
            children: [
              GlassButton(
                key: const ValueKey('monitoring-binding-discover'),
                label: l.monitoringBindingIdentity,
                onPressed: busy || contextBusy || timeline?.busy == true || !widget.active || timeWindow.dirty ? null : () => run(discover: true),
              ),
              GlassButton(
                key: const ValueKey('monitoring-binding-load-panel'),
                label: l.monitoringBindingLoadPanel,
                onPressed: busy || contextBusy || timeline?.busy == true || !widget.active || timeWindow.dirty ? null : () => run(discover: false),
              ),
              if (busy || contextBusy)
                TextButton(
                  key: const ValueKey('monitoring-binding-cancel'),
                  onPressed: invalidate,
                  child: Text(l.monitoringCancel),
                ),
            ],
          ),
          if (busy) const Padding(padding: EdgeInsets.only(top: 12), child: LinearProgressIndicator()),
          if (error != null)
            SelectableText(
              error is MonitoringFailure
                  ? monitoringFailureMessage(l, error as MonitoringFailure)
                  : errorMessage(l, error!),
              key: const ValueKey('monitoring-binding-read-error'),
              style: TextStyle(color: Theme.of(context).colorScheme.error),
            ),
          if (result != null && !(widget.isPanelTransferred?.call(panel) ?? false))
            MonitoringBindingResultView(
              result: result!,
              jobTimeline: chartTimeline,
              requestedWindow: resultWindow,
              timeWindow: timeWindow,
              active: widget.active,
            ),
        ],
      ),
    );
  }
}

/// Passive projection of already accepted Native metadata. No service, resolver,
/// timer or execution capability. All child metric charts share one UTC axis.
class MonitoringBindingResultView extends StatelessWidget {
  const MonitoringBindingResultView({
    super.key,
    required this.result,
    this.jobTimeline,
    this.requestedWindow,
    this.timeWindow,
    this.active = true,
  });
  final BindingReadResult result;
  final MonitoringCorrelationTimeline? jobTimeline;
  final MonitoringTimeWindow? requestedWindow;
  final MonitoringTimeWindowController? timeWindow;
  final bool active;
  @override
  Widget build(BuildContext context) {
    final l = context.l10n, value = result;
    final candidate = jobTimeline;
    final acceptedTimeline = candidate != null && identical(candidate.owner.snapshot, candidate.snapshot) &&
      candidate.snapshot.selection.bindingId == result.bindingId &&
      candidate.snapshot.selection.bindingRevision == result.bindingRevision &&
      candidate.window == requestedWindow ? candidate : null;
    final indicesString = value.candidateIndices.join(', ');
    final router = GoRouter.maybeOf(context);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        const Divider(),
        Text(
          monitoringBindingStateLabel(l, value.state),
          key: const ValueKey('monitoring-binding-read-state'),
          style: Theme.of(context).textTheme.titleSmall,
        ),
        if (value.reason != null)
          Text(switch (value.reason!) {
            BindingReadReason.discoveryFailed => l.monitoringBindingDiscoveryFailed,
            BindingReadReason.discoveryInvalid => l.monitoringBindingDiscoveryInvalid,
            BindingReadReason.metricProbeFailed => l.monitoringBindingProbeFailed,
          }),
        if (value.preparationReason != null)
          Text(switch (value.preparationReason!) {
            BindingPreparationReason.missingMetrics => l.monitoringBindingMissingMetrics,
            BindingPreparationReason.probeFailed => l.monitoringBindingProbeFailed,
            BindingPreparationReason.selectorConflict => l.monitoringBindingSelectorConflict,
            BindingPreparationReason.queryBudget => l.monitoringBindingQueryBudget,
          }),
        if (value.state == BindingReadState.unavailable &&
            value.preparationReason == BindingPreparationReason.missingMetrics && value.panel != null)
          MonitoringPanelCollectionGuidance(
            panel: value.panel!,
            enabled: active,
            onOpenKubernetes: router == null ? null : () {
              // Fixed section navigation only. Retired result callbacks carry
              // no source/cluster identity and cannot act after replacement.
              if (active && context.mounted && identical(context.widget, this)) {
                router.go(AppRoutes.kubernetes);
              }
            },
          ),
        if (value.error != null) SelectableText(monitoringFailureMessage(l, value.error!)),
        if (value.preparationError != null) SelectableText(monitoringFailureMessage(l, value.preparationError!)),
        for (final metric in value.missingMetrics) SelectableText(metric),
        if (value.limited) Text(l.monitoringErrorLimit),
        for (final observation in [value.discovery, value.probe])
          if (observation != null) ...[
            if (observation.partial) Text(l.monitoringPartialResult),
            if (observation.error != null) SelectableText(monitoringFailureMessage(l, observation.error!)),
            for (final warning in observation.warnings) Text(l.monitoringWarning(warning)),
            for (final info in observation.infos) Text(l.monitoringInfo(info)),
          ],
        if (value.candidateIndices.isNotEmpty) ...[
          Text('${l.monitoringBindingOriginalIndices}: $indicesString'),
          ExpansionTile(
            title: Text(l.monitoringBindingCandidates),
            children: [
              for (final index in value.candidateIndices) ...[
                SelectableText('$index: ${jsonEncode(value.discovery!.data!.series[index].labels)}'),
                for (final sample in value.discovery!.data!.series[index].samples)
                  SelectableText('${sample.timestamp}: ${sample.display}'),
              ],
            ],
          ),
        ],
        if (value.discovery != null)
          ExpansionTile(
            title: Text(l.monitoringBindingDiscoveryDetails),
            children: [MonitoringResultView(result: value.discovery!)],
          ),
        if (value.probe != null)
          ExpansionTile(
            title: Text(l.monitoringBindingProbeDetails),
            children: [MonitoringResultView(result: value.probe!)],
          ),
        for (final query in value.queries)
          Padding(
            key: ValueKey('monitoring-panel-query-${query.metric.name}'),
            padding: const EdgeInsets.only(top: 16),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Align(
                  alignment: Alignment.centerLeft,
                  child: GlassBadge(label: monitoringUnitLabel(l, query.metric.unit), tone: GlassTone.info),
                ),
                const SizedBox(height: 8),
                Text(monitoringMetricLabel(l, query.metric)),
                const SizedBox(height: 8),
                if (query.error != null) SelectableText(monitoringFailureMessage(l, query.error!)),
                if (query.outcome != null)
                  MonitoringResultView(
                    key: ValueKey(query.metric),
                    result: query.outcome!,
                    jobTimeline: acceptedTimeline,
                    requestedWindow: requestedWindow,
                    unit: query.metric.unit,
                    timeWindow: timeWindow,
                    active: active,
                  ),
              ],
            ),
          ),
      ],
    );
  }
}
