import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/ai/device_local_ai_service.dart';

import 'package:consolecrypt/app/critical_notice_owner.dart';
import 'package:consolecrypt/app/critical_notice_providers.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/ai_service.dart' show deviceLocalAiProviderId;
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/credentials/credential_dialogs.dart';
import 'package:consolecrypt/hosts/host_picker.dart';
import 'package:consolecrypt/monitoring/agent_association_entry.dart';
import 'package:consolecrypt/monitoring/agent_association_port.dart';
import 'package:consolecrypt/monitoring/agent_reader_entry.dart';
import 'package:consolecrypt/monitoring/agent_reader_port.dart';
import 'package:consolecrypt/monitoring/agent_setup_entry.dart';
import 'package:consolecrypt/monitoring/agent_setup_models.dart';
import 'package:consolecrypt/monitoring/agent_setup_port.dart';
import 'package:consolecrypt/monitoring/job_context_focus.dart';
import 'package:consolecrypt/monitoring/job_context_navigation.dart';
import 'package:consolecrypt/monitoring/job_context_project_guard.dart';
import 'package:consolecrypt/monitoring/job_context_receiver.dart';
import 'package:consolecrypt/monitoring/job_context_section_adapter.dart';
import 'package:consolecrypt/monitoring/metrics_ai_context.dart';
import 'package:consolecrypt/monitoring/metrics_ai_leased_transport.dart';
import 'package:consolecrypt/monitoring/metrics_ai_native_transport.dart';
import 'package:consolecrypt/monitoring/metrics_ai_review_dialog.dart';
import 'package:consolecrypt/monitoring/metrics_ai_workspace_input.dart';
import 'package:consolecrypt/monitoring/monitoring_agent_overview.dart';
import 'package:consolecrypt/monitoring/monitoring_dashboard.dart';
import 'package:consolecrypt/monitoring/monitoring_grafana_browser.dart';
import 'package:consolecrypt/monitoring/monitoring_grafana_import.dart';
import 'package:consolecrypt/monitoring/monitoring_grafana_input.dart';
import 'package:consolecrypt/monitoring/monitoring_grafana_open.dart';
import 'package:consolecrypt/monitoring/monitoring_grafana_prefill.dart';
import 'package:consolecrypt/monitoring/monitoring_grafana_url.dart';
import 'package:consolecrypt/monitoring/monitoring_grafana_worker.dart';
import 'package:consolecrypt/monitoring/monitoring_host_focus.dart';
import 'package:consolecrypt/monitoring/monitoring_host_link_dialog.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_pinned_providers.dart';
import 'package:consolecrypt/monitoring/monitoring_pinned_view.dart';
import 'package:consolecrypt/monitoring/monitoring_providers.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';
import 'package:consolecrypt/monitoring/monitoring_workspace.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

final grafanaDashboardInspectorFactoryProvider = Provider<GrafanaDashboardInspector Function()>(
  (ref) => IsolatedGrafanaDashboardInspector.new,
);

class MonitoringScreen extends ConsumerStatefulWidget {
  const MonitoringScreen({super.key, this.contextFocus, this.setupFocus, this.hostFocus});
  final JobContextFocus? contextFocus;
  final AgentSetupFocus? setupFocus;
  final MonitoringHostFocus? hostFocus;
  @override
  ConsumerState<MonitoringScreen> createState() => _MonitoringScreenState();
}

class _MonitoringScreenState extends ConsumerState<MonitoringScreen> {
  bool _advanced = false;
  bool _agents = false;
  int _setupGeneration = 0;
  AgentSetupVerification? _setupResult;
  AgentSetupFocus? _handledSetupFocus;
  MonitoringHostFocus? _handledHostFocus;
  MonitoringBinding? _linkedHost;
  bool _hostDismissed = false, _linkOpen = false;

  void _showSources() => setState(() {
    _advanced = true;
    _agents = false;
    _readyFocus = null;
  });

  Future<void> _chooseMetricsHost() async {
    final scope = ref.read(monitoringScopeProvider), session = ref.read(monitoringSessionProvider);
    final app = ref.read(appServicesProvider), router = ref.read(routerProvider);
    final uri = router.routeInformationProvider.value.uri;
    final host = await showGlassDialog<Host>(
      context,
      builder: (_) => _MonitoringScopeGuard(scope: scope, requireActive: true, child: const HostPickerDialog()),
    );
    if (!mounted ||
        host == null ||
        !scope.current ||
        !identical(ref.read(monitoringScopeProvider), scope) ||
        !identical(ref.read(monitoringSessionProvider), session) ||
        !identical(ref.read(appServicesProvider), app) ||
        router.routeInformationProvider.value.uri != uri) {
      return;
    }
    openHostMonitoring(ref, host);
  }

  Future<void> _linkMetricsHost(MonitoringHostFocus focus) async {
    final scope = ref.read(monitoringScopeProvider), session = ref.read(monitoringSessionProvider);
    if (session == null || !focus.current(ref) || _linkOpen) return;
    final router = ref.read(routerProvider), uri = ref.read(routerProvider).routeInformationProvider.value.uri;
    var open = true;
    bool current() =>
        open &&
        mounted &&
        focus.current(ref) &&
        identical(widget.hostFocus, focus) &&
        !_hostDismissed &&
        scope.current &&
        identical(ref.read(monitoringSessionProvider), session) &&
        router.routeInformationProvider.value.uri == uri;
    setState(() => _linkOpen = true);
    try {
      await showAppDialog<void>(
        context,
        secure: true,
        builder: (dialogContext) => _MonitoringScopeGuard(
          scope: scope,
          requireActive: true,
          child: MonitoringHostLinkDialog(
            host: MonitoringHostRef(id: focus.host.id.value, name: focus.host.name),
            service: session.service,
            store: session.sources,
            isCurrent: current,
            onLinked: (binding) {
              if (!current()) return;
              setState(() {
                _linkedHost = binding;
                _setupGeneration++;
              });
              Navigator.of(dialogContext).pop();
            },
            onConfigureSource: () {
              if (!current()) return;
              Navigator.of(dialogContext).pop();
              _showSources();
            },
          ),
        ),
      );
    } finally {
      open = false;
      if (mounted) setState(() => _linkOpen = false);
    }
  }

  Future<void> _setup() async {
    final originalScope = ref.read(monitoringScopeProvider), originalSession = ref.read(monitoringSessionProvider);
    final originalApp = ref.read(appServicesProvider), router = ref.read(routerProvider);
    final originalUri = router.routeInformationProvider.value.uri;
    final result = await showAgentSetup(context, ref);
    if (!mounted ||
        result == null ||
        !originalScope.current ||
        !identical(ref.read(monitoringScopeProvider), originalScope) ||
        !identical(ref.read(monitoringSessionProvider), originalSession) ||
        !identical(ref.read(appServicesProvider), originalApp) ||
        router.routeInformationProvider.value.uri != originalUri) {
      return;
    }
    setState(() {
      _setupResult = result;
      _advanced = false;
      _agents = false;
      _setupGeneration++;
    });
  }

  JobContextFocus? _handledFocus, _readyFocus;
  Object? _focusSession, _focusApp, _focusJobs;
  int _focusGeneration = 0;
  bool _focusVisible = false, _focusBusy = false, _focusFailed = false;

  Future<void> receiveMetricsFocus(JobContextFocus focus) async {
    final scope = ref.read(monitoringScopeProvider), session = ref.read(monitoringSessionProvider);
    final jobs = ref.read(automationJobServiceProvider), app = ref.read(appServicesProvider);
    final token = ++_focusGeneration;
    bool current() =>
        mounted &&
        token == _focusGeneration &&
        identical(widget.contextFocus, focus) &&
        scope.current &&
        identical(ref.read(monitoringScopeProvider), scope) &&
        identical(ref.read(monitoringSessionProvider), session) &&
        identical(ref.read(appServicesProvider), app) &&
        identical(ref.read(automationJobServiceProvider), jobs) &&
        ref.read(routerProvider).routeInformationProvider.value.uri.path == AppRoutes.monitoring;
    if (!current() ||
        jobs == null ||
        session == null ||
        !identical(session.correlationScope, focus.scope) ||
        !focus.metadataOnly ||
        focus.selection.destination != JobContextDestination.monitoringBinding) {
      return;
    }
    setState(() {
      _focusBusy = true;
      _focusFailed = false;
      _readyFocus = null;
    });
    try {
      final catalog = await jobs.jobs();
      if (!current()) return;
      if (!jobContextMonitoringJobMatches(focus, catalog)) throw const FormatException('job_context');
      final ready = await receiveJobContextFocus(
        ref,
        focus,
        destination: JobContextDestination.monitoringBinding,
        receiverCurrent: current,
      );
      if (!current()) return;
      if (ready == null) throw const FormatException('job_context');
      setState(() => _readyFocus = ready);
    } catch (_) {
      if (current()) setState(() => _focusFailed = true);
    } finally {
      if (current()) setState(() => _focusBusy = false);
    }
  }

  Future<void> reviewMetricsAi(
    BuildContext context,
    WidgetRef ref,
    MonitoringScope scope,
    MonitoringSession session,
    MonitoringSource source,
    MetricsAiWorkspaceInput input,
  ) async {
    final service = session.service;
    if (service is! MetricsAiNativeCapability || scope.profileId is! ObjectId) return;
    final capability = service as MetricsAiNativeCapability;
    if (!capability.metricsAiSupported) return;
    final profileId = (scope.profileId as ObjectId).value;
    bool originalCurrent() =>
        context.mounted &&
        input.isCurrent() &&
        scope.current &&
        identical(ref.read(monitoringScopeProvider), scope) &&
        identical(ref.read(monitoringSessionProvider), session) &&
        ref.read(routerProvider).routeInformationProvider.value.uri.path == AppRoutes.monitoring;
    if (!originalCurrent()) return;
    final provider = await showAppDialog<AiProviderConfig>(
      context,
      secure: true,
      builder: (pickerContext) => _MonitoringScopeGuard(
        scope: scope,
        requireActive: true,
        child: _MetricsAiProviderPicker(
          isCurrent: originalCurrent,
          onChoose: (provider) {
            if (originalCurrent()) Navigator.of(pickerContext).pop(provider);
          },
          onClose: () => Navigator.of(pickerContext).pop(),
        ),
      ),
    );
    if (!originalCurrent() || provider == null) return;
    String fingerprint(AiProviderConfig value) => jsonEncode([
      value.id.value,
      value.name,
      value.provider.name,
      value.baseUrl,
      value.apiKeySecretId?.value,
      value.chatModel,
      value.embeddingModel,
      value.timeoutSecs,
      value.streaming,
      value.toolSupport,
      value.privacyProfile.name,
      value.isDefault,
      value.createdAt.toIso8601String(),
      value.updatedAt.toIso8601String(),
    ]);
    final originalProvider = fingerprint(provider), providerLease = Object();
    var open = true;
    final providerCurrent = ValueNotifier(true);
    final ai = ref.read(aiServiceProvider);
    final subscription = ai.watchProviders().listen(
      (providers) {
        final next = providers.where((value) => value.id == provider.id).firstOrNull;
        if (next == null || fingerprint(next) != originalProvider) providerCurrent.value = false;
      },
      onError: (Object _) {
        providerCurrent.value = false;
      },
    );
    bool current() => open && providerCurrent.value && originalCurrent() && identical(ref.read(aiServiceProvider), ai);
    final fence = MetricsAiFence(
      profileLease: session,
      metricsLease: input.metricsLease,
      providerLease: providerLease,
      sourceFingerprint: source.fingerprint,
    );
    final transport = MetricsAiNativeTransport(
      capability: capability,
      original: fence,
      profileId: profileId,
      source: source,
      providerId: provider.id.value,
      isCurrent: current,
    );
    final leased = provider.id == deviceLocalAiProviderId && ai is DeviceLocalAiService
        ? MetricsAiLeasedTransport(
            delegate: transport,
            withProvider: (body) => ai.withDeviceLocalProvider(body),
            isCurrent: current,
          )
        : null;
    try {
      if (!context.mounted || !current()) return;
      await showAppDialog<void>(
        context,
        secure: true,
        builder: (dialogContext) => _MonitoringScopeGuard(
          scope: scope,
          requireActive: true,
          child: ValueListenableBuilder<bool>(
            valueListenable: providerCurrent,
            builder: (context, _, _) => MetricsAiReviewDialog(
              fence: fence,
              transport: leased ?? transport,
              providerLabel: provider.name,
              metric: input.metric?.call(fence),
              alerts: input.alerts,
              isCurrent: () => current() && dialogContext.mounted && ModalRoute.of(dialogContext)?.isCurrent == true,
              onClose: () => Navigator.of(dialogContext).pop(),
            ),
          ),
        ),
      );
    } finally {
      open = false;
      transport.dispose();
      leased?.dispose();
      await subscription.cancel();
      providerCurrent.dispose();
    }
  }

  @override
  Widget build(BuildContext context) {
    final originalApp = ref.watch(appServicesProvider);
    final scope = ref.watch(monitoringScopeProvider);
    final session = ref.watch(monitoringSessionProvider);
    final app = ref.watch(appServicesProvider), jobs = ref.watch(automationJobServiceProvider);
    if (!identical(_focusSession, session) || !identical(_focusApp, app) || !identical(_focusJobs, jobs)) {
      _focusGeneration++;
      _readyFocus = null;
      _focusBusy = false;
      _focusFailed = false;
      _setupResult = null;
      _linkedHost = null;
      _advanced = false;
      _agents = false;
      _focusSession = session;
      _focusApp = app;
      _focusJobs = jobs;
    }
    if (!scope.current) return const SizedBox.shrink();
    if (session == null) return Center(child: Text(context.l10n.monitoringUnavailable));
    final credentials = ref.watch(credentialsProvider);
    final hosts = ref.watch(hostsProvider);
    final readerAvailable =
        session.service is AgentReaderCapability && (session.service as AgentReaderCapability).agentReaderPort != null;
    final associationAvailable =
        session.service is AgentAssociationCapability &&
        (session.service as AgentAssociationCapability).agentAssociationPort != null;
    final readerKnown = (readerAvailable || associationAvailable) ? ref.watch(knownHostsProvider) : null;
    final intent = ref.watch(criticalNoticeOpenIntentProvider);
    final target = intent?.target;
    final monitoring = intent?.current == true && target is CriticalMonitoringTarget ? target : null;
    void handled() {
      if (context.mounted && scope.current && identical(ref.read(monitoringScopeProvider), scope) && intent != null) {
        ref.read(criticalNoticeOpenIntentProvider.notifier).take(intent);
      }
    }

    if (monitoring != null && monitoring.sourceId == null) {
      WidgetsBinding.instance.addPostFrameCallback((_) => handled());
    }

    return ValueListenableBuilder<RouteInformation>(
      valueListenable: ref.watch(routerProvider).routeInformationProvider,
      builder: (context, route, _) {
        final visible = route.uri.path == AppRoutes.monitoring;
        if (_focusVisible && !visible) {
          _focusGeneration++;
          _readyFocus = null;
          _focusBusy = false;
          _focusFailed = false;
        }
        _focusVisible = visible;
        final incoming = widget.contextFocus;
        if (visible && incoming != null && !identical(incoming, _handledFocus)) {
          _handledFocus = incoming;
          _readyFocus = null;
          _focusFailed = false;
          WidgetsBinding.instance.addPostFrameCallback((_) {
            if (mounted) receiveMetricsFocus(incoming);
          });
        }
        final focus = _readyFocus;
        final incomingSetup = widget.setupFocus;
        if (visible && incomingSetup != null && !identical(incomingSetup, _handledSetupFocus)) {
          _handledSetupFocus = incomingSetup;
          _setupResult = incomingSetup.result;
          _advanced = false;
          _agents = false;
          _setupGeneration++;
        }
        final incomingHost = widget.hostFocus;
        if (incomingHost != null && !identical(_handledHostFocus, incomingHost)) {
          _handledHostFocus = incomingHost;
          _hostDismissed = false;
          _linkedHost = null;
          _advanced = false;
          _agents = false;
          _readyFocus = null;
          _setupResult = null;
          _setupGeneration++;
        }
        final hostFocus = _hostDismissed ? null : incomingHost;
        final hostCurrent = hostFocus == null || hostFocus.current(ref);
        final showAdvanced = !_agents && (_advanced || focus != null || monitoring != null);
        final setupService = session.service;
        final canSetup =
            setupService is AgentSetupCapability && (setupService as AgentSetupCapability).agentSetupPort != null;
        bool agentCurrent() =>
            mounted &&
            _agents &&
            scope.current &&
            identical(ref.read(monitoringScopeProvider), scope) &&
            identical(ref.read(monitoringSessionProvider), session) &&
            ref.read(routerProvider).routeInformationProvider.value.uri.path == AppRoutes.monitoring;
        Future<void> openReader() async {
          if (!agentCurrent()) return;
          try {
            final sources = await session.sources.load();
            if (!context.mounted || !agentCurrent()) return;
            final store = session.sources;
            final bindings = store is MonitoringBindingStore
                ? (store as MonitoringBindingStore).bindings
                : <MonitoringBinding>[];
            await showAgentReader(context, ref, scope, session, sources, bindings, agentCurrent);
          } catch (error) {
            if (context.mounted && agentCurrent()) {
              ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(errorMessage(context.l10n, error))));
            }
          }
        }

        return Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(20, 16, 20, 0),
              child: Wrap(
                spacing: 8,
                runSpacing: 8,
                crossAxisAlignment: WrapCrossAlignment.center,
                children: [
                  Text(context.l10n.monitoringTitle, style: Theme.of(context).textTheme.headlineSmall),
                  GlassButton.prominent(
                    key: const ValueKey('monitoring-host-choose'),
                    label: context.l10n.monitoringHostMetrics,
                    icon: Icons.monitor_heart_outlined,
                    onPressed: hosts.hasValue ? _chooseMetricsHost : null,
                  ),
                  GlassButton(
                    key: const ValueKey('monitoring-view-toggle'),
                    label: showAdvanced || _agents
                        ? context.l10n.monitoringSetupGraphs
                        : context.l10n.monitoringSourcesSettings,
                    icon: showAdvanced ? Icons.show_chart : Icons.settings_outlined,
                    onPressed: () => setState(() {
                      _advanced = !_agents && !showAdvanced;
                      _agents = false;
                      _readyFocus = null;
                    }),
                  ),
                  GlassButton(
                    key: const ValueKey('monitoring-agents-open'),
                    label: context.l10n.monitoringAgentOverviewTab,
                    icon: Icons.hub_outlined,
                    onPressed: () => setState(() {
                      _agents = true;
                      _advanced = false;
                      _readyFocus = null;
                    }),
                  ),
                  if (_agents && canSetup)
                    GlassButton(
                      key: const ValueKey('monitoring-setup-open'),
                      label: context.l10n.agentSetupTitle,
                      icon: Icons.settings_outlined,
                      onPressed: hosts.hasValue ? _setup : null,
                    ),
                ],
              ),
            ),
            if (_focusBusy) const LinearProgressIndicator(),
            if (hostFocus != null && !_agents && !showAdvanced)
              Padding(
                padding: const EdgeInsets.fromLTRB(20, 8, 20, 0),
                child: Row(
                  children: [
                    Expanded(
                      child: Text(
                        hostCurrent ? hostFocus.host.name : context.l10n.correlationContextChanged,
                        style: Theme.of(context).textTheme.titleMedium,
                      ),
                    ),
                    TextButton(
                      onPressed: () => setState(() {
                        _hostDismissed = true;
                        _linkedHost = null;
                      }),
                      child: Text(context.l10n.monitoringAllGraphs),
                    ),
                  ],
                ),
              ),
            if (_focusFailed) InfoBanner(message: context.l10n.correlationContextChanged, tone: BannerTone.danger),
            if (focus != null)
              InfoBanner(
                message: context.l10n.correlationJobMetricsFocused(focus.selection.jobId!),
                tone: BannerTone.info,
                action: TextButton(
                  key: const ValueKey('monitoring-job-context-dismiss'),
                  onPressed: () {
                    if (mounted && identical(_readyFocus, focus)) setState(() => _readyFocus = null);
                  },
                  child: Text(context.l10n.commonClose),
                ),
              ),
            if (_agents)
              Expanded(
                child: MonitoringAgentOverview(
                  isCurrent: agentCurrent,
                  onOpenAgentReader: readerAvailable && readerKnown?.hasValue == true && hosts.hasValue
                      ? openReader
                      : null,
                  onOpenAgentAssociation: associationAvailable && readerKnown?.hasValue == true && hosts.hasValue
                      ? () => showAgentAssociation(context, ref, scope, session, agentCurrent)
                      : null,
                ),
              ),
            if (!showAdvanced && !_agents && hostCurrent)
              Expanded(
                child: MonitoringDashboard(
                  key: ValueKey((session, _setupGeneration)),
                  service: session.service,
                  store: session.sources,
                  active: visible && !_linkOpen,
                  requestedSourceId: _linkedHost?.sourceId ?? _setupResult?.graphs?.querySourceId,
                  requestedHostId: hostFocus?.host.id.value ?? _setupResult?.hostId,
                  keepHostSelection: hostFocus != null,
                  requestedBindingIds: hostFocus != null
                      ? const []
                      : _linkedHost == null
                      ? _setupResult?.graphs?.bindingIds ?? const []
                      : [_linkedHost!.id],
                  onConfigure: _showSources,
                  onLinkHost: hostFocus == null ? null : () => _linkMetricsHost(hostFocus),
                  isCurrent: () =>
                      mounted &&
                      visible &&
                      scope.current &&
                      identical(ref.read(monitoringScopeProvider), scope) &&
                      identical(ref.read(monitoringSessionProvider), session) &&
                      (hostFocus == null || hostFocus.current(ref)) &&
                      ref.read(routerProvider).routeInformationProvider.value.uri.path == AppRoutes.monitoring,
                ),
              ),
            if (showAdvanced)
              Expanded(
                child: MonitoringWorkspace(
                  key: ObjectKey(session),
                  service: session.service,
                  showHeading: false,
                  panelSyncScope: scope.nativeScope,
                  panelSyncCurrent: () =>
                      context.mounted &&
                      scope.current &&
                      identical(ref.read(monitoringScopeProvider), scope) &&
                      identical(ref.read(monitoringSessionProvider), session) &&
                      identical(ref.read(appServicesProvider), originalApp) &&
                      ref.read(routerProvider).routeInformationProvider.value.uri.path == AppRoutes.monitoring,
                  correlationScope: session.correlationScope,
                  correlationCurrent: () =>
                      context.mounted &&
                      scope.current &&
                      identical(ref.read(monitoringScopeProvider), scope) &&
                      identical(ref.read(monitoringSessionProvider), session),
                  correlationAutomation: ref.watch(automationServiceProvider),
                  onContextNavigation: (focus) {
                    if (!context.mounted ||
                        !scope.current ||
                        !identical(ref.read(monitoringScopeProvider), scope) ||
                        !identical(ref.read(monitoringSessionProvider), session) ||
                        ref.read(routerProvider).routeInformationProvider.value.uri.path != AppRoutes.monitoring) {
                      return;
                    }
                    ref.read(routerProvider).go(jobContextSectionRoute(focus.selection), extra: focus);
                  },
                  store: session.sources,
                  reviewMetricsAi:
                      session.service is MetricsAiNativeCapability &&
                          (session.service as MetricsAiNativeCapability).metricsAiSupported
                      ? (source, input) => reviewMetricsAi(context, ref, scope, session, source, input)
                      : null,
                  requestedSource: focus != null
                      ? (id: focus.metadata.sourceId, revision: focus.metadata.sourceRevision, token: focus)
                      : monitoring?.sourceId != null && monitoring?.sourceRevision != null
                      ? (id: monitoring!.sourceId!, revision: monitoring.sourceRevision!, token: intent!)
                      : null,
                  requestedContext: focus,
                  onContextDismissed: focus == null
                      ? null
                      : () {
                          if (mounted && identical(_readyFocus, focus)) setState(() => _readyFocus = null);
                        },
                  onRequestedSourceHandled: focus == null ? handled : null,
                  openGrafana: () async {
                    var open = true;
                    bool current() =>
                        open &&
                        context.mounted &&
                        scope.current &&
                        identical(ref.read(monitoringScopeProvider), scope) &&
                        identical(ref.read(monitoringSessionProvider), session) &&
                        ref.read(routerProvider).routeInformationProvider.value.uri.path == AppRoutes.monitoring;
                    if (!current()) return;
                    try {
                      await showAppDialog<void>(
                        context,
                        secure: true,
                        builder: (dialogContext) {
                          bool dialogCurrent() =>
                              current() && dialogContext.mounted && ModalRoute.of(dialogContext)?.isCurrent == true;
                          return _MonitoringScopeGuard(
                            scope: scope,
                            requireActive: true,
                            child: MonitoringGrafanaOpen(
                              isCurrent: dialogCurrent,
                              launch: (uri) async {
                                final accepted = reviewGrafanaBrowserUri(uri.toString());
                                if (!dialogCurrent() || accepted == null || accepted != uri) return false;
                                return ref.read(monitoringGrafanaBrowserLauncherProvider)(accepted);
                              },
                              onOpened: () {
                                if (dialogCurrent()) Navigator.of(dialogContext).pop();
                              },
                              onCancel: () {
                                if (dialogCurrent()) Navigator.of(dialogContext).pop();
                              },
                            ),
                          );
                        },
                      );
                    } finally {
                      open = false;
                    }
                  },
                  importGrafana: (sources) async {
                    var open = true;
                    bool current() =>
                        open &&
                        context.mounted &&
                        scope.current &&
                        identical(ref.read(monitoringScopeProvider), scope) &&
                        identical(ref.read(monitoringSessionProvider), session) &&
                        ref.read(routerProvider).routeInformationProvider.value.uri.path == AppRoutes.monitoring;
                    if (!current()) return null;
                    try {
                      final draft = await showAppDialog<GrafanaPanelPrefill>(
                        context,
                        secure: true,
                        builder: (dialogContext) => _MonitoringScopeGuard(
                          scope: scope,
                          requireActive: true,
                          child: MonitoringGrafanaImport(
                            inspectorFactory: ref.read(grafanaDashboardInspectorFactoryProvider),
                            sources: sources,
                            isCurrent: current,
                            loadJson: () async {
                              final path = await ref
                                  .read(fileDialogServiceProvider)
                                  .chooseOpenFile(extensions: ['json']);
                              if (!current() || path == null) return null;
                              return readGrafanaInput(File(path).openRead(), isCurrent: current);
                            },
                            onContinue: (value) {
                              if (current()) Navigator.of(dialogContext).pop(value);
                            },
                            onCancel: () => Navigator.of(dialogContext).pop(),
                          ),
                        ),
                      );
                      return current() ? draft : null;
                    } finally {
                      open = false;
                    }
                  },
                  observation: ref.watch(monitoringPinnedObservationProvider),
                  pinControls: (source, expression, instant, enabled) =>
                      MonitoringPinControls(source: source, expression: expression, instant: instant, enabled: enabled),
                  pinnedObservationBuilder: (timeline) =>
                      MonitoringPinnedView(active: route.uri.path == AppRoutes.monitoring, jobTimeline: timeline),
                  onSourcesLoaded: (sources, readOnly) =>
                      ref.read(monitoringPinnedObservationProvider)?.sourcesChanged(sources, readOnly: readOnly),
                  active: route.uri.path == AppRoutes.monitoring,
                  credentialsLoading: credentials.isLoading,
                  credentialsError: credentials.error == null ? null : errorMessage(context.l10n, credentials.error!),
                  credentials: [
                    for (final c in credentials.value ?? <Credential>[])
                      if (c.kind == CredentialKind.password && c.secretId != null)
                        MonitoringCredentialRef(id: c.id.value, name: c.name, username: c.username),
                  ],
                  createCredential: (auth) async {
                    bool current() =>
                        context.mounted &&
                        scope.current &&
                        identical(ref.read(monitoringScopeProvider), scope) &&
                        ref.read(routerProvider).routeInformationProvider.value.uri.path == AppRoutes.monitoring;
                    if (!current()) return null;
                    final created = await showAppDialog<Credential>(
                      context,
                      secure: true,
                      builder: (_) => _MonitoringScopeGuard(
                        scope: scope,
                        requireActive: true,
                        child: PasswordCredentialDialog(requireUsername: auth == SourceAuth.basicRef),
                      ),
                    );
                    if (!current() || created == null) return null;
                    final refreshed = await ref.refresh(credentialsProvider.future);
                    if (!current()) return null;
                    final saved = refreshed
                        .where((c) => c.id == created.id && c.kind == CredentialKind.password && c.secretId != null)
                        .firstOrNull;
                    return saved == null
                        ? null
                        : MonitoringCredentialRef(id: saved.id.value, name: saved.name, username: saved.username);
                  },
                  hosts: [
                    for (final host in hosts.value ?? <Host>[]) MonitoringHostRef(id: host.id.value, name: host.name),
                  ],
                  sshRouteHosts: [
                    for (final host in hosts.value ?? <Host>[])
                      if (host.protocol == HostProtocol.ssh && host.backend == SshBackend.native)
                        MonitoringHostRef(id: host.id.value, name: host.name),
                  ],
                  chooseSshRouteHost: () async {
                    final originalApp = ref.read(appServicesProvider), originalSession = session;
                    final router = ref.read(routerProvider),
                        uri = ref.read(routerProvider).routeInformationProvider.value.uri;
                    bool current() =>
                        context.mounted &&
                        scope.current &&
                        identical(ref.read(monitoringScopeProvider), scope) &&
                        identical(ref.read(monitoringSessionProvider), originalSession) &&
                        identical(ref.read(appServicesProvider), originalApp) &&
                        identical(ref.read(routerProvider), router) &&
                        uri.path == AppRoutes.monitoring &&
                        router.routeInformationProvider.value.uri == uri;
                    if (!current()) return null;
                    final eligible = (ref.read(hostsProvider).value ?? <Host>[])
                        .where((host) => host.protocol == HostProtocol.ssh && host.backend == SshBackend.native)
                        .toList();
                    final result = await showGlassDialog<Host>(
                      context,
                      builder: (_) => _MonitoringScopeGuard(
                        scope: scope,
                        requireActive: true,
                        child: ProviderScope(
                          overrides: [hostsProvider.overrideWith((ref) => Stream.value(eligible))],
                          child: HostPickerDialog(title: context.l10n.monitoringSshRouteChoose),
                        ),
                      ),
                    );
                    if (!current() || result == null) return null;
                    final saved = (ref.read(hostsProvider).value ?? <Host>[])
                        .where(
                          (host) =>
                              host.id == result.id &&
                              host.protocol == HostProtocol.ssh &&
                              host.backend == SshBackend.native,
                        )
                        .firstOrNull;
                    return saved == null ? null : MonitoringHostRef(id: saved.id.value, name: saved.name);
                  },
                  chooseHost: () async {
                    final result = await showGlassDialog<Host>(
                      context,
                      builder: (_) =>
                          _MonitoringScopeGuard(scope: scope, child: const HostPickerDialog(protocol: null)),
                    );
                    if (!context.mounted ||
                        !scope.current ||
                        !identical(ref.read(monitoringScopeProvider), scope) ||
                        result == null) {
                      return null;
                    }
                    return MonitoringHostRef(id: result.id.value, name: result.name);
                  },
                ),
              ),
          ],
        );
      },
    );
  }
}

class _MetricsAiProviderPicker extends ConsumerWidget {
  const _MetricsAiProviderPicker({required this.isCurrent, required this.onChoose, required this.onClose});
  final bool Function() isCurrent;
  final ValueChanged<AiProviderConfig> onChoose;
  final VoidCallback onClose;
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final providers = ref.watch(aiProvidersProvider), l = context.l10n;
    return GlassDialog(
      title: l.aiChatProviderLabel,
      width: 520,
      content: SizedBox(
        height: 240,
        child: providers.isLoading
            ? const Center(child: CircularProgressIndicator())
            : providers.hasError
            ? InfoBanner(message: l.metricsAiFailed, tone: BannerTone.danger)
            : (providers.value?.isEmpty ?? true)
            ? Text(l.metricsAiNoProvider)
            : ListView(
                children: [
                  for (final provider in providers.value!)
                    ListTile(title: Text(provider.name), onTap: isCurrent() ? () => onChoose(provider) : null),
                ],
              ),
      ),
      secondaryActions: [GlassButton(label: l.commonCancel, onPressed: onClose)],
    );
  }
}

class _MonitoringScopeGuard extends ConsumerWidget {
  const _MonitoringScopeGuard({required this.scope, required this.child, this.requireActive = false});
  final MonitoringScope scope;
  final Widget child;
  final bool requireActive;
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final scopeCurrent = scope.current && identical(ref.watch(monitoringScopeProvider), scope);
    Widget guarded(bool current) {
      if (current) return child;
      final route = ModalRoute.of(context);
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (context.mounted && route != null && route.isActive) Navigator.of(context).removeRoute(route);
      });
      return const SizedBox.shrink();
    }

    if (!requireActive) return guarded(scopeCurrent);
    return ValueListenableBuilder<RouteInformation>(
      valueListenable: ref.watch(routerProvider).routeInformationProvider,
      builder: (_, route, _) => guarded(scopeCurrent && route.uri.path == AppRoutes.monitoring),
    );
  }
}
