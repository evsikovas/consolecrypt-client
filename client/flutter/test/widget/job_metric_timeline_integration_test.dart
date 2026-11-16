import 'dart:async';

import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/monitoring/job_context_focus.dart';
import 'package:consolecrypt/monitoring/job_context_navigation.dart';
import 'package:consolecrypt/monitoring/job_metric_correlation.dart';
import 'package:consolecrypt/monitoring/job_metric_timeline_view.dart';
import 'package:consolecrypt/monitoring/monitoring_binding_models.dart';
import 'package:consolecrypt/monitoring/monitoring_binding_reader.dart';
import 'package:consolecrypt/monitoring/monitoring_host_context.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_pinned_observation.dart';
import 'package:consolecrypt/monitoring/monitoring_pinned_providers.dart';
import 'package:consolecrypt/monitoring/monitoring_pinned_view.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window_controls.dart';
import 'package:consolecrypt/monitoring/monitoring_workspace.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_ui_proof.dart';
import '../helpers/monitoring_binding_fakes.dart';
import '../helpers/monitoring_binding_reads_fakes.dart';
import '../helpers/monitoring_fakes.dart';
import '../helpers/test_app.dart' show settle, tapKey;
import '../unit/job_metric_correlation_test.dart' as f;

class _Reader extends MonitoringBindingReadTestService implements MonitoringHostContextReader {
  final captures = <JobContextMetadata>[], checks = <JobContextMetadata>[];
  Completer<JobContextMetadata>? pendingCheck, pendingCapture;
  @override
  bool get hostContextSupported => true;
  @override
  Future<JobContextMetadata> captureHostContext({
    required String profileId,
    required MonitoringSource source,
    required MonitoringBinding binding,
  }) async {
    final value = JobContextMetadata.parse({
      'version': 1,
      'profileId': profileId,
      'notificationScopeId': f.uuid(900),
      'hostId': binding.hostId,
      'hostRevision': 9,
      'protocol': 'ssh',
      'sourceId': source.id,
      'sourceRevision': source.revision,
      'bindingId': binding.id,
      'bindingRevision': binding.revision,
    })!;
    captures.add(value);
    return pendingCapture?.future ?? value;
  }

  @override
  Future<JobContextMetadata> recheckHostContext(JobContextMetadata original) async {
    checks.add(original);
    return pendingCheck?.future ?? original;
  }
}

class _Jobs implements AutomationService, AutomationJobService {
  int reads = 0, projectReads = 0, fileReads = 0;
  @override
  Future<AutomationJson> jobs() async {
    reads++;
    final row = f.row(hosts: [bindingHost.id], revision: 3);
    return f.catalog([row]);
  }

  @override
  Future<AutomationProjectCatalog> projects() async {
    projectReads++;
    return AutomationProjectCatalog(
      version: 1,
      readOnly: false,
      deviceLocal: true,
      projects: [AutomationProject(id: f.project, name: 'Explicit target project', revision: 6, readOnly: false)],
    );
  }

  @override
  Future<AutomationProjectRevision> files(String id) async {
    fileReads++;
    return AutomationProjectRevision(digest: f.digest, bytes: 20, entries: const []);
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => throw StateError('No execute, file open or mutation is allowed');
}

MonitoringTimeWindow window(int shift) => MonitoringTimeWindow(
  startUtc: f.at.subtract(const Duration(minutes: 10)).add(Duration(minutes: shift)),
  endUtc: f.at.add(Duration(minutes: 10 + shift)),
  stepSeconds: 15,
);

Future<void> _open(
  WidgetTester tester,
  _Reader reader,
  _Jobs jobs,
  MonitoringTimeWindowController controller, {
  MonitoringCorrelationPanel? Function()? accepted,
  bool transferred = false,
  ValueChanged<JobContextFocus>? navigate,
  bool Function()? current,
  Locale locale = const Locale('en'),
  Size size = const Size(1400, 1500),
}) async {
  await tester.binding.setSurfaceSize(size);
  await loadAutomationProofFonts(tester);
  await tester.pumpWidget(
    MaterialApp(
      locale: locale,
      builder: (context, child) => RepaintBoundary(key: const ValueKey('automation-proof-root'), child: child!),
      theme: AppTheme.dark(platform: TargetPlatform.windows),
      supportedLocales: AppLocalizations.supportedLocales,
      localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
      home: Scaffold(
        body: SingleChildScrollView(
          child: MonitoringBindingReaderView(
            reader: reader,
            source: monitoringFixture,
            binding: bindingFixture(),
            timeWindow: controller,
            correlationScope: _scope,
            correlationAutomation: jobs,
            correlationCurrent: current,
            acceptedPanelForCorrelation: accepted == null ? null : (_, _) => accepted(),
            isPanelTransferred: (_) => transferred,
            onContextNavigation: navigate,
          ),
        ),
      ),
    ),
  );
  await settle(tester);
}

final _scope = JobCorrelationScope(f.profile);

void main() {
  testWidgets('timeline does not query on opening and requires accepted panel before explicit history read', (
    tester,
  ) async {
    final reader = _Reader(), jobs = _Jobs(), controller = MonitoringTimeWindowController(initial: window(0));
    addTearDown(controller.dispose);
    addTearDown(() => tester.binding.setSurfaceSize(null));
    await _open(tester, reader, jobs, controller);
    expect(reader.panels, isEmpty);
    expect(reader.captures, isEmpty);
    expect(jobs.reads, 0);
    expect(tester.widget<GlassButton>(find.byKey(const ValueKey('job-timeline-read'))).onPressed, isNull);
    await tapKey(tester, 'monitoring-binding-load-panel');
    expect(reader.panels, hasLength(1));
    expect(jobs.reads, 0);
    await tapKey(tester, 'job-timeline-read');
    expect(jobs.reads, 1);
    expect(reader.captures, hasLength(1));
    expect(reader.checks.single, same(reader.captures.single));
    expect(reader.panels, hasLength(1));
    expect(reader.queries, 0);
    final snapshot = tester.widget<JobMetricTimelineView>(find.byType(JobMetricTimelineView)).snapshot!;
    expect(snapshot.selection.startUtc, window(0).startUtc);
    expect(snapshot.selection.endUtc, window(0).endUtc);
  });
  testWidgets(
    'history uses actual retained owner UTC window rather than requested next window; selection only focuses project',
    (tester) async {
      final reader = _Reader(), jobs = _Jobs(), controller = MonitoringTimeWindowController(initial: window(0));
      addTearDown(controller.dispose);
      addTearDown(() => tester.binding.setSurfaceSize(null));
      final accepted = (
        read: BindingReadResult.fromJson(bindingPanelJson(panel: MonitoringPanel.hostOverview)),
        window: window(3),
      );
      JobContextFocus? focus;
      await _open(
        tester,
        reader,
        jobs,
        controller,
        transferred: true,
        accepted: () => accepted,
        navigate: (value) => focus = value,
      );
      await tapKey(tester, 'job-timeline-read');
      final snapshot = tester.widget<JobMetricTimelineView>(find.byType(JobMetricTimelineView)).snapshot!;
      expect(snapshot.selection.startUtc, window(3).startUtc);
      expect(snapshot.selection.endUtc, window(3).endUtc);
      expect(reader.panels, isEmpty);
      expect(reader.queries, 0);
      await tapKey(tester, 'job-timeline-row-${snapshot.jobs.markers.single.jobId}');
      await tapKey(tester, 'job-timeline-choose-${snapshot.jobs.markers.single.jobId}');
      expect(focus, isNotNull);
      expect(focus!.originalContext!.startUtc, window(3).startUtc);
      expect(focus!.originalContext!.endUtc, window(3).endUtc);
      expect(jobs.projectReads, 1);
      expect(jobs.fileReads, 1);
      expect(reader.checks, hasLength(2));
    },
  );
  testWidgets('late original recheck after accepted owner window advances cannot expose old timeline or focus', (
    tester,
  ) async {
    final reader = _Reader(), jobs = _Jobs(), controller = MonitoringTimeWindowController(initial: window(0));
    addTearDown(controller.dispose);
    addTearDown(() => tester.binding.setSurfaceSize(null));
    MonitoringCorrelationPanel? accepted = (
      read: BindingReadResult.fromJson(bindingPanelJson(panel: MonitoringPanel.hostOverview)),
      window: window(0),
    );
    await _open(tester, reader, jobs, controller, transferred: true, accepted: () => accepted);
    reader.pendingCheck = Completer<JobContextMetadata>();
    await tester.ensureVisible(find.byKey(const ValueKey('job-timeline-read')));
    await tester.tap(find.byKey(const ValueKey('job-timeline-read')));
    await tester.pump();
    expect(reader.checks, hasLength(1));
    accepted = (read: accepted.read, window: window(1));
    reader.pendingCheck!.complete(reader.captures.single);
    await settle(tester);
    expect(tester.widget<JobMetricTimelineView>(find.byType(JobMetricTimelineView)).snapshot, isNull);
    expect(find.byKey(const ValueKey('job-timeline-rows')), findsNothing);
  });
  testWidgets('transferred owner clear cannot resurrect a locally accepted earlier result', (tester) async {
    final reader = _Reader(), jobs = _Jobs(), controller = MonitoringTimeWindowController(initial: window(0));
    addTearDown(controller.dispose);
    addTearDown(() => tester.binding.setSurfaceSize(null));
    await _open(tester, reader, jobs, controller);
    await tapKey(tester, 'monitoring-binding-load-panel');
    await _open(tester, reader, jobs, controller, transferred: true, accepted: () => null);
    expect(tester.widget<GlassButton>(find.byKey(const ValueKey('job-timeline-read'))).onPressed, isNull);
    expect(reader.captures, isEmpty);
    expect(jobs.reads, 0);
  });
  testWidgets('a new accepted rolling window clears previous history without silently rereading jobs', (tester) async {
    final reader = _Reader(), jobs = _Jobs(), controller = MonitoringTimeWindowController(initial: window(0));
    addTearDown(controller.dispose);
    addTearDown(() => tester.binding.setSurfaceSize(null));
    MonitoringCorrelationPanel? accepted = (
      read: BindingReadResult.fromJson(bindingPanelJson(panel: MonitoringPanel.hostOverview)),
      window: window(0),
    );
    await _open(tester, reader, jobs, controller, transferred: true, accepted: () => accepted);
    await tapKey(tester, 'job-timeline-read');
    expect(tester.widget<JobMetricTimelineView>(find.byType(JobMetricTimelineView)).snapshot, isNotNull);
    accepted = (read: accepted.read, window: window(1));
    await _open(tester, reader, jobs, controller, transferred: true, accepted: () => accepted);
    expect(tester.widget<JobMetricTimelineView>(find.byType(JobMetricTimelineView)).snapshot, isNull);
    expect(jobs.reads, 1);
    expect(reader.captures, hasLength(1));
  });

  testWidgets('synchronous original session boundary denies initial metadata completion before any jobs read', (
    tester,
  ) async {
    final reader = _Reader(), jobs = _Jobs(), controller = MonitoringTimeWindowController(initial: window(0));
    addTearDown(controller.dispose);
    addTearDown(() => tester.binding.setSurfaceSize(null));
    final accepted = (
      read: BindingReadResult.fromJson(bindingPanelJson(panel: MonitoringPanel.hostOverview)),
      window: window(0),
    );
    var current = true;
    await _open(tester, reader, jobs, controller, transferred: true, accepted: () => accepted, current: () => current);
    reader.pendingCapture = Completer<JobContextMetadata>();
    await tester.ensureVisible(find.byKey(const ValueKey('job-timeline-read')));
    await tester.tap(find.byKey(const ValueKey('job-timeline-read')));
    await tester.pump();
    expect(reader.captures, hasLength(1));
    expect(jobs.reads, 0);
    current = false; // Existing hub boundary, before a parent widget rebuild.
    reader.pendingCapture!.complete(reader.captures.single);
    await settle(tester);
    expect(jobs.reads, 0);
    expect(reader.checks, isEmpty);
    expect(tester.widget<JobMetricTimelineView>(find.byType(JobMetricTimelineView)).snapshot, isNull);
  });

  for (final width in [460.0, 1400.0]) {
    testWidgets('RU actual accepted panel history service proof width=$width', (tester) async {
      final reader = _Reader(), jobs = _Jobs(), controller = MonitoringTimeWindowController(initial: window(0));
      addTearDown(controller.dispose);
      addTearDown(() => tester.binding.setSurfaceSize(null));
      final accepted = (
        read: BindingReadResult.fromJson(bindingPanelJson(panel: MonitoringPanel.hostOverview)),
        window: window(0),
      );
      await _open(
        tester,
        reader,
        jobs,
        controller,
        transferred: true,
        accepted: () => accepted,
        locale: const Locale('ru'),
        size: Size(width, 1200),
      );
      await tapKey(tester, 'job-timeline-read');
      final snapshot = tester.widget<JobMetricTimelineView>(find.byType(JobMetricTimelineView)).snapshot!;
      await tapKey(tester, 'job-timeline-row-${snapshot.jobs.markers.single.jobId}');
      expect(jobs.reads, 1);
      expect(reader.panels, isEmpty);
      expect(reader.checks, hasLength(1));
      expect(tester.takeException(), isNull);
      await captureAutomationWorkspace(tester, 'cor-job-timeline-${width < 600 ? 'narrow' : 'wide'}-ru');
    });
  }
  testWidgets('manual graph receives exact accepted history; zone edit formats without reading or navigating', (tester) async {
    final reader = _Reader(), jobs = _Jobs(), controller = MonitoringTimeWindowController(initial: window(0));
    addTearDown(controller.dispose);
    addTearDown(() => tester.binding.setSurfaceSize(null));
    await _open(tester, reader, jobs, controller);
    await tapKey(tester, 'monitoring-binding-load-panel');
    await tapKey(tester, 'job-timeline-read');
    final charts = tester.widgetList<MonitoringResultView>(find.byType(MonitoringResultView));
    expect(charts.where((chart) => chart.jobTimeline != null), isNotEmpty);
    final original = charts.firstWhere((chart) => chart.jobTimeline != null).jobTimeline!;
    tester.widget<GlassSelect<bool>>(find.byKey(const ValueKey('job-timeline-time-zone'))).onChanged!(true);
    await settle(tester);
    final local = tester.widgetList<MonitoringResultView>(find.byType(MonitoringResultView))
      .firstWhere((chart) => chart.jobTimeline != null).jobTimeline!;
    expect(local.localTime, isTrue);
    expect(local.snapshot, same(original.snapshot));
    expect(local.snapshot.selection.startUtc.isUtc, isTrue);
    expect(reader.panels, hasLength(1));
    expect(reader.queries, 0);
    expect(jobs.reads, 1);
    expect(jobs.projectReads, 0);
  });
  testWidgets('default Auto graph reuses one accepted owner and existing timeline→project navigation', (tester) async {
    final reader = _Reader(), jobs = _Jobs(), controller = MonitoringTimeWindowController(initial: window(0));
    final store = MonitoringBindingTestStore(rows: [bindingFixture()]);
    final transport = _NoBackgroundReads();
    final owner = MonitoringPinnedObservation(createTransport: () => transport, isCurrent: () => true);
    addTearDown(controller.dispose);
    addTearDown(owner.dispose);
    addTearDown(() => tester.binding.setSurfaceSize(null));
    await tester.binding.setSurfaceSize(const Size(1400, 1400));
    JobContextFocus? focus;
    await tester.pumpWidget(ProviderScope(overrides: [monitoringPinnedObservationProvider.overrideWithValue(owner)],
      child: MaterialApp(theme: AppTheme.dark(platform: TargetPlatform.windows),
        supportedLocales: AppLocalizations.supportedLocales,
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        home: Scaffold(body: MonitoringWorkspace(service: reader, store: store,
          hosts: const [bindingHost], timeWindow: controller, observation: owner,
          correlationScope: _scope, correlationAutomation: jobs,
          onContextNavigation: (value) => focus = value,
          pinnedObservationBuilder: (timeline) => MonitoringPinnedView(jobTimeline: timeline))))));
    await settle(tester);
    await tapKey(tester, 'monitoring-auto-settings');
    expect(tester.widget<CheckboxListTile>(find.byKey(const ValueKey('monitoring-auto-enabled'))).value, isTrue);
    await tapKey(tester, 'monitoring-binding-inspect-${bindingFixture().id}');
    await tapKey(tester, 'monitoring-binding-load-panel');
    expect(owner.state.value, isA<MonitoringObservedBindingPanel>());
    expect(reader.panels, hasLength(1));
    expect(jobs.reads, 0);
    await tapKey(tester, 'job-timeline-read');
    final charts = tester.widgetList<MonitoringResultView>(find.byType(MonitoringResultView));
    expect(charts.where((chart) => chart.jobTimeline != null).length, MonitoringPanel.hostOverview.metrics.length);
    expect(charts.firstWhere((chart) => chart.jobTimeline != null).jobTimeline!.snapshot.selection.startUtc,
      owner.state.value!.window!.startUtc);
    final graph = find.byKey(const ValueKey('monitoring-chart')).first;
    await tester.ensureVisible(graph);
    await tester.pump();
    await tester.tapAt(tester.getRect(graph).center);
    await settle(tester);
    expect(find.byKey(ValueKey('job-chart-selected-${f.uuid(100)}')), findsOneWidget);
    expect(focus, isNull);
    await tapKey(tester, 'job-timeline-choose-${f.uuid(100)}');
    expect(focus, isNotNull);
    expect(jobs.projectReads, 1);
    expect(jobs.fileReads, 1);
    expect(jobs.reads, 1);
    expect(reader.panels, hasLength(1));
    expect(transport.reads, 0);
  });

}

class _NoBackgroundReads implements MonitoringPinTransport {
  int reads = 0;
  @override
  void cancel() {}
  @override
  Future<MonitoringSource?> loadSource(String sourceId) async { reads++; throw StateError('No hidden source reads'); }
  @override
  Future<QueryOutcome> readCurrent(MonitoringSource source, String expression) async { reads++; throw StateError('No hidden query'); }
}
