import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_scope_guard.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/monitoring/job_context_focus.dart';
import 'package:consolecrypt/monitoring/job_context_project_guard.dart';
import 'package:consolecrypt/monitoring/job_metric_correlation.dart';
import 'package:consolecrypt/monitoring/monitoring_host_context.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_providers.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

bool jobMetricsAvailable(WidgetRef ref) {
  final scope = ref.read(monitoringScopeProvider),
      session = ref.read(monitoringSessionProvider);
  if (!scope.current || session == null || session.correlationScope == null) {
    return false;
  }
  return session.sources is MonitoringBindingStore &&
      session.service is MonitoringHostContextReader &&
      (session.service as MonitoringHostContextReader).hostContextSupported;
}

/// Capture the existing owners before any picker/catalog/metadata await. Only
/// a fixed in-memory focus packet crosses sections, not a query or endpoint.
Future<void> showJobMetricsChooser(
  BuildContext context,
  WidgetRef ref, {
  required AutomationScope scope,
  required AutomationJobService jobs,
  required AutomationJson job,
  required bool Function() producerCurrent,
}) async {
  final app = ref.read(appServicesProvider),
      session = ref.read(monitoringSessionProvider);
  final monitoringScope = ref.read(monitoringScopeProvider),
      router = ref.read(routerProvider);
  final originPath = router.routeInformationProvider.value.uri.path;
  if (!jobMetricsAvailable(ref) ||
      session == null ||
      monitoringScope.profileId is! ObjectId ||
      scope.profileId is! ObjectId) {
    return;
  }
  final profileId = (monitoringScope.profileId as ObjectId).value;
  bool original() =>
      context.mounted &&
      producerCurrent() &&
      (scope.profileId as ObjectId).value == profileId &&
      identical(ref.read(appServicesProvider), app) &&
      identical(ref.read(automationScopeProvider), scope) &&
      scope.unlocked &&
      identical(ref.read(automationJobServiceProvider), jobs) &&
      monitoringScope.current &&
      identical(ref.read(monitoringScopeProvider), monitoringScope) &&
      identical(ref.read(monitoringSessionProvider), session) &&
      router.routeInformationProvider.value.uri.path == originPath;
  if (!original()) return;
  final packet = await showAppDialog<JobContextFocus>(
    context,
    builder: (dialogContext) {
      final navigator = Navigator.of(dialogContext),
          route = ModalRoute.of(dialogContext);
      void dismiss() => WidgetsBinding.instance.addPostFrameCallback((_) {
        if (navigator.mounted && route?.isActive == true) {
          navigator.removeRoute(route!);
        }
      });
      return AutomationScopeGuard(
        scope: scope,
        builder: (_) => Consumer(
          builder: (_, inner, _) {
            inner.watch(appServicesProvider);
            inner.watch(monitoringScopeProvider);
            inner.watch(monitoringSessionProvider);
            inner.watch(automationJobServiceProvider);
            final hosts = inner.watch(hostsProvider).value ?? const <Host>[];
            return ValueListenableBuilder<RouteInformation>(
              valueListenable: router.routeInformationProvider,
              builder: (_, _, _) {
                if (!original()) {
                  dismiss();
                  return const SizedBox.shrink();
                }
                return JobMetricsChooser(
                  job: job,
                  profileId: profileId,
                  scope: session.correlationScope!,
                  sourceStore: session.sources,
                  bindings: session.sources as MonitoringBindingStore,
                  reader: session.service as MonitoringHostContextReader,
                  jobs: jobs,
                  hosts: [
                    for (final host in hosts)
                      MonitoringHostRef(id: host.id.value, name: host.name),
                  ],
                  isCurrent: original,
                  onCancel: dismiss,
                  onChoose: (value) {
                    if (original() && route?.isCurrent == true) {
                      navigator.pop(value);
                    }
                  },
                );
              },
            );
          },
        ),
      );
    },
  );
  if (packet == null || !original()) return;
  // The source page hiding is not a profile/session retirement. The receiver
  // consumes this once and rechecks the ORIGINAL Native nonce before focusing.
  router.go(AppRoutes.monitoring, extra: packet);
}

/// Actual saved references only; source/binding names and addresses are never
/// matched to an approved host or used as permission/installation evidence.
class JobMetricsChooser extends StatefulWidget {
  const JobMetricsChooser({
    super.key,
    required this.job,
    required this.profileId,
    required this.scope,
    required this.sourceStore,
    required this.bindings,
    required this.reader,
    required this.jobs,
    required this.hosts,
    required this.isCurrent,
    required this.onChoose,
    required this.onCancel,
  });
  final AutomationJson job;
  final String profileId;
  final JobCorrelationScope scope;
  final MonitoringSourceStore sourceStore;
  final MonitoringBindingStore bindings;
  final MonitoringHostContextReader reader;
  final AutomationJobService jobs;
  final List<MonitoringHostRef> hosts;
  final bool Function() isCurrent;
  final ValueChanged<JobContextFocus> onChoose;
  final VoidCallback onCancel;
  @override
  State<JobMetricsChooser> createState() => _JobMetricsChooserState();
}

class _JobMetricsChooserState extends State<JobMetricsChooser> {
  List<MonitoringSource> sources = [];
  Set<String> approved = {};
  String? hostId, sourceId, bindingId;
  String query = '';
  bool loading = true, busy = false, failed = false;
  int generation = 0;
  bool get current => mounted && widget.isCurrent();
  MonitoringSource? get source =>
      sources.where((s) => s.id == sourceId).firstOrNull;
  List<MonitoringBinding> get bindings => widget.bindings.bindings
      .where(
        (b) =>
            b.hostId == hostId &&
            b.sourceId == sourceId &&
            b.sourceRevision == source?.revision &&
            b.state == MonitoringBindingState.current,
      )
      .toList();
  MonitoringBinding? get binding =>
      bindings.where((b) => b.id == bindingId).firstOrNull;

  @override
  void initState() {
    super.initState();
    load();
  }

  @override
  void didUpdateWidget(JobMetricsChooser old) {
    super.didUpdateWidget(old);
    if (!identical(old.scope, widget.scope) ||
        old.profileId != widget.profileId ||
        old.job['job_id'] != widget.job['job_id'] ||
        old.job['manifest_digest'] != widget.job['manifest_digest'] ||
        old.job['notification_revision'] !=
            widget.job['notification_revision'] ||
        !identical(old.reader, widget.reader) ||
        !identical(old.sourceStore, widget.sourceStore) ||
        !identical(old.bindings, widget.bindings) ||
        !identical(old.jobs, widget.jobs)) {
      generation++;
      sources = [];
      approved = {};
      hostId = null;
      sourceId = null;
      bindingId = null;
      loading = false;
      busy = false;
      failed = true;
    }
  }

  @override
  void dispose() {
    generation++;
    super.dispose();
  }

  Future<void> load() async {
    final token = ++generation, store = widget.sourceStore;
    final job = widget.job;
    final id = job['job_id'],
        rev = job['notification_revision'],
        project = job['project_id'],
        digest = job['manifest_digest'];
    try {
      if (!current ||
          id is! String ||
          rev is! int ||
          project is! String ||
          digest is! String) {
        throw const FormatException('job');
      }
      final ids = jobContextApprovedHostIds(
        job,
        profileId: widget.profileId,
        jobId: id,
        revision: rev,
        projectId: project,
        manifestDigest: digest,
      );
      if (ids == null) throw const FormatException('job');
      final values = await store.load();
      if (!current ||
          token != generation ||
          !identical(store, widget.sourceStore)) {
        return;
      }
      if (store.readOnly ||
          values.length > 128 ||
          widget.bindings.bindings.length > 10000) {
        throw const FormatException('catalog');
      }
      setState(() {
        approved = ids;
        sources = values.where((s) => !s.readOnly && s.revision > 0).toList();
      });
    } catch (_) {
      if (current && token == generation) setState(() => failed = true);
    } finally {
      if (current && token == generation) setState(() => loading = false);
    }
  }

  Future<void> open() async {
    final source = this.source,
        binding = this.binding,
        reader = widget.reader,
        jobs = widget.jobs;
    if (!current ||
        loading ||
        busy ||
        source == null ||
        binding == null ||
        !approved.contains(binding.hostId) ||
        !widget.hosts.any((host) => host.id == binding.hostId)) {
      return;
    }
    final token = ++generation, scope = widget.scope, job = widget.job;
    final jobId = job['job_id'],
        jobRevision = job['notification_revision'],
        projectId = job['project_id'],
        digest = job['manifest_digest'];
    if (jobId is! String ||
        jobRevision is! int ||
        projectId is! String ||
        digest is! String) {
      return;
    }
    bool original() =>
        current &&
        token == generation &&
        identical(widget.scope, scope) &&
        identical(widget.reader, reader) &&
        identical(widget.jobs, jobs) &&
        this.source?.fingerprint == source.fingerprint &&
        this.binding?.id == binding.id &&
        this.binding?.revision == binding.revision;
    setState(() {
      busy = true;
      failed = false;
    });
    try {
      final captured = await reader.captureHostContext(
        profileId: widget.profileId,
        source: source,
        binding: binding,
      );
      if (!original()) return;
      final focus = JobContextFocus.forMonitoring(
        scope: scope,
        metadata: captured,
        jobId: jobId,
        jobRevision: jobRevision,
        projectId: projectId,
        manifestDigest: digest,
      );
      if (focus == null ||
          captured.hostId != binding.hostId ||
          captured.sourceId != source.id ||
          captured.sourceRevision != source.revision ||
          captured.bindingId != binding.id ||
          captured.bindingRevision != binding.revision) {
        throw const FormatException('context');
      }
      final catalog = await jobs.jobs();
      if (!original()) return;
      if (!jobContextMonitoringJobMatches(focus, catalog)) {
        throw const FormatException('job_changed');
      }
      final checked = await reader.recheckHostContext(captured);
      if (!original()) return;
      if (!sameJobContextMetadata(captured, checked)) {
        throw const FormatException('context_changed');
      }
      widget.onChoose(focus);
    } catch (_) {
      if (original()) setState(() => failed = true);
    } finally {
      if (original()) setState(() => busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    if (!widget.isCurrent()) return const SizedBox.shrink();
    final l = context.l10n;
    final hosts = widget.hosts
        .where(
          (h) =>
              approved.contains(h.id) &&
              (query.isEmpty ||
                  h.name.toLowerCase().contains(query) ||
                  h.id.contains(query)),
        )
        .take(64)
        .toList();
    return GlassDialog(
      key: const ValueKey('job-metrics-chooser'),
      title: l.correlationJobMetricsTitle,
      width: 640,
      content: SingleChildScrollView(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(l.correlationJobMetricsHint),
            if (loading || busy) const LinearProgressIndicator(),
            if (failed)
              InfoBanner(
                message: l.correlationContextChanged,
                tone: BannerTone.danger,
              ),
            if (!loading && sources.isEmpty)
              Text(l.correlationJobMetricsNoBindings),
            GlassField(
              fieldKey: const ValueKey('job-metrics-host-search'),
              search: true,
              placeholder: l.correlationJobMetricsHostSearch,
              enabled: !loading && !busy,
              inputFormatters: [LengthLimitingTextInputFormatter(128)],
              onChanged: (value) => setState(() {
                query = value.toLowerCase();
                hostId = null;
                bindingId = null;
              }),
            ),
            GlassSelect<String?>(
              key: const ValueKey('job-metrics-host'),
              value: hostId,
              expand: true,
              semanticLabel: l.monitoringBindingHost,
              items: [
                GlassSelectItem(
                  value: null,
                  label: l.monitoringBindingChooseHost,
                ),
                for (final host in hosts)
                  GlassSelectItem(value: host.id, label: host.name),
              ],
              onChanged: loading || busy
                  ? null
                  : (value) => setState(() {
                      hostId = value;
                      bindingId = null;
                    }),
            ),
            const SizedBox(height: 12),
            GlassSelect<String?>(
              key: const ValueKey('job-metrics-source'),
              value: sourceId,
              expand: true,
              semanticLabel: l.monitoringSources,
              items: [
                GlassSelectItem(
                  value: null,
                  label: l.correlationJobMetricsChooseSource,
                ),
                for (final item in sources)
                  GlassSelectItem(value: item.id, label: item.name),
              ],
              onChanged: loading || busy
                  ? null
                  : (value) => setState(() {
                      sourceId = value;
                      bindingId = null;
                    }),
            ),
            const SizedBox(height: 12),
            if (hostId != null && sourceId != null && bindings.isEmpty)
              Text(l.correlationJobMetricsNoBindings),
            GlassSelect<String?>(
              key: const ValueKey('job-metrics-binding'),
              value: bindingId,
              expand: true,
              semanticLabel: l.monitoringBindingName,
              items: [
                GlassSelectItem(
                  value: null,
                  label: l.correlationJobMetricsChooseBinding,
                ),
                for (final item in bindings)
                  GlassSelectItem(value: item.id, label: item.name),
              ],
              onChanged: loading || busy
                  ? null
                  : (value) => setState(() => bindingId = value),
            ),
          ],
        ),
      ),
      secondaryActions: [
        TextButton(onPressed: widget.onCancel, child: Text(l.commonCancel)),
      ],
      primaryAction: GlassButton(
        key: const ValueKey('job-metrics-open'),
        label: l.correlationJobMetricsOpen,
        onPressed: loading || busy || binding == null ? null : open,
      ),
    );
  }
}
