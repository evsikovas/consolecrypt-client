import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_association_port.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_route_port.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_saved_route_port.dart';
import 'package:consolecrypt/monitoring/job_context_focus.dart';
import 'package:consolecrypt/monitoring/job_metric_correlation.dart';
import 'package:consolecrypt/monitoring/job_metric_timeline_flow.dart';
import 'package:consolecrypt/monitoring/job_metric_timeline_view.dart';
import 'package:consolecrypt/monitoring/metrics_ai_context.dart';
import 'package:consolecrypt/monitoring/metrics_ai_review_dialog.dart';
import 'package:consolecrypt/monitoring/metrics_ai_workspace_input.dart';
import 'package:consolecrypt/monitoring/monitoring_accepted_query.dart';
import 'package:consolecrypt/monitoring/monitoring_accepted_query_view.dart';
import 'package:consolecrypt/monitoring/monitoring_alert_models.dart';
import 'package:consolecrypt/monitoring/monitoring_alert_view.dart';
import 'package:consolecrypt/monitoring/monitoring_binding_models.dart';
import 'package:consolecrypt/monitoring/monitoring_binding_reader.dart';
import 'package:consolecrypt/monitoring/monitoring_bindings.dart';
import 'package:consolecrypt/monitoring/monitoring_grafana_prefill.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_observation_controls.dart';
import 'package:consolecrypt/monitoring/monitoring_panel_sync_consent.dart';
import 'package:consolecrypt/monitoring/monitoring_panel_sync_consent_dialog.dart';
import 'package:consolecrypt/monitoring/monitoring_pinned_observation.dart';
import 'package:consolecrypt/monitoring/monitoring_result_view.dart';
import 'package:consolecrypt/monitoring/monitoring_saved_panel_models.dart';
import 'package:consolecrypt/monitoring/monitoring_saved_panels.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window_controls.dart';
import 'package:material_ui/material_ui.dart';

export 'package:consolecrypt/monitoring/monitoring_result_view.dart';

class _History {
  _History(this.sourceId, this.sourceName, this.query, this.time);
  final String sourceId, sourceName;
  final MonitoringQuery query;
  final DateTime time;
}

/// No primary navigation registration. The host must supply a profile-scoped
/// service/store and dispose this subtree when its vault generation changes.
class MonitoringWorkspace extends StatefulWidget {
  const MonitoringWorkspace({
    super.key,
    required this.service,
    required this.store,
    this.active = true,
    this.credentials = const [],
    this.credentialsLoading = false,
    this.credentialsError,
    this.createCredential,
    this.hosts = const [],
    this.chooseHost,
    this.sshRouteHosts = const [],
    this.chooseSshRouteHost,
    this.timeWindow,
    this.pinControls,
    this.observation,
    this.pinnedObservation,
    this.pinnedObservationBuilder,
    this.onSourcesLoaded,
    this.importGrafana,
    this.openGrafana,
    this.openAgentReader,
    this.openAgentAssociation,
    this.requestedSource,
    this.requestedContext,
    this.onContextDismissed,
    this.onRequestedSourceHandled,
    this.correlationScope,
    this.correlationCurrent,
    this.correlationAutomation,
    this.onContextNavigation,
    this.reviewMetricsAi,
    this.panelSyncScope,
    this.panelSyncCurrent,
  });
  final Future<GrafanaPanelPrefill?> Function(List<MonitoringSource>)? importGrafana;
  final Future<void> Function()? openGrafana;
  final Future<void> Function(List<MonitoringSource>,List<MonitoringBinding>,bool Function())? openAgentReader;
  final Future<void> Function(bool Function())? openAgentAssociation;
  final JobCorrelationScope? correlationScope;
  final bool Function()? correlationCurrent;
  final AutomationService? correlationAutomation;
  final ValueChanged<JobContextFocus>? onContextNavigation;
  final MonitoringService service;
  final MonitoringSourceStore store;
  final RuntimeNoticeScope? panelSyncScope;
  final bool Function()? panelSyncCurrent;
  final bool active, credentialsLoading;
  final List<MonitoringCredentialRef> credentials;
  final String? credentialsError;
  final Future<MonitoringCredentialRef?> Function(SourceAuth auth)? createCredential;
  final List<MonitoringHostRef> hosts;
  final Future<MonitoringHostRef?> Function()? chooseHost;
  final List<MonitoringHostRef> sshRouteHosts;
  final Future<MonitoringHostRef?> Function()? chooseSshRouteHost;
  final MonitoringTimeWindowController? timeWindow;
  final Widget Function(MonitoringSource? source, String expression, bool instant, bool enabled)? pinControls;
  final Widget? pinnedObservation;
  final Widget Function(MonitoringCorrelationTimeline? timeline)? pinnedObservationBuilder;
  final MonitoringPinnedObservation? observation;
  final void Function(List<MonitoringSource> sources, bool readOnly)? onSourcesLoaded;
  final ({String id, int revision, Object token})? requestedSource;
  final JobContextFocus? requestedContext;
  final VoidCallback? onContextDismissed;
  final VoidCallback? onRequestedSourceHandled;
  final Future<void> Function(MonitoringSource source, MetricsAiWorkspaceInput input)? reviewMetricsAi;
  @override
  State<MonitoringWorkspace> createState() => _MonitoringWorkspaceState();
}

class _MonitoringWorkspaceState extends State<MonitoringWorkspace> {
  MonitoringCorrelationTimeline? chartTimeline;
  void timelineChanged(JobMetricTimelineFlow owner, MonitoringCorrelationTimeline? value) {
    if (!mounted) return;
    if (value == null) {
      if (identical(chartTimeline?.owner, owner)) setState(() => chartTimeline = null);
    } else if (widget.active && identical(owner.snapshot, value.snapshot)) {
      setState(() => chartTimeline = value);
    }
  }
  MonitoringCorrelationTimeline? get currentChartTimeline => widget.active &&
    chartTimeline != null && identical(chartTimeline!.owner.snapshot, chartTimeline!.snapshot)
      ? chartTimeline : null;
  List<MonitoringSource> sources = [];
  String? selected;
  Object? handledSourceRequest;
  bool loading = true, editing = false, busy = false, range = false;
  String? editId;
  Object? deleteReview;
  VoidCallback? dismissDeleteReview;
  SourceCapabilities? capabilities;
  QueryOutcome? result;
  MonitoringAcceptedQueryState acceptedQuery = MonitoringAcceptedQueryState.empty;
  MonitoringAlertSnapshot? alertResult;
  bool readingAlerts = false;
  SavedPanelRead? savedRead;
  GrafanaPanelPrefill? pendingImport;
  int importGeneration = 0;
  String? savedTitle;
  String? previewExpression;
  MonitoringAcceptedQueryState previewAccepted = MonitoringAcceptedQueryState.empty;
  MonitoringTimeWindow? savedWindow;
  MonitoringTimeWindow? resultWindow;
  late MonitoringTimeWindowController timeWindow;
  late bool ownsTimeWindow;
  bool reading = false;
  bool autoObserve = true, rollingObserve = false;
  MonitoringPinInterval observationInterval = MonitoringPinInterval.seconds15;
  int observationGeneration = 0;
  ({String bindingId, int revision, MonitoringPanel panel})? transferredBinding;
  StreamSubscription<MonitoringPinState>? observationSubscription;
  ({MonitoringPinPlan plan, MonitoringObservationValue value})? latestObservation;

  int? metadataCacheClearedGeneration;
  LabelValues? metadata;
  String? metadataLabel;
  Object? error;
  int generation = 0;
  int syncGeneration = 0;
  bool syncChecking = false, syncFailed = false;
  PanelSyncConsentController? syncConsentOwner;
  VoidCallback? dismissSyncConsent;
  void invalidateSyncConsent() {
    syncConsentOwner?.retire(notify:false);
    final dismiss=dismissSyncConsent;dismissSyncConsent=null;dismiss?.call();
  }
  MonitoringPanelSyncConsentPort? get syncConsentPort {
    final store=widget.store;
    return store is MonitoringPanelSyncConsentCapability?(store as MonitoringPanelSyncConsentCapability).panelSyncConsentPort:null;
  }
  Future<void> reviewPanelPolicy() async {
    final port=syncConsentPort,scope=widget.panelSyncScope,store=widget.store,service=widget.service,live=widget.panelSyncCurrent;
    if(port==null||scope==null||!widget.active||live?.call()!=true||syncConsentOwner!=null)return;
    final catalog=catalogGeneration,fingerprint=source?.fingerprint;
    bool current()=>mounted&&widget.active&&live?.call()==true&&scope.same(widget.panelSyncScope)&&
      identical(widget.store,store)&&identical(widget.service,service)&&identical(syncConsentPort,port)&&
      catalogGeneration==catalog&&source?.fingerprint==fingerprint;
    final owner=PanelSyncConsentController(port:port,scope:scope,isOriginal:current);
    syncConsentOwner=owner;
    try{
      await showAppDialog<void>(context,secure:true,builder:(dialogContext){
        final navigator=Navigator.of(dialogContext),route=ModalRoute.of(dialogContext);
        dismissSyncConsent=()=>WidgetsBinding.instance.addPostFrameCallback((_){
          if(navigator.mounted&&route?.isActive==true)navigator.removeRoute(route!);
        });
        return MonitoringPanelSyncConsentDialog(owner:owner,onClose:(){if(route?.isCurrent==true)navigator.pop();});
      });
    }finally{
      if(identical(syncConsentOwner,owner)){syncConsentOwner=null;dismissSyncConsent=null;}
      owner.dispose();
    }
  }
  MonitoringPanelSyncSnapshot? get panelSyncSnapshot {
    final scope = widget.panelSyncScope, store = widget.store;
    if (scope == null || widget.panelSyncCurrent?.call() != true || store is! MonitoringPanelSyncDiagnostics) return null;
    final snapshot = (store as MonitoringPanelSyncDiagnostics).panelSync;
    return snapshot?.matches(scope.profileId,scope.nonce) == true ? snapshot : null;
  }
  Future<void> checkPanelSync() async {
    final scope = widget.panelSyncScope, originalStore = widget.store, originalService = widget.service;
    final live = widget.panelSyncCurrent;
    if (syncChecking || scope == null || originalStore is! MonitoringPanelSyncDiagnostics || live?.call() != true) return;
    final reader = originalStore as MonitoringPanelSyncDiagnostics;
    if (!reader.canCheckPanelSync) return;
    final token = ++syncGeneration, fingerprint = source?.fingerprint;
    bool current() => mounted && token == syncGeneration && widget.active && live?.call() == true &&
      identical(widget.store,originalStore) && identical(widget.service,originalService) &&
      scope.same(widget.panelSyncScope) && source?.fingerprint == fingerprint;
    if (!current()) return;
    setState(() { syncChecking = true; syncFailed = false; });
    try { await reader.checkPanelSync(original:scope,isCurrent:current); }
    catch (_) { if (current()) setState(() => syncFailed = true); }
    finally { if (mounted && token == syncGeneration) setState(() => syncChecking = false); }
  }

  MonitoringCorrelationPanel? acceptedPanelForCorrelation(MonitoringBinding binding, MonitoringPanel panel) {
    MonitoringCorrelationPanel? accepted(MonitoringPinPlan plan, MonitoringObservationValue? value, Object? failure) {
      if (value is! MonitoringObservedBindingPanel ||
          value.window == null ||
          !monitoringObservationValueMatchesPlan(plan, value) ||
          plan.source.fingerprint != source?.fingerprint ||
          failure != null && !monitoringMayRetainAcceptedQuery(failure)) {
        return null;
      }
      final target = plan.target;
      if (target is! MonitoringBindingPanelTarget ||
          target.panel != panel ||
          jsonEncode(monitoringBindingObservationIdentity(target.binding)) !=
              jsonEncode(monitoringBindingObservationIdentity(binding))) {
        return null;
      }
      return (read: value.read, window: value.window!);
    }

    final state = widget.observation?.state, plan = widget.observation?.state.plan;
    if (plan != null) return accepted(plan, state?.value, state?.error);
    final retained = latestObservation;
    return retained == null ? null : accepted(retained.plan, retained.value, null);
  }

  bool aiReviewing = false;
  Future<void> reviewMetricsAi({bool previewOnly = false}) async {
    final review = widget.reviewMetricsAi, capturedSource = source;
    if (review == null ||
        capturedSource == null ||
        !widget.active ||
        busy ||
        editing ||
        aiReviewing ||
        widget.store.readOnly ||
        capturedSource.readOnly) {
      return;
    }
    final token = generation, catalog = catalogGeneration, service = widget.service, store = widget.store;
    final owner = widget.observation, observation = owner?.state;
    final accepted = previewOnly ? previewAccepted : acceptedQuery;
    final alerts = previewOnly ? null : alertResult;
    if (previewOnly &&
        (accepted.accepted == null || savedRead?.outcome != accepted.accepted!.outcome || previewExpression == null)) {
      return;
    }
    final observed =
        !previewOnly &&
        observation?.plan?.source.fingerprint == capturedSource.fingerprint &&
        observation?.value?.queryOutcome != null &&
        observation?.lastSuccessUtc != null;
    final Object lease = observed ? owner! : accepted.identity?.scope ?? Object();
    MetricsAiMetricInput Function(MetricsAiFence)? metric;
    if (observed) {
      final snapshot = observation!;
      metric = (fence) => MetricsAiMetricInput(
        outcome: snapshot.value!.queryOutcome!,
        select: (series, samples) => MetricsAiMetricSelection.fromObservation(
          snapshot,
          fence,
          originalOwner: lease,
          seriesIndex: series,
          sampleIndices: samples,
        ),
      );
    } else if (accepted.accepted != null) {
      metric = (fence) => MetricsAiMetricInput(
        outcome: accepted.accepted!.outcome,
        select: (series, samples) =>
            MetricsAiMetricSelection.fromAccepted(accepted, fence, seriesIndex: series, sampleIndices: samples),
      );
    }
    bool current() =>
        mounted &&
        widget.active &&
        !editing &&
        !widget.store.readOnly &&
        token == generation &&
        catalog == catalogGeneration &&
        identical(widget.service, service) &&
        identical(widget.store, store) &&
        source?.fingerprint == capturedSource.fingerprint &&
        source?.readOnly == false &&
        (previewOnly
            ? savedRead?.outcome == accepted.accepted?.outcome && identical(previewAccepted.accepted, accepted.accepted)
            : observed
            ? owner?.state.value != null && owner?.state.plan?.requestIdentity == observation?.plan?.requestIdentity
            : accepted.accepted == null || identical(acceptedQuery.accepted, accepted.accepted));
    setState(() => aiReviewing = true);
    try {
      await review(
        capturedSource,
        MetricsAiWorkspaceInput(metricsLease: lease, metric: metric, alerts: alerts, isCurrent: current),
      );
    } finally {
      if (mounted) setState(() => aiReviewing = false);
    }
  }

  int catalogGeneration = 0;
  final history = <_History>[];
  final expression = TextEditingController(text: 'up');
  final name = TextEditingController(),
      endpoint = TextEditingController(),
      ca = TextEditingController(),
      routeRef = TextEditingController(),
      routeDestinationPortInput = TextEditingController();
  final label = TextEditingController(text: '__name__'), metadataFilter = TextEditingController();
  String? selectedCredential;
  SourceRoute route = SourceRoute.direct;
  String? routeHostId, routeAssociationId, routeDestinationIp;
  AgentAssociationStatus? sourceAssociationStatus;
  SourceAuth auth = SourceAuth.none;
  String authLabel(SourceAuth value) => switch (value) {
    SourceAuth.none => l.monitoringNoAuth,
    SourceAuth.bearerRef => l.monitoringBearerAuth,
    SourceAuth.basicRef => l.monitoringBasicAuth,
  };
  List<MonitoringCredentialRef> get eligibleCredentials =>
      widget.credentials.where((c) => auth != SourceAuth.basicRef || (c.username?.trim().isNotEmpty ?? false)).toList();
  MonitoringSource? get source => sources.where((s) => s.id == selected).firstOrNull;
  AppLocalizations get l => context.l10n;
  @override
  void initState() {
    super.initState();
    attachTimeWindow();
    attachObservation();
    metadataFilter.addListener(_metadataChanged);
    endpoint.addListener(_metadataChanged);
    expression.addListener(customDraftChanged);
    load();
  }

  void attachObservation() {
    widget.observation?.setForegroundActive(widget.active);
    final owner = widget.observation;
    observationSubscription = owner?.changes.listen((_) {
      if (!mounted || !identical(owner, widget.observation)) return;
      // Restart emits stopped then seeded state. Consume the current owner
      // snapshot so an older queued restart event cannot clear the new draft.
      final value = owner.state;
      setState(() {
        final latest = latestObservation;
        if (latest != null && value.plan?.requestIdentity == latest.plan.requestIdentity && value.hasAcceptedResult) {
          transferManualResult(latest.plan.target);
        }
        if (value.phase == MonitoringPinPhase.stopped && value.stopReason != MonitoringPinStop.autoDisabled) {
          latestObservation = null;
        }
      });
    });
  }

  void cancelForegroundObservation() {
    observationGeneration++;
    if (latestObservation != null && mounted) {
      setState(() => latestObservation = null);
    } else {
      latestObservation = null;
    }
    widget.observation?.cancelForeground();
  }

  /// Once adopted by the observation owner, the first manual snapshot is not
  /// rendered a second time or resurrected after a permanent observation error.
  void transferManualResult(MonitoringObservationTarget target) {
    switch (target) {
      case MonitoringCustomInstantTarget() || MonitoringCustomRangeTarget():
        result = null;
        acceptedQuery = MonitoringAcceptedQueryState.empty;
        resultWindow = null;
      case MonitoringSavedPanelTarget():
        savedRead = null;
      case MonitoringBindingPanelTarget(:final binding, :final panel):
        transferredBinding = (bindingId: binding.id, revision: binding.revision, panel: panel);
    }
  }

  MonitoringObservationContext? observationContext(MonitoringPinPlan plan, List<MonitoringSource> values) {
    final savedSource = values.where((s) => s.id == plan.source.id).firstOrNull;
    if (savedSource == null || widget.store.readOnly) return null;
    final bindingStore = widget.store is MonitoringBindingStore ? widget.store as MonitoringBindingStore : null;
    final panelStore = widget.store is MonitoringSavedPanelStore ? widget.store as MonitoringSavedPanelStore : null;
    return MonitoringObservationContext(
      source: savedSource,
      binding: plan.target is MonitoringBindingPanelTarget
          ? bindingStore?.bindings
                .where((b) => b.id == (plan.target as MonitoringBindingPanelTarget).binding.id)
                .firstOrNull
          : null,
      panel: plan.target is MonitoringSavedPanelTarget
          ? panelStore?.panels.where((p) => p.id == (plan.target as MonitoringSavedPanelTarget).panel.id).firstOrNull
          : null,
    );
  }

  void acknowledgeObservationCatalog(List<MonitoringSource> values) {
    final owner = widget.observation, plan = widget.observation?.state.plan;
    if (owner != null && plan != null) {
      owner.contextChanged(observationContext(plan, values), readOnly: widget.store.readOnly);
    }
    final latest = latestObservation;
    if (latest != null && observationContext(latest.plan, values)?.matches(latest.plan) != true) {
      latestObservation = null;
      observationGeneration++;
    }
  }

  MonitoringObservationWindow observationWindow(MonitoringTimeWindow window) => !rollingObserve
      ? MonitoringFixedObservationWindow(window)
      : MonitoringRollingObservationWindow(preset: timeWindow.preset, stepSeconds: window.stepSeconds);

  /// Capture options and scope BEFORE the manual Native read. Later edits cannot
  /// arm a timer with a response produced under different visible settings.
  ValueChanged<MonitoringObservationValue> beginObservation(MonitoringObservationTarget target) {
    final owner = widget.observation, selectedSource = source;
    if (owner == null || selectedSource == null || !selectedSource.allowsBackgroundObservation) return (_) {};
    final plan = MonitoringPinPlan.forTarget(source: selectedSource, target: target, interval: observationInterval);
    plan.validate();
    final token = observationGeneration, service = widget.service, store = widget.store, auto = autoObserve;
    return (value) {
      if (!mounted ||
          !widget.active ||
          token != observationGeneration ||
          !identical(service, widget.service) ||
          !identical(store, widget.store) ||
          value.failure != null ||
          !monitoringObservationValueMatchesPlan(plan, value) ||
          observationContext(plan, sources)?.matches(plan) != true) {
        return;
      }
      latestObservation = (plan: plan, value: value);
      if (auto && owner.observeForeground(plan, value)) transferManualResult(plan.target);
    };
  }

  ValueChanged<BindingReadResult>? beginBindingObservation(
    MonitoringBinding binding,
    MonitoringPanel panel,
    MonitoringTimeWindow window,
  ) {
    transferredBinding = null;
    final accept = beginObservation(
      MonitoringBindingPanelTarget(binding: binding, panel: panel, windowPolicy: observationWindow(window)),
    );
    return (value) {
      accept(MonitoringObservedBindingPanel(value, window));
    };
  }

  Widget observationControls() => MonitoringObservationControls(
    owner: widget.observation!,
    auto: source?.allowsBackgroundObservation==true && autoObserve,
    interval: observationInterval,
    rolling: rollingObserve,
    enabled: widget.active && !busy && !editing && source?.allowsBackgroundObservation==true,
    rollingAvailable: timeWindow.preset != MonitoringTimePreset.custom,
    acceptedPlan: latestObservation?.plan,
    acceptedValue: latestObservation?.value,
    isAcceptedCurrent: (plan, value) =>
        mounted &&
        widget.active &&
        !editing &&
        identical(latestObservation?.plan, plan) &&
        identical(latestObservation?.value, value) &&
        observationContext(plan, sources)?.matches(plan) == true,
    onAuto: (value) {
      observationGeneration++;
      if (!value) widget.observation?.disableForeground();
      final paused = widget.observation?.state;
      if (paused case MonitoringPinState(
        stopReason: MonitoringPinStop.autoDisabled,
        plan: final plan?,
        value: final value?,
      )) {
        latestObservation = (plan: plan, value: value);
      }
      setState(() => autoObserve = value);
    },
    onInterval: (value) {
      cancelForegroundObservation();
      setState(() => observationInterval = value);
    },
    onRolling: (value) {
      cancelForegroundObservation();
      setState(() => rollingObserve = value);
    },
  );

  void attachTimeWindow() {
    ownsTimeWindow = widget.timeWindow == null;
    timeWindow = widget.timeWindow ?? MonitoringTimeWindowController();
    timeWindow.addListener(timeChanged);
  }

  void detachTimeWindow() {
    timeWindow.removeListener(timeChanged);
    if (ownsTimeWindow) timeWindow.dispose();
  }

  void timeChanged() {
    cancelForegroundObservation();
    final wasReading = reading;
    if (wasReading) generation++;
    widget.service.cancel();
    if (mounted) {
      setState(() {
        if (timeWindow.preset == MonitoringTimePreset.custom) rollingObserve = false;
        if (wasReading) {
          busy = false;
          reading = false;
          readingAlerts = false;
        }
        result = null;
        acceptedQuery = MonitoringAcceptedQueryState.empty;
        alertResult = null;
        savedRead = null;
        resultWindow = null;
        error = null;
      });
    }
  }

  void customDraftChanged() {
    cancelForegroundObservation();
    final identity = acceptedQuery.identity;
    if (!mounted || identity == null || identity.expression == expression.text.trim()) return;
    final wasPending = acceptedQuery.pendingToken != null;
    if (wasPending) {
      generation++;
      widget.service.cancel();
    }
    setState(() {
      if (wasPending) {
        busy = false;
        reading = false;
        readingAlerts = false;
      }
      result = null;
      acceptedQuery = MonitoringAcceptedQueryState.empty;
      resultWindow = null;
      error = null;
    });
  }

  void _metadataChanged() {
    if (mounted) setState(() {});
  }

  @override
  void didUpdateWidget(MonitoringWorkspace oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!widget.active || !identical(oldWidget.store,widget.store) || !identical(oldWidget.service,widget.service) ||
        !(oldWidget.panelSyncScope?.same(widget.panelSyncScope) ?? widget.panelSyncScope == null)) {
      if (selectedAgentRouteId != null) cancel();
      clearAgentRouteSelection();
      invalidateSyncConsent();
      syncGeneration++;
      syncChecking = false;
      syncFailed = false;
    }
    if (!widget.active || !identical(oldWidget.service, widget.service) || !identical(oldWidget.store, widget.store)) {
      invalidateDeleteReview();
    }
    if (!identical(oldWidget.observation, widget.observation)) {
      oldWidget.observation?.setForegroundActive(false);
      unawaited(observationSubscription?.cancel());
      latestObservation = null;
      observationGeneration++;
      attachObservation();
    } else if (oldWidget.active != widget.active) {
      widget.observation?.setForegroundActive(widget.active);
      if (!widget.active) {
        latestObservation = null;
        observationGeneration++;
      }
    }
    if (!identical(oldWidget.service, widget.service) || !identical(oldWidget.store, widget.store)) {
      generation++;
      catalogGeneration++;
      oldWidget.service.cancel();
      busy = false;
      reading = false;
      readingAlerts = false;
      alertResult = null;
      savedRead = null;
      result = null;
      acceptedQuery = MonitoringAcceptedQueryState.empty;
      capabilities = null;
      metadata = null;
      error = null;
      sources = [];
      selected = null;
      editing = false;
      loading = true;
      if (widget.active) load();
    }
    if (oldWidget.timeWindow != widget.timeWindow) {
      detachTimeWindow();
      attachTimeWindow();
      timeChanged();
    }
    if (oldWidget.active && !widget.active) {
      alertResult = null;
      savedRead = null;
      result = null;
      acceptedQuery = MonitoringAcceptedQueryState.empty;
      resultWindow = null;
      generation++;
      widget.service.cancel();
      busy = false;
      reading = false;
      readingAlerts = false;
    }
    final request = widget.requestedSource;
    if (widget.active &&
        request != null &&
        !identical(request.token, handledSourceRequest) &&
        (!oldWidget.active || !identical(request.token, oldWidget.requestedSource?.token))) {
      cancel();
      selected = null;
      editing = false;
      loading = true;
      load(preserveSelection: true);
    } else if (!oldWidget.active && widget.active) {
      load(preserveSelection: true);
    }
  }

  List<SavedMonitoringPanel> get savedPanels =>
      widget.store is MonitoringSavedPanelStore ? (widget.store as MonitoringSavedPanelStore).panels : const [];
  String get savedCatalogFingerprint =>
      jsonEncode(savedPanels.map((p) => [p.id, p.revision, p.state.name, p.definition.toJson()]).toList());

  Future<void> load({bool preserveSelection = false}) async {
    invalidateSyncConsent();
    invalidateDeleteReview();
    final token = ++catalogGeneration;
    final previous = source?.fingerprint;
    final previousSaved = savedCatalogFingerprint;
    try {
      final loaded = await widget.store.load();
      if (!mounted || token != catalogGeneration) return;
      acknowledgeObservationCatalog(loaded);
      widget.onSourcesLoaded?.call(loaded, widget.store.readOnly);
      setState(() {
        sources = loaded;
        error = null;
        selected = preserveSelection && loaded.any((s) => s.id == selected) ? selected : loaded.firstOrNull?.id;
        if (!preserveSelection) editing = loaded.isEmpty;
        final request = widget.requestedSource;
        if (request != null && !identical(request.token, handledSourceRequest)) {
          selected = loaded
              .where((source) => source.id == request.id && source.revision == request.revision)
              .firstOrNull
              ?.id;
          handledSourceRequest = request.token;
          editing = false;
          error = selected == null ? const MonitoringFailure.local(MonitoringErrorCode.sourceChanged) : null;
        }
        if (previous != source?.fingerprint || previousSaved != savedCatalogFingerprint) {
          generation++;
          widget.service.cancel();
          busy = false;
          reading = false;
          readingAlerts = false;
          result = null;
          acceptedQuery = MonitoringAcceptedQueryState.empty;
          alertResult = null;
          savedRead = null;
          capabilities = null;
          metadata = null;
        }
      });
      if (widget.requestedSource case final request?) {
        if (identical(request.token, handledSourceRequest)) {
          widget.onRequestedSourceHandled?.call();
        }
      }
    } catch (e) {
      if (mounted && token == catalogGeneration) {
        setState(() {
          error = e;
          savedRead = null;
        });
      }
    } finally {
      if (mounted && token == catalogGeneration) setState(() => loading = false);
    }
  }

  @override
  void dispose() {
    invalidateSyncConsent();
    syncGeneration++;
    invalidateDeleteReview();
    widget.observation?.setForegroundActive(false);
    unawaited(observationSubscription?.cancel());
    observationGeneration++;
    generation++;
    catalogGeneration++;
    widget.service.cancel();
    detachTimeWindow();
    for (final c in [expression, name, endpoint, ca, routeRef, routeDestinationPortInput, label, metadataFilter]) {
      c.dispose();
    }
    super.dispose();
  }

  void cancel() {
    invalidateDeleteReview();
    cancelForegroundObservation();
    generation++;
    widget.service.cancel();
    setState(() {
      acceptedQuery = MonitoringAcceptedQueryState.empty;
      result = null;
      resultWindow = null;
      busy = false;
      reading = false;
      readingAlerts = false;
    });
  }

  void choose(String id) {
    if (widget.requestedContext case final focus? when focus.metadata.sourceId != id) {
      widget.onContextDismissed?.call();
    }
    cancel();
    setState(() {
      selected = id;
      clearAgentRouteSelection();
      capabilities = null;
      result = null;
      acceptedQuery = MonitoringAcceptedQueryState.empty;
      alertResult = null;
      savedRead = null;
      metadata = null;
      error = null;
      editing = false;
    });
  }

  void edit({bool create = false}) {
    cancel();
    final s = create ? null : source;
    setState(() {
      editing = true;
      clearAgentRouteSelection();
      capabilities = null;
      result = null;
      acceptedQuery = MonitoringAcceptedQueryState.empty;
      alertResult = null;
      savedRead = null;
      metadata = null;
      editId = s?.id;
      name.text = s?.name ?? '';
      endpoint.text = s?.endpoint ?? '';
      ca.text = s?.customCaPem ?? '';
      selectedCredential = s?.credentialId;
      routeRef.text = s?.routeId ?? '';
      routeHostId = s?.hostId;
      routeAssociationId = s?.associationId;
      routeDestinationIp = s?.destinationIp;
      routeDestinationPortInput.text = s?.destinationPort?.toString() ?? '';
      sourceAssociationStatus = null;
      route = s?.route ?? SourceRoute.direct;
      auth = s?.auth ?? SourceAuth.none;
      error = null;
    });
  }

  Future<void> saveSource() async {
    if (busy) return;
    final token = ++generation;
    setState(() {
      busy = true;
      error = null;
    });
    try {
      final s = MonitoringSource(
        id: editId ?? '',
        revision: editId != null && editId == source?.id ? source!.revision : 0,
        name: name.text.trim(),
        endpoint: endpoint.text.trim(),
        route: route,
        routeId: route == SourceRoute.sshTunnel || route == SourceRoute.agentTunnel ? routeRef.text.trim() : null,
        hostId: route == SourceRoute.sshHost || route == SourceRoute.agentDestination ? routeHostId : null,
        associationId: route == SourceRoute.agentDestination ? routeAssociationId : null,
        destinationIp: route == SourceRoute.agentDestination ? routeDestinationIp : null,
        destinationPort: route == SourceRoute.agentDestination ? int.tryParse(routeDestinationPortInput.text.trim()) : null,
        auth: auth,
        credentialId: selectedCredential,
        customCaPem: ca.text.trim().isEmpty ? null : ca.text.trim(),
      );
      if (auth != SourceAuth.none &&
          (!eligibleCredentials.any((c) => c.id == selectedCredential) ||
              widget.credentialsLoading ||
              widget.credentialsError != null)) {
        throw const MonitoringFailure.local(MonitoringErrorCode.authUnavailable);
      }
      s.validate(allowEmptyId: editId == null);
      final saved = await widget.store.upsert(s, create: editId == null);
      final updated = await widget.store.load();
      if (!mounted || token != generation) return;
      acknowledgeObservationCatalog(updated);
      widget.onSourcesLoaded?.call(updated, widget.store.readOnly);
      setState(() {
        sources = updated;
        selected = saved.id;
        editing = false;
        editId = null;
        capabilities = null;
        result = null;
        acceptedQuery = MonitoringAcceptedQueryState.empty;
        alertResult = null;
        savedRead = null;
        metadata = null;
        error = null;
      });
    } catch (e) {
      if (mounted && token == generation) setState(() => error = e);
    } finally {
      if (mounted && token == generation) setState(() => busy = false);
    }
  }

  int? selectedAgentRouteVersion;
  String? selectedAgentRouteId;
  String? agentRouteSourceFingerprint;
  AgentTunnelSavedIntentCache? get agentRouteCache {
    final service = widget.service;
    final port = service is AgentTunnelRouteCapability ?
      (service as AgentTunnelRouteCapability).agentTunnelRoutePort : null;
    return port is AgentTunnelSavedRoutePort ? (port as AgentTunnelSavedRoutePort).savedIntents : null;
  }
  List<AgentTunnelRetainedRoute> get retainedAgentRoutes {
    final scope = widget.panelSyncScope;
    if (!widget.active || scope == null || widget.panelSyncCurrent?.call() != true) return const [];
    final selectedSource = source;
    return (agentRouteCache?.retainedRoutes(scope) ?? const <AgentTunnelRetainedRoute>[])
      .where((row) => selectedSource?.route != SourceRoute.sshHost || row.hostId == selectedSource?.hostId)
      .where((row) => selectedSource?.route != SourceRoute.agentDestination ||
        row.associationId == selectedSource?.associationId && row.hostId == selectedSource?.hostId &&
        row.destinationIp == selectedSource?.destinationIp && row.destinationPort == selectedSource?.destinationPort)
      .toList(growable: false);
  }
  AgentAssociationPort? get sourceAssociationPort {
    final service=widget.service;
    return service is AgentAssociationCapability ? (service as AgentAssociationCapability).agentAssociationPort : null;
  }
  List<AgentAssociationRecord> get sourceAgentAssociations {
    final scope=widget.panelSyncScope, status=sourceAssociationStatus;
    if (!widget.active || scope==null || widget.panelSyncCurrent?.call()!=true || status==null || !status.scope.same(scope)) return const [];
    return status.records.where((row)=>row.canRequestTunnel && status.hosts.any((host)=>
      host.id==row.hostId && host.revision==row.hostRevision)).toList(growable:false);
  }
  String sourceAgentHostName(String? id) => widget.sshRouteHosts.where((host)=>host.id==id).firstOrNull?.name ??
    sourceAssociationStatus?.hosts.where((host)=>host.id==id).firstOrNull?.name ?? l.monitoringSshRouteMissing;
  Future<void> loadSourceAgentAssociations() async {
    final scope=widget.panelSyncScope, port=sourceAssociationPort;
    if (busy || !widget.active || !editing || route!=SourceRoute.agentDestination || scope==null || port==null || widget.panelSyncCurrent?.call()!=true) return;
    final token=++generation, originalEdit=editId, service=widget.service, store=widget.store;
    setState(() {busy=true;error=null;});
    bool current()=>mounted && widget.active && editing && route==SourceRoute.agentDestination && token==generation && editId==originalEdit &&
      identical(service,widget.service) && identical(store,widget.store) && scope.same(widget.panelSyncScope) && widget.panelSyncCurrent?.call()==true;
    try {
      final status=await port.status(scope); // Existing local metadata read; no preparation or grant.
      if (!current()) return;
      if (!status.scope.same(scope)) throw const MonitoringFailure.local(MonitoringErrorCode.invalidResponse);
      setState(()=>sourceAssociationStatus=status);
    } catch(e) {if(current()) setState(()=>error=e);}
    finally {if(mounted && token==generation) setState(()=>busy=false);}
  }
  void chooseSourceAgentDestination(String? id) {
    if (busy || !widget.active || !editing || route!=SourceRoute.agentDestination) return;
    final row=sourceAgentAssociations.where((row)=>row.id==id).firstOrNull;
    if (row==null) return;
    setState(() {routeAssociationId=row.id;routeHostId=row.hostId;routeDestinationIp??='127.0.0.1';});
  }
  void openSourceAgentSetup() {
    final open=widget.openAgentAssociation;
    final scope=widget.panelSyncScope;
    if (busy || !widget.active || !editing || route!=SourceRoute.agentDestination || open==null || scope==null || widget.panelSyncCurrent?.call()!=true) return;
    final service=widget.service, store=widget.store, originalEdit=editId;
    bool current()=>mounted && widget.active && editing && route==SourceRoute.agentDestination && editId==originalEdit &&
      identical(service,widget.service) && identical(store,widget.store) && scope.same(widget.panelSyncScope) && widget.panelSyncCurrent?.call()==true;
    open(current); // Same explicit existing association workflow, no automatic open/read.
  }
  bool agentRouteMatchesSource(AgentTunnelRetainedRoute row, MonitoringSource source) =>
    source.route == SourceRoute.agentDestination
      ? row.associationId == source.associationId && row.hostId == source.hostId &&
        row.destinationIp == source.destinationIp && row.destinationPort == source.destinationPort
      : row.matchesEndpoint(source.endpoint);
  AgentTunnelRetainedRoute? get selectedAgentRoute => agentRouteSourceFingerprint == source?.fingerprint
    ? retainedAgentRoutes.where((row) => row.routeId == selectedAgentRouteId &&
        row.metadataVersion == selectedAgentRouteVersion).firstOrNull : null;
  void clearAgentRouteSelection() {
    selectedAgentRouteVersion = null;
    selectedAgentRouteId = null;
    agentRouteSourceFingerprint = null;
  }

  void changeSourceRoute(SourceRoute next) {
    if (busy || !editing) return;
    cancel();
    setState(() {
      route = next;
      if (next == SourceRoute.agentDestination) routeDestinationIp ??= '127.0.0.1';
      if (next != SourceRoute.sshHost && next != SourceRoute.agentDestination) routeHostId = null;
      if (next != SourceRoute.agentDestination) {
        routeAssociationId = null; routeDestinationIp = null; routeDestinationPortInput.clear();
        sourceAssociationStatus = null;
      }
    });
  }

  Future<void> chooseSourceSshHost() async {
    final choose = widget.chooseSshRouteHost;
    if (busy || !widget.active || !editing || route != SourceRoute.sshHost || choose == null) return;
    final token = ++generation, originalEdit = editId;
    final store = widget.store, service = widget.service;
    setState(() { busy = true; error = null; });
    bool current() => mounted && widget.active && editing && token == generation && editId == originalEdit &&
      identical(widget.store, store) && identical(widget.service, service) && route == SourceRoute.sshHost;
    try {
      final selected = await choose();
      if (!current() || selected == null) return;
      if (!widget.sshRouteHosts.any((host) => host.id == selected.id)) {
        throw const MonitoringFailure.local(MonitoringErrorCode.invalidConfig);
      }
      setState(() => routeHostId = selected.id);
    } catch (e) {
      if (current()) setState(() => error = e);
    } finally {
      if (mounted && token == generation) setState(() => busy = false);
    }
  }

  Future<void> addCredential() async {
    final create = widget.createCredential;
    if (busy || !widget.active || create == null || !editing) return;
    final token = ++generation;
    final requestedAuth = auth;
    setState(() {
      busy = true;
      error = null;
    });
    try {
      final created = await create(requestedAuth);
      if (!mounted || !widget.active || token != generation || !editing || auth != requestedAuth) return;
      if (created == null) return;
      if (auth == SourceAuth.basicRef && (created.username?.trim().isEmpty ?? true)) {
        throw const MonitoringFailure.local(MonitoringErrorCode.authUnavailable);
      }
      setState(() => selectedCredential = created.id);
    } catch (e) {
      if (mounted && widget.active && token == generation) setState(() => error = e);
    } finally {
      if (mounted && token == generation) setState(() => busy = false);
    }
  }

  void invalidateDeleteReview() {
    deleteReview = null;
    final dismiss = dismissDeleteReview;
    dismissDeleteReview = null;
    dismiss?.call();
  }

  Future<void> reviewDeleteSource() async {
    final original = source, store = widget.store, service = widget.service;
    if (original == null || busy || !widget.active || store.readOnly || original.readOnly || sourceHasReferences) {
      return;
    }
    final ticket = Object(), request = generation, catalog = catalogGeneration;
    deleteReview = ticket;
    bool current() =>
        mounted &&
        widget.active &&
        identical(deleteReview, ticket) &&
        identical(widget.store, store) &&
        identical(widget.service, service) &&
        generation == request &&
        catalogGeneration == catalog &&
        !busy &&
        !store.readOnly &&
        source?.fingerprint == original.fingerprint &&
        !sourceHasReferences;
    bool accepted = false;
    try {
      accepted =
          await showAppDialog<bool>(
            context,
            builder: (dialogContext) {
              final navigator = Navigator.of(dialogContext), route = ModalRoute.of(dialogContext);
              dismissDeleteReview = () => WidgetsBinding.instance.addPostFrameCallback((_) {
                if (navigator.mounted && route?.isActive == true) navigator.removeRoute(route!);
              });
              return GlassDialog(
                key: const ValueKey('monitoring-delete-dialog'),
                title: l.monitoringDeleteNamedSource(original.name),
                icon: Icons.delete_outline,
                iconTone: GlassTone.danger,
                width: 520,
                content: Column(
                  mainAxisSize: MainAxisSize.min,
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    Text(l.monitoringDeleteHint),
                    const SizedBox(height: 12),
                    Text(l.monitoringDeleteCredentialsKept),
                  ],
                ),
                secondaryActions: [
                  GlassButton(
                    key: const ValueKey('monitoring-delete-cancel'),
                    label: l.monitoringKeepSource,
                    autofocus: true,
                    onPressed: () {
                      if (navigator.mounted && route?.isCurrent == true) navigator.pop(false);
                    },
                  ),
                ],
                primaryAction: GlassButton.destructive(
                  key: const ValueKey('monitoring-delete-confirm'),
                  label: l.monitoringConfirmDelete,
                  onPressed: () {
                    if (navigator.mounted && route?.isCurrent == true) navigator.pop(current());
                  },
                ),
              );
            },
          ) ??
          false;
      accepted = accepted && current();
    } finally {
      if (identical(deleteReview, ticket)) {
        deleteReview = null;
        dismissDeleteReview = null;
      }
    }
    if (accepted) await deleteSource(original, store, service);
  }

  Future<void> deleteSource(MonitoringSource original, MonitoringSourceStore store, MonitoringService service) async {
    bool sameSession() =>
        mounted && widget.active && identical(widget.store, store) && identical(widget.service, service);
    if (!sameSession() ||
        busy ||
        source?.fingerprint != original.fingerprint ||
        store.readOnly ||
        sourceHasReferences) {
      return;
    }
    cancel();
    final token = ++generation;
    setState(() {
      busy = true;
      error = null;
    });
    try {
      // Native receives the reviewed immutable record and independently checks
      // catalog/source revisions and every binding/saved-panel reference.
      await store.delete(original);
      if (!sameSession()) return;
      // A successful Native acknowledgement is sufficient to remove this exact
      // record even if the subsequent refresh fails. Do not leave it selectable
      // or keep its observation authority alive while waiting for another read.
      final remaining = sources.where((value) => value.id != original.id).toList();
      acknowledgeObservationCatalog(remaining);
      widget.onSourcesLoaded?.call(remaining, store.readOnly);
      setState(() {
        sources = remaining;
        if (selected == original.id) selected = null;
      });
      // Reconcile authoritative records even if the user selected another source
      // while the mutation was pending; retain that selection, never restore the
      // deleted source or a cancelled read from it.
      await load(preserveSelection: true);
      if (sameSession() && source == null) setState(() => editing = true);
    } catch (e) {
      if (sameSession() && token == generation) setState(() => error = e);
    } finally {
      if (sameSession() && token == generation) setState(() => busy = false);
    }
  }

  Future<void> operate<T>(
    Future<T> Function(MonitoringSource) work,
    void Function(T) accept, {
    bool alerts = false,
    bool observationRead = false,
    void Function(Object failure)? onFailure,
  }) async {
    final s = source;
    if (s == null || busy || !widget.active) return;
    if (observationRead) cancelForegroundObservation();
    final token = ++generation, fingerprint = s.fingerprint;
    setState(() {
      busy = true;
      reading = true;
      readingAlerts = alerts;
      error = null;
    });
    try {
      final value = await work(s);
      if (mounted && token == generation && source?.fingerprint == fingerprint) {
        setState(() => accept(value));
      }
    } catch (e) {
      if (mounted && token == generation) {
        setState(() {
          error = e;
          onFailure?.call(e);
        });
      }
    } finally {
      if (mounted && token == generation) {
        setState(() {
          busy = false;
          reading = false;
          readingAlerts = false;
        });
      }
    }
  }

  Future<void> importSavedPanel() async {
    final open = widget.importGrafana;
    if (open == null ||
        widget.store is! MonitoringSavedPanelStore ||
        widget.store.readOnly ||
        !widget.active ||
        busy ||
        savedPanels.length >= 64) {
      return;
    }
    final request = ++importGeneration, operation = generation, catalog = catalogGeneration;
    final service = widget.service, store = widget.store;
    final catalogFingerprint = jsonEncode(sources.map((s) => [s.fingerprint, s.readOnly]).toList());
    final savedFingerprint = savedCatalogFingerprint;
    bool current() =>
        mounted &&
        widget.active &&
        request == importGeneration &&
        operation == generation &&
        catalog == catalogGeneration &&
        identical(service, widget.service) &&
        identical(store, widget.store);
    var checkingCatalog = false;
    try {
      final draft = await open(List.unmodifiable(sources));
      if (!current() || draft == null || busy || widget.store.readOnly || savedPanels.length >= 64) return;
      // Continue is explicit: re-read only the existing source catalog before
      // filling an editor. This is not a metric query or persistence operation.
      checkingCatalog = true;
      setState(() => busy = true);
      final reviewed = await store.load();
      if (!current()) return;
      widget.onSourcesLoaded?.call(reviewed, store.readOnly);
      if (store.readOnly ||
          savedPanels.length >= 64 ||
          savedFingerprint != savedCatalogFingerprint ||
          catalogFingerprint != jsonEncode(reviewed.map((s) => [s.fingerprint, s.readOnly]).toList())) {
        setState(() => error = const MonitoringFailure.local(MonitoringErrorCode.sourceChanged));
        return;
      }
      final target = reviewed.where(draft.matches).firstOrNull;
      if (target == null) {
        setState(() => error = const MonitoringFailure.local(MonitoringErrorCode.sourceChanged));
        return;
      }
      choose(target.id);
      setState(() => pendingImport = draft);
    } catch (e) {
      if (current()) setState(() => error = e);
    } finally {
      if (checkingCatalog && current()) setState(() => busy = false);
    }
  }

  Future<bool> mutateSaved(Future<void> Function(MonitoringSavedPanelStore) mutation) async {
    if (widget.store is! MonitoringSavedPanelStore || busy || !widget.active || widget.store.readOnly) return false;
    cancel();
    final token = ++generation;
    setState(() {
      busy = true;
      error = null;
      savedRead = null;
      result = null;
      acceptedQuery = MonitoringAcceptedQueryState.empty;
      alertResult = null;
    });
    try {
      await mutation(widget.store as MonitoringSavedPanelStore);
      if (!mounted || token != generation || !widget.active) return false;
      await load(preserveSelection: true);
      return mounted && widget.active && token == generation;
    } catch (e) {
      if (mounted && token == generation) setState(() => error = e);
      return false;
    } finally {
      if (mounted && token == generation) setState(() => busy = false);
    }
  }

  Future<void> previewPanel(SavedPanelDefinition draft) async {
    final target = source;
    if (target == null ||
        target.readOnly ||
        busy ||
        editing ||
        !widget.active ||
        target.id != draft.sourceId ||
        target.revision != draft.sourceRevision) {
      return;
    }
    try {
      final window = timeWindow.requested;
      final query = MonitoringQuery(
        expression: draft.query.trim(),
        start: window.startSeconds,
        end: window.endSeconds,
        stepSeconds: window.stepSeconds,
      );
      query.validate();
      final identity = MonitoringQueryIdentity(scope: widget.service, source: target, query: query);
      setState(() {
        savedRead = null;
        previewAccepted = MonitoringAcceptedQueryState.empty;
      });
      // An explicit draft preview reads metrics only. It never admits/saves a
      // panel, mutates the exploratory query, or enables background polling.
      await operate<QueryOutcome>((s) => widget.service.query(s, query), (value) {
        savedRead = SavedPanelRead(value, draft.unitHint);
        previewAccepted = MonitoringAcceptedQueryState.empty
            .begin(identity, generation)
            .complete(generation, value, DateTime.now().toUtc());
        previewExpression = query.expression;
        savedTitle = draft.title.isEmpty ? l.monitoringSavedPanelPreview : draft.title;
        savedWindow = window;
      }, observationRead: true);
    } catch (e) {
      if (mounted && widget.active) {
        setState(
          () => error = e is MonitoringTimeWindowFailure
              ? const MonitoringFailure.local(MonitoringErrorCode.invalidQuery)
              : e,
        );
      }
    }
  }

  Future<void> runSaved(SavedMonitoringPanel panel, bool useRange) async {
    final reader = widget.service;
    if (reader is! MonitoringSavedPanelReader ||
        busy ||
        editing ||
        !widget.active ||
        source == null ||
        !panel.currentFor(source!)) {
      return;
    }
    try {
      final window = useRange ? timeWindow.requested : null;
      final requested = window == null
          ? null
          : SavedPanelRange(start: window.startSeconds, end: window.endSeconds, stepSeconds: window.stepSeconds);
      setState(() => savedRead = null);
      ValueChanged<MonitoringObservationValue>? acceptObservation;
      await operate<SavedPanelRead>(
        (s) async {
          acceptObservation = beginObservation(
            MonitoringSavedPanelTarget(panel: panel, windowPolicy: window == null ? null : observationWindow(window)),
          );
          final value = await (reader as MonitoringSavedPanelReader).readSavedPanel(s, panel, window: requested);
          if (!savedPanels.any((p) => p.id == panel.id && p.revision == panel.revision && p.currentFor(s))) {
            throw const MonitoringFailure.local(MonitoringErrorCode.sourceChanged);
          }
          return value;
        },
        (value) {
          savedRead = value;
          previewExpression = null;
          savedTitle = panel.definition.title;
          savedWindow = window;
          acceptObservation?.call(MonitoringObservedSavedPanel(value, window));
        },
        observationRead: true,
      );
    } catch (e) {
      if (mounted && widget.active) {
        setState(
          () => error = e is MonitoringTimeWindowFailure
              ? const MonitoringFailure.local(MonitoringErrorCode.invalidQuery)
              : e,
        );
      }
    }
  }

  Future<void> runAlerts(MonitoringAlertKind kind) async {
    final reader = widget.service;
    if (reader is! MonitoringAlertReader || busy || !widget.active || editing) return;
    final capability = reader as MonitoringAlertReader;
    setState(() => alertResult = null);
    await operate<MonitoringAlertSnapshot>(
      (source) => capability.readAlerts(source, kind),
      (value) => alertResult = value,
      alerts: true,
    );
  }

  Future<void> runQuery() async {
    try {
      final requestedWindow = range ? timeWindow.requested : null;
      final query = MonitoringQuery(
        expression: expression.text.trim(),
        start: requestedWindow?.startSeconds,
        end: requestedWindow?.endSeconds,
        stepSeconds: requestedWindow?.stepSeconds ?? 15,
      );
      query.validate();
      final selectedSource = source;
      if (selectedSource == null || busy || !widget.active || editing) return;
      final selectedRouteId = selectedAgentRouteId;
      final requestScope = widget.panelSyncScope;
      final routeSnapshot = selectedAgentRoute;
      final reader = widget.service;
      final identity = MonitoringQueryIdentity(scope: reader, source: selectedSource, query: query);
      late int readToken;
      ValueChanged<MonitoringObservationValue>? acceptObservation;
      await operate<QueryOutcome>(
        (s) {
          readToken = generation;
          acceptObservation = selectedRouteId != null ? null : beginObservation(
            requestedWindow == null
                ? MonitoringCustomInstantTarget(query.expression)
                : MonitoringCustomRangeTarget(
                    expression: query.expression,
                    windowPolicy: observationWindow(requestedWindow),
                  ),
          );
          setState(() => acceptedQuery = acceptedQuery.begin(identity, readToken));
          if (selectedRouteId != null) {
            if (reader is! MonitoringAgentRouteReader || routeSnapshot == null ||
                selectedAgentRoute?.metadataVersion != routeSnapshot.metadataVersion ||
                !agentRouteMatchesSource(routeSnapshot, s)) {
              throw const MonitoringFailure.local(MonitoringErrorCode.sourceChanged);
            }
            return (reader as MonitoringAgentRouteReader).queryViaAgentRoute(s, query, routeId: selectedRouteId);
          }
          return reader.query(s, query);
        },
        (value) {
          if (selectedRouteId != null && (!widget.active || !identical(widget.service, reader) ||
              requestScope == null || !requestScope.same(widget.panelSyncScope) ||
              widget.panelSyncCurrent?.call() != true)) {
            acceptedQuery = MonitoringAcceptedQueryState.empty;
            result = null;
            resultWindow = null;
            return;
          }
          final completed = DateTime.now().toUtc();
          acceptedQuery = acceptedQuery.complete(readToken, value, completed);
          result = value;
          resultWindow = requestedWindow;
          acceptObservation?.call(MonitoringObservedQuery(value, requestedWindow));
          history.insert(0, _History(source!.id, source!.name, query, completed));
          if (history.length > 40) history.removeLast();
        },
        observationRead: true,
        onFailure: (failure) => acceptedQuery = acceptedQuery.fail(readToken, failure),
      );
    } catch (e) {
      if (mounted) {
        setState(
          () => error = e is MonitoringTimeWindowFailure
              ? const MonitoringFailure.local(MonitoringErrorCode.invalidQuery)
              : e,
        );
      }
    }
  }

  Future<void> probeSource() async {
    if (busy || !widget.active) return;
    setState(() => capabilities = null);
    await operate(widget.service.probe, (v) => capabilities = v);
  }

  Future<void> clearMetadataCache() async {
    final originalService = widget.service, originalStore = widget.store, selectedSource = source;
    if (originalService is! MonitoringMetadataCache || selectedSource == null ||
        selectedSource.readOnly || busy || editing || !widget.active) {
      return;
    }
    final cache = originalService as MonitoringMetadataCache;
    final token = ++generation, catalog = catalogGeneration;
    final scope = widget.panelSyncScope, live = widget.panelSyncCurrent;
    final route = ModalRoute.of(context);
    bool current() => mounted && widget.active && !editing && token == generation &&
        catalog == catalogGeneration && identical(widget.service, originalService) &&
        identical(widget.store, originalStore) && source?.fingerprint == selectedSource.fingerprint &&
        (scope?.same(widget.panelSyncScope) ?? widget.panelSyncScope == null) &&
        (live == null || live()) && identical(ModalRoute.of(context), route) &&
        (route == null || route.isCurrent);
    if (!current()) return;
    setState(() { busy = true; error = null; metadataCacheClearedGeneration = null; });
    try {
      await cache.clearCache(selectedSource);
      if (!current()) return;
      setState(() {
        metadata = null;
        metadataLabel = null;
        metadataFilter.clear();
        metadataCacheClearedGeneration = token;
      });
    } catch (failure) {
      if (current()) setState(() => error = failure);
    } finally {
      if (mounted && token == generation) setState(() => busy = false);
    }
  }

  Future<void> loadMetadata() async {
    if (busy || !widget.active) return;
    final requestedLabel = label.text.trim();
    setState(() { metadata = null; metadataCacheClearedGeneration = null; });
    await operate((s) => widget.service.labelValues(s, requestedLabel), (v) {
      metadata = v;
      metadataLabel = requestedLabel;
    });
  }

  Widget field(String title, TextEditingController controller, {String? keyName, int maxLines = 1}) => Padding(
    padding: const EdgeInsets.only(bottom: 12),
    child: TextField(
      key: keyName == null ? null : ValueKey(keyName),
      controller: controller,
      enabled: !busy,
      maxLines: maxLines,
      autocorrect: false,
      decoration: InputDecoration(labelText: title),
    ),
  );
  Widget sourcePane() => ContentSurface(
    child: Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(l.monitoringSources, style: Theme.of(context).textTheme.titleMedium),
        const SizedBox(height: 12),
        if (sources.isNotEmpty)
          GlassSelect<String?>(
            key: const ValueKey('monitoring-source-select'),
            value: selected,
            expand: true,
            items: [for (final s in sources) GlassSelectItem(value: s.id, label: s.name)],
            onChanged: (id) {
              if (id != null) choose(id);
            },
          ),
        Wrap(
          spacing: 8,
          children: [
            TextButton.icon(
              key: const ValueKey('monitoring-new-source'),
              onPressed: widget.store.readOnly ? null : () => edit(create: true),
              icon: const Icon(Icons.add),
              label: Text(l.monitoringAddSource),
            ),
            if (source != null)
              TextButton(
                onPressed: widget.store.readOnly || source!.readOnly ? null : () => edit(),
                child: Text(l.monitoringEditSource),
              ),
          ],
        ),
        if (source != null && (!editing || editId == source!.id) && !widget.store.readOnly && !source!.readOnly)
          Align(
            alignment: Alignment.centerLeft,
            child: GlassButton(
              key: const ValueKey('monitoring-delete-source'),
              label: l.monitoringDeleteSource,
              icon: Icons.delete_outline,
              style: GlassButtonStyle.destructiveQuiet,
              size: GlassControlSize.sm,
              onPressed: busy || sourceHasReferences ? null : reviewDeleteSource,
            ),
          ),
        if (sourceHasBindings) Text(l.monitoringBindingSourceInUse),
        if (sourceHasSavedPanels) Text(l.monitoringSavedPanelSourceInUse),
        if (widget.store.readOnly) Text(l.monitoringReadOnly),
        if (editing) ...[
          field(l.monitoringName, name, keyName: 'monitoring-source-name'),
          field(l.monitoringEndpoint, endpoint, keyName: 'monitoring-source-endpoint'),
          Text(l.monitoringEndpointHelp),
          const SizedBox(height: 12),
          GlassSelect<SourceRoute>(
            key: const ValueKey('monitoring-source-route'),
            value: route,
            expand: true,
            items: [
              GlassSelectItem(value: SourceRoute.direct, label: l.monitoringDirect),
              if (widget.chooseSshRouteHost != null || route == SourceRoute.sshHost)
                GlassSelectItem(value: SourceRoute.sshHost, label: l.monitoringSshRoute),
              if (sourceAssociationPort != null || route == SourceRoute.agentDestination)
                GlassSelectItem(value: SourceRoute.agentDestination, label: l.hostAuthModeAgent),
              if (route == SourceRoute.sshTunnel || route == SourceRoute.agentTunnel)
                GlassSelectItem(value: route, label: l.monitoringSavedTunnel),
            ],
            onChanged: busy ? null : changeSourceRoute,
          ),
          const SizedBox(height: 12),
          if (route == SourceRoute.sshHost) ...[
            Text(l.monitoringSshRouteHint),
            const SizedBox(height: 8),
            GlassButton(key: const ValueKey('monitoring-source-ssh-host'),
              icon: Icons.dns_outlined, label: widget.sshRouteHosts.where((host) => host.id == routeHostId).firstOrNull?.name ??
                (routeHostId == null ? l.monitoringSshRouteChoose : l.monitoringSshRouteMissing),
              onPressed: busy || widget.chooseSshRouteHost == null ? null : chooseSourceSshHost),
            const SizedBox(height: 12),
          ],
          if (route == SourceRoute.agentDestination) ...[
            OutlinedButton(key: const ValueKey('monitoring-source-agent-status'),
              onPressed: busy || !widget.active || sourceAssociationPort==null ? null : loadSourceAgentAssociations,
              child: Text(l.agentAssociationReadStatus)),
            GlassSelect<String?>(
              key: const ValueKey('monitoring-source-agent-destination'),
              value: sourceAgentAssociations.any((row)=>row.id==routeAssociationId) ? routeAssociationId : null,
              expand: true,
              items: [
                GlassSelectItem(value:null,label:l.agentAssociationChooseHost),
                for (final row in sourceAgentAssociations)
                  GlassSelectItem(value:row.id,label:sourceAgentHostName(row.hostId)),
              ],
              onChanged:busy ? null : chooseSourceAgentDestination,
            ),
            if (sourceAgentAssociations.isEmpty) ...[
              Text(l.agentAssociationDisclosure),
              if (widget.openAgentAssociation!=null) TextButton(
                key:const ValueKey('monitoring-source-agent-setup'),onPressed:busy||!widget.active?null:openSourceAgentSetup,
                child:Text(l.agentAssociationOpen)),
            ],
            Text(l.agentTunnelDestinationIp),
            GlassSelect<String>(value:routeDestinationIp??'127.0.0.1',expand:true,
              items:[for(final ip in const ['127.0.0.1','::1']) GlassSelectItem(value:ip,label:ip)],
              onChanged:busy?null:(value)=>setState(()=>routeDestinationIp=value)),
            field(l.agentTunnelDestinationPort,routeDestinationPortInput,keyName:'monitoring-source-agent-port'),
            if (routeAssociationId!=null)
              Text('${sourceAgentHostName(routeHostId)} · ${routeDestinationIp=='::1'?'[::1]':routeDestinationIp}:${routeDestinationPortInput.text}',
                key:const ValueKey('monitoring-source-agent-reference')),
            Text(l.monitoringAgentDestinationHint),
            const SizedBox(height: 12),
          ],
          if (route == SourceRoute.sshTunnel || route == SourceRoute.agentTunnel) ...[
            Text(l.monitoringTunnelUnavailable),
            if (routeRef.text.isNotEmpty) SelectableText(routeRef.text),
            const SizedBox(height: 12),
          ],
          GlassSelect<SourceAuth>(
            value: auth,
            expand: true,
            items: [
              for (final a in SourceAuth.values)
                GlassSelectItem(
                  value: a,
                  label: switch (a) {
                    SourceAuth.none => l.monitoringNoAuth,
                    SourceAuth.bearerRef => l.monitoringBearerAuth,
                    SourceAuth.basicRef => l.monitoringBasicAuth,
                  },
                ),
            ],
            onChanged: busy
                ? null
                : (a) => setState(() {
                    auth = a;
                    if (!eligibleCredentials.any((c) => c.id == selectedCredential)) selectedCredential = null;
                  }),
          ),
          const SizedBox(height: 12),
          if (auth != SourceAuth.none) ...[
            if (widget.credentialsLoading) const LinearProgressIndicator(),
            if (widget.credentialsError != null) Text(widget.credentialsError!),
            GlassSelect<String?>(
              key: const ValueKey('monitoring-credential-ref'),
              value: selectedCredential,
              expand: true,
              semanticLabel: l.monitoringCredential,
              items: [
                GlassSelectItem(value: null, label: l.monitoringChooseCredential),
                if (selectedCredential != null && !eligibleCredentials.any((c) => c.id == selectedCredential))
                  GlassSelectItem(value: selectedCredential, label: l.monitoringMissingCredential),
                for (final c in eligibleCredentials)
                  GlassSelectItem(value: c.id, label: c.username == null ? c.name : '${c.name} · ${c.username}'),
              ],
              onChanged: busy ? null : (id) => setState(() => selectedCredential = id),
            ),
            const SizedBox(height: 8),
            if (widget.createCredential != null)
              Align(
                alignment: AlignmentDirectional.centerStart,
                child: GlassButton(
                  key: const ValueKey('monitoring-add-credential'),
                  label: l.monitoringAddCredential,
                  onPressed: busy ? null : addCredential,
                ),
              ),
            if (auth == SourceAuth.basicRef) Text(l.monitoringBasicUsernameHelp),
          ],
          if (auth == SourceAuth.basicRef && Uri.tryParse(endpoint.text.trim())?.scheme == 'http')
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 8),
              child: Text(l.monitoringHttpBasicPlaintext, key: const ValueKey('monitoring-http-basic-notice')),
            ),
          Text(l.monitoringCredentialHelp),
          const SizedBox(height: 8),
          InfoBanner(
            key: const ValueKey('monitoring-source-access-policy'),
            tone: BannerTone.warning,
            message: l.monitoringSourceAccessPolicy,
          ),
          const SizedBox(height: 8),
          Text(l.monitoringMetricsPrivacy),
          const SizedBox(height: 12),
          ExpansionTile(title: Text(l.monitoringCustomCa), children: [field(l.monitoringCaPem, ca, maxLines: 5)]),
          const SizedBox(height: 12),
          FilledButton(
            key: const ValueKey('monitoring-save-source'),
            onPressed: busy || widget.store.readOnly ? null : saveSource,
            child: Text(l.monitoringSaveSource),
          ),
        ] else if (source case final s?) ...[
          SelectableText(s.endpoint),
          const SizedBox(height: 8),
          Text(
            '${s.route == SourceRoute.direct ? l.monitoringDirect : s.route == SourceRoute.sshHost ? l.monitoringSshRoute : s.route == SourceRoute.agentDestination ? l.hostAuthModeAgent : l.monitoringSavedTunnel} · ${authLabel(s.auth)}',
          ),
          if (s.route == SourceRoute.agentDestination) Text(l.monitoringAgentDestinationHint),
          if (s.route == SourceRoute.sshHost)
            Text(widget.sshRouteHosts.where((host) => host.id == s.hostId).firstOrNull?.name ?? l.monitoringSshRouteMissing,
              key: const ValueKey('monitoring-source-ssh-host-name')),
          const SizedBox(height: 12),
          FilledButton(
            key: const ValueKey('monitoring-probe'),
            onPressed: busy ? null : probeSource,
            child: Text(l.monitoringProbe),
          ),
          const SizedBox(height: 12),
          Text(
            capabilities == null
                ? l.monitoringNotChecked
                : capabilities!.compatible
                ? l.monitoringSupported
                : l.monitoringUnsupported,
            key: const ValueKey('monitoring-capability-summary'),
          ),
          if (capabilities case final c?) ...[
            capability('/api/v1/query', c.query),
            capability('/api/v1/query_range', c.queryRange),
          ],
        ],
      ],
    ),
  );
  bool get sourceHasBindings =>
      widget.store is MonitoringBindingStore &&
      (widget.store as MonitoringBindingStore).bindings.any((b) => b.sourceId == source?.id);
  bool get sourceHasSavedPanels => savedPanels.any((panel) => panel.definition.sourceId == source?.id);
  bool get sourceHasReferences => sourceHasBindings || sourceHasSavedPanels;
  Widget capability(String endpoint, EndpointCapability value) => Padding(
    padding: const EdgeInsets.only(top: 12),
    child: Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          l.monitoringEndpointStatus(
            endpoint,
            value.supported ? l.monitoringSupportedEndpoint : l.monitoringUnsupportedEndpoint,
            value.httpStatus?.toString() ?? '—',
          ),
        ),
        if (value.error != null)
          Text(
            monitoringFailureText(context, value.error!),
            style: TextStyle(color: Theme.of(context).colorScheme.error),
          ),
        for (final warning in value.warnings) Text(l.monitoringWarning(warning)),
      ],
    ),
  );
  Widget queryPane() => Column(
    crossAxisAlignment: CrossAxisAlignment.stretch,
    children: [
      ContentSurface(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(l.monitoringExplore, style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 12),
            Text(l.monitoringSelectorHelp),
            const SizedBox(height: 8),
            field(l.monitoringExpression, expression, keyName: 'monitoring-expression', maxLines: 3),
            CheckboxListTile(
              contentPadding: EdgeInsets.zero,
              key: const ValueKey('monitoring-range'),
              value: range,
              title: Text(l.monitoringRange),
              onChanged: busy
                  ? null
                  : (v) {
                      cancel();
                      setState(() {
                        range = v ?? false;
                        result = null;
                        acceptedQuery = MonitoringAcceptedQueryState.empty;
                        alertResult = null;
                        savedRead = null;
                        resultWindow = null;
                      });
                    },
            ),
            if (range) MonitoringTimeWindowControls(controller: timeWindow, enabled: widget.active),
            const SizedBox(height: 12),
            if (widget.service is MonitoringAgentRouteReader && agentRouteCache != null && source != null) ...[
              Text(l.monitoringAgentRoute),
              GlassSelect<int?>(
                key: const ValueKey('monitoring-agent-route'), value: selectedAgentRouteVersion,
                expand: true,
                items: [
                  GlassSelectItem(value: null, label: l.monitoringAgentRouteSavedSource),
                  if (selectedAgentRouteVersion != null && selectedAgentRoute == null)
                    GlassSelectItem(value: selectedAgentRouteVersion, label: l.monitoringAgentRouteUnavailable),
                  for (final row in retainedAgentRoutes)
                    if (row.metadataVersion != selectedAgentRouteVersion)
                      GlassSelectItem(value: row.metadataVersion, label: '${sourceAgentHostName(row.hostId)} · ${row.destination}'),
                  if (selectedAgentRoute case final row?)
                    GlassSelectItem(value: row.metadataVersion, label: '${sourceAgentHostName(row.hostId)} · ${row.destination}'),
                ],
                onChanged: busy || editing ? null : (version) {
                  cancel();
                  setState(() {
                    if (version == null) {
                      clearAgentRouteSelection();
                    } else {
                      final row = retainedAgentRoutes.where((row) => row.metadataVersion == version).firstOrNull;
                      if (row != null) {
                        selectedAgentRouteVersion = row.metadataVersion;
                        selectedAgentRouteId = row.routeId;
                        agentRouteSourceFingerprint = source?.fingerprint;
                      }
                    }
                  });
                },
              ),
              if (selectedAgentRouteId != null) ...[
                Text(l.monitoringAgentRouteHint),
                if (selectedAgentRoute case final row?) ...[
                  Text('${l.monitoringAgentRouteExpires}: ${row.expiresUtc.toIso8601String()}'),
                  if (!agentRouteMatchesSource(row, source!)) Text(l.monitoringAgentRouteEndpointMismatch),
                ] else Text(l.monitoringAgentRouteUnavailable),
              ],
              const SizedBox(height: 12),
            ],
            Text(l.monitoringQueryLimits),
            const SizedBox(height: 12),
            FilledButton(
              key: const ValueKey('monitoring-query'),
              onPressed: source == null || busy || editing || !widget.active || (range && timeWindow.dirty)
                  ? null
                  : runQuery,
              child: Text(range ? l.monitoringQueryRange : l.monitoringQueryCurrent),
            ),
            if (widget.observation == null && widget.pinControls != null)
              ValueListenableBuilder<TextEditingValue>(
                valueListenable: expression,
                builder: (_, value, _) => widget.pinControls!(
                  source,
                  value.text,
                  !range,
                  widget.active && !editing && !busy && !widget.store.readOnly && source?.allowsBackgroundObservation==true,
                ),
              ),
            if (busy) ...[
              const SizedBox(height: 12),
              const LinearProgressIndicator(),
              TextButton(key: const ValueKey('monitoring-cancel'), onPressed: cancel, child: Text(l.monitoringCancel)),
            ],
          ],
        ),
      ),
      const SizedBox(height: 16),
      if (widget.observation == null) ?widget.pinnedObservation,
      if (widget.reviewMetricsAi != null)
        Align(
          alignment: Alignment.centerLeft,
          child: GlassButton(
            key: const ValueKey('monitoring-review-ai'),
            label: l.metricsAiTitle,
            onPressed: source == null || busy || editing || aiReviewing || !widget.active ? null : reviewMetricsAi,
          ),
        ),
      if (acceptedQuery.identity != null)
        MonitoringAcceptedQueryView(
          state: acceptedQuery,
          requestedWindow: resultWindow,
          timeWindow: timeWindow,
          active: widget.active,
        )
      else if (result case final r?)
        MonitoringResultView(
          key: ObjectKey(r),
          result: r,
          requestedWindow: resultWindow,
          timeWindow: timeWindow,
          active: widget.active,
        ),
      const SizedBox(height: 16),
      ContentSurface(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(l.monitoringMetadata, style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 12),
            field(l.monitoringLabel, label, keyName: 'monitoring-label'),
            TextButton(
              key: const ValueKey('monitoring-label-values'),
              onPressed: source == null || busy || editing ? null : loadMetadata,
              child: Text(l.monitoringLoadValues),
            ),
            Text(l.monitoringMetadataLimits),
            if (widget.service case final MonitoringMetadataCache cache) ...[
              Text(l.monitoringMetadataCacheTtl(cache.labelCacheTtl.inSeconds.toString()),
                key: const ValueKey('monitoring-metadata-cache-ttl')),
              TextButton(
                key: const ValueKey('monitoring-clear-metadata-cache'),
                onPressed: source == null || source!.readOnly || busy || editing || !widget.active ? null : clearMetadataCache,
                child: Text(l.monitoringClearMetadataCache),
              ),
              if (metadataCacheClearedGeneration == generation)
                InfoBanner(
                  key: const ValueKey('monitoring-metadata-cache-cleared'),
                  tone: BannerTone.info,
                  message: l.monitoringMetadataCacheCleared,
                ),
            ],
            if (metadata case final values?) ...[
              if (values.error != null) Text(monitoringFailureText(context, values.error!)),
              if (values.partial) Text(l.monitoringPartialMetadata),
              for (final warning in values.warnings) Text(l.monitoringWarning(warning)),
              for (final info in values.infos) Text(l.monitoringInfo(info)),
              Text(l.monitoringValueCount(values.values.length.toString(), values.cached ? l.monitoringCached : '')),
              field(l.monitoringFilterValues, metadataFilter, keyName: 'monitoring-metadata-filter'),
              SizedBox(
                height: 160,
                child: ListView(
                  children: [
                    for (final value in values.values.where((v) => v.contains(metadataFilter.text)))
                      ListTile(
                        dense: true,
                        title: Text(value),
                        onTap: () => setState(
                          () => expression.text = metadataLabel == '__name__'
                              ? value
                              : '{${metadataLabel!}=${jsonEncode(value)}}',
                        ),
                      ),
                  ],
                ),
              ),
            ],
          ],
        ),
      ),
      const SizedBox(height: 16),
      if (history.isNotEmpty)
        ContentSurface(
          key: const ValueKey('monitoring-history'),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(l.monitoringHistory, style: Theme.of(context).textTheme.titleMedium),
              Text(l.monitoringHistoryHint),
              for (final h in history)
                TextButton(
                  onPressed: () {
                    if (!sources.any((s) => s.id == h.sourceId)) return;
                    choose(h.sourceId);
                    setState(() {
                      expression.text = h.query.expression;
                      range = h.query.range;
                      if (h.query.range) {
                        timeWindow.restore(
                          MonitoringTimeWindow.fromEpoch(h.query.start!, h.query.end!, h.query.stepSeconds),
                        );
                      }
                    });
                  },
                  child: Align(
                    alignment: Alignment.centerLeft,
                    child: Text(
                      l.monitoringHistoryEntry(
                        h.time.toLocal().toString(),
                        h.sourceName,
                        h.query.range ? l.monitoringHistorical : l.monitoringCurrent,
                        h.query.expression,
                      ),
                    ),
                  ),
                ),
            ],
          ),
        ),
    ],
  );
  Widget explorationPane() => Column(
    crossAxisAlignment: CrossAxisAlignment.stretch,
    children: [
      if (widget.store case final MonitoringBindingStore bindings) ...[
        MonitoringBindings(
          key: ValueKey('bindings-${source?.id}-${source?.revision}'),
          store: bindings,
          sourceStore: widget.store,
          correlationScope: widget.correlationScope,
          correlationCurrent: widget.correlationCurrent,
          correlationAutomation: widget.correlationAutomation,
          acceptedPanelForCorrelation: acceptedPanelForCorrelation,
          onTimelineChanged: timelineChanged,
          onContextNavigation: widget.onContextNavigation,
          requestedContext: widget.requestedContext,
          onContextDismissed: widget.onContextDismissed,
          source: source,
          sourceIds: sources.map((s) => s.id).toSet(),
          hosts: widget.hosts,
          chooseHost: widget.chooseHost,
          reader: widget.service is MonitoringBindingReader ? widget.service as MonitoringBindingReader : null,
          timeWindow: timeWindow,
          onBeforeRead: cancelForegroundObservation,
          onBeginPanelRead: beginBindingObservation,
          isPanelTransferred: (binding, panel) =>
              transferredBinding == (bindingId: binding.id, revision: binding.revision, panel: panel),
          active: widget.active,
          onChanged: () async {
            cancel();
            setState(() {
              result = null;
              acceptedQuery = MonitoringAcceptedQueryState.empty;
              alertResult = null;
              savedRead = null;
              metadata = null;
              capabilities = null;
            });
            await load(preserveSelection: true);
          },
        ),
        const SizedBox(height: 16),
      ],
        if (widget.openAgentAssociation != null) Align(alignment: Alignment.centerLeft,
          child: OutlinedButton(key: const ValueKey('monitoring-agent-association-open'),
            onPressed: !widget.active || loading || busy || editing ? null : () {
              final service = widget.service, store = widget.store;
              bool current() => mounted && widget.active && identical(widget.service, service) && identical(widget.store, store);
              widget.openAgentAssociation!(current);
            }, child: Text(l.agentAssociationOpen))),
        if (widget.openAgentReader != null) Align(alignment: Alignment.centerLeft,
          child: OutlinedButton(key: const ValueKey('monitoring-agent-reader-open'),
            onPressed: !widget.active || loading || busy || editing ? null : () {
              final service = widget.service, store = widget.store, originalSources = List<MonitoringSource>.of(sources);
              final originalBindings = store is MonitoringBindingStore ? List<MonitoringBinding>.of((store as MonitoringBindingStore).bindings) : <MonitoringBinding>[];
              bool current() => mounted && widget.active && identical(widget.service,service) && identical(widget.store,store) &&
                originalSources.length == sources.length && originalSources.every((s)=>sources.any((v)=>identical(s,v))) &&
                store is MonitoringBindingStore && originalBindings.length == (store as MonitoringBindingStore).bindings.length &&
                originalBindings.every((b)=>(store as MonitoringBindingStore).bindings.any((v)=>identical(b,v)));
              widget.openAgentReader!(List.unmodifiable(originalSources),List.unmodifiable(originalBindings),current);
            },child: Text(l.agentReaderOpen))),
      if (widget.store is MonitoringSavedPanelStore) ...[
        MonitoringSavedPanels(
          key: ValueKey('saved-panels-${source?.id}-${source?.revision}'),
          panels: savedPanels,
          source: source,
          syncSnapshot: panelSyncSnapshot,
          onReviewSyncConsent: widget.active&&widget.panelSyncScope!=null&&widget.panelSyncCurrent?.call()==true&&
            panelSyncSnapshot?.storage!=PanelSyncStorage.local&&syncConsentPort!=null?reviewPanelPolicy:null,
          canSave: panelSyncSnapshot?.storage != PanelSyncStorage.synced,
          syncChecking: syncChecking,
          syncFailed: syncFailed,
          onCheckSync: widget.active && widget.panelSyncScope != null && widget.panelSyncCurrent?.call() == true &&
            widget.store is MonitoringPanelSyncDiagnostics && (widget.store as MonitoringPanelSyncDiagnostics).canCheckPanelSync
            ? checkPanelSync : null,
          sourceNames: {for (final source in sources) source.id: source.name},
          canRead: widget.service is MonitoringSavedPanelReader,
          enabled: widget.active && (!editing || sources.isEmpty) && !widget.store.readOnly,
          busy: busy,
          onSave: (definition, previous) => mutateSaved((store) async {
            await store.savePanel(definition, previous: previous);
          }),
          onDelete: (panel) => mutateSaved((store) => store.deletePanel(panel)),
          onRead: runSaved,
          onPreview: previewPanel,
          previewControls: ExpansionTile(
            key: const ValueKey('saved-panel-period'),
            tilePadding: EdgeInsets.zero,
            title: Text(l.monitoringTimeWindow),
            subtitle: Text(
              timeWindow.dirty ? l.monitoringTimeUnapplied : monitoringTimePresetLabel(l, timeWindow.preset),
            ),
            children: [
              MonitoringTimeWindowControls(
                controller: timeWindow,
                enabled: widget.active && !busy,
                keyPrefix: 'saved-panel-time',
              ),
            ],
          ),
          previewEnabled: !timeWindow.dirty,
          onImport: widget.importGrafana == null ? null : importSavedPanel,
          onOpenGrafana: widget.active ? widget.openGrafana : null,
          prefill: pendingImport,
          onPrefillHandled: (draft) {
            if (mounted && identical(pendingImport, draft)) setState(() => pendingImport = null);
          },
        ),
        if (savedRead case final value?) ...[
          Text(savedTitle ?? l.monitoringSavedPanels, style: Theme.of(context).textTheme.titleMedium),
          if (previewExpression case final query?) ...[
            SelectableText('${l.monitoringExpression}: $query', key: const ValueKey('saved-panel-preview-expression')),
            if (widget.reviewMetricsAi != null && previewAccepted.accepted != null)
              Align(
                alignment: Alignment.centerLeft,
                child: GlassButton(
                  key: const ValueKey('saved-panel-review-ai'),
                  label: l.metricsAiTitle,
                  onPressed: busy || aiReviewing || !widget.active ? null : () => reviewMetricsAi(previewOnly: true),
                ),
              ),
          ],
          Text('${l.monitoringSavedPanelHint}: ${savedPanelUnitLabel(l, value.unitHint)}'),
          MonitoringResultView(
            key: ObjectKey(value),
            result: value.outcome,
            requestedWindow: savedWindow,
            timeWindow: timeWindow,
            active: widget.active,
          ),
        ],
        const SizedBox(height: 16),
      ],
      if (widget.service is MonitoringAlertReader) ...[
        MonitoringAlertsView(
          key: ValueKey('alerts-${source?.id}-${source?.revision}'),
          result: alertResult,
          enabled: widget.active && source != null && !editing && !source!.readOnly,
          busy: busy,
          reading: readingAlerts,
          onRead: runAlerts,
          onCancel: cancel,
        ),
        const SizedBox(height: 16),
      ],
      queryPane(),
    ],
  );
  @override
  Widget build(BuildContext context) => LayoutBuilder(
    builder: (context, constraints) => SingleChildScrollView(
      key: const ValueKey('monitoring-workspace-scroll'),
      padding: const EdgeInsets.all(20),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(l.monitoringTitle, style: Theme.of(context).textTheme.headlineSmall),
          const SizedBox(height: 8),
          Text(l.monitoringIntro),
          const SizedBox(height: 16),
          if (loading) const LinearProgressIndicator(),
          if (error != null && !identical(error, acceptedQuery.error))
            Padding(
              padding: const EdgeInsets.only(bottom: 16),
              child: SelectableText(
                savedPanelAdmissionMessage(l, error!) ??
                    (error is MonitoringFailure
                        ? monitoringFailureText(context, error as MonitoringFailure)
                        : errorMessage(l, error!)),
                key: const ValueKey('monitoring-error'),
                style: TextStyle(color: Theme.of(context).colorScheme.error),
              ),
            ),
          if (widget.observation != null) ...[
            observationControls(),
            const SizedBox(height: 16),
            ?(widget.pinnedObservationBuilder?.call(currentChartTimeline) ?? widget.pinnedObservation),
            const SizedBox(height: 16),
          ],
          if (constraints.maxWidth >= 880)
            Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                SizedBox(width: 320, child: sourcePane()),
                const SizedBox(width: 16),
                Expanded(child: explorationPane()),
              ],
            )
          else ...[
            sourcePane(),
            const SizedBox(height: 16),
            explorationPane(),
          ],
        ],
      ),
    ),
  );
}

class MonitoringCredentialRef {
  const MonitoringCredentialRef({required this.id, required this.name, this.username});
  final String id, name;
  final String? username;
}
