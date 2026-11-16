import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/monitoring/job_context_navigation.dart';
import 'package:consolecrypt/monitoring/job_metric_correlation.dart';
import 'package:consolecrypt/monitoring/job_metric_timeline_flow.dart';
import 'package:consolecrypt/monitoring/job_metric_timeline_view.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_result_view.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window_controls.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_ui_proof.dart';
import '../helpers/test_app.dart' show settle, tapKey;
import '../unit/job_metric_correlation_test.dart' as f;
import '../unit/job_metric_provenance_test.dart' as p;
import 'job_metric_timeline_view_test.dart' as ui;

class Fixture {
  Fixture({List<Object?>? rows}) {
    final selection = JobMetricTimelineSelection(
      scope: JobCorrelationScope(f.profile),
      hostId: f.host,
      sourceId: f.source,
      sourceRevision: 3,
      bindingId: f.binding,
      bindingRevision: 4,
      startUtc: window.startUtc,
      endUtc: window.endUtc,
    );
    final metadata = JobContextMetadata.parse({
      'version': 1,
      'profileId': f.profile,
      'notificationScopeId': f.uuid(900),
      'hostId': f.host,
      'hostRevision': 7,
      'protocol': 'ssh',
      'sourceId': f.source,
      'sourceRevision': 3,
      'bindingId': f.binding,
      'bindingRevision': 4,
    })!;
    flow = JobMetricTimelineFlow(
      selection: selection,
      capture: () async => metadata,
      recheck: (original) async => original,
      isCurrent: () => current,
      nowUtc: () => f.at,
      readJobs: () async {
        reads++;
        return f.catalog(rows ?? [f.row()]);
      },
    );
  }
  bool current = true;
  int reads = 0;
  late final JobMetricTimelineFlow flow;
  final choices = <JobMetricMarker>[];
  MonitoringCorrelationTimeline timeline({bool local = false}) => (
    owner: flow,
    snapshot: flow.snapshot!,
    window: window,
    openProject: choices.add,
    localTime: local,
  );
}

final window = MonitoringTimeWindow(
  startUtc: f.at.subtract(const Duration(minutes: 10)),
  endUtc: f.at.add(const Duration(minutes: 10)),
  stepSeconds: 15,
);
final outcome = QueryOutcome(
  status: 'success',
  httpStatus: 200,
  data: QueryData(
    'matrix',
    series: [
      MonitoringSeries(
        const {'job': 'synthetic'},
        [
          MonitoringSample(window.startSeconds, SampleKind.finite, 0),
          MonitoringSample(f.seconds(f.at).toDouble(), SampleKind.nan),
          MonitoringSample(window.endSeconds, SampleKind.finite, 2),
        ],
      ),
    ],
  ),
  effectiveStepSeconds: 15,
);

Future<void> open(
  WidgetTester tester,
  MonitoringCorrelationTimeline? timeline, {
  MonitoringTimeWindow? requested,
  MonitoringTimeWindowController? controller,
  bool active = true,
  Locale locale = const Locale('en'),
  double width = 1100,
}) => ui.pump(
  tester,
  RepaintBoundary(
    key: const ValueKey('automation-proof-root'),
    child: MonitoringResultView(
      result: outcome,
      requestedWindow: requested ?? window,
      timeWindow: controller,
      jobTimeline: timeline,
      active: active,
    ),
  ),
  locale: locale,
  size: Size(width, 1000),
);

List<double> paintedAdmissions(WidgetTester tester) {
  final dynamic painter = tester
      .widget<CustomPaint>(find.byKey(const ValueKey('monitoring-chart-paint')))
      .painter;
  // The production painter is private; inspect only its non-authoritative
  // rendered timestamps to prove that no clock offset or clamping was invented.
  // ignore: avoid_dynamic_calls
  return List<double>.of(painter.admissions as List<double>);
}

Future<void> clickMiddle(WidgetTester tester) async {
  final graph = find.byKey(const ValueKey('monitoring-chart'));
  await tester.ensureVisible(graph);
  await tester.pump();
  await tester.tapAt(tester.getRect(graph).center);
  await settle(tester);
}

void main() {
  testWidgets(
    'unknown skew renders explicitly nominal client admission without inventing alignment',
    (tester) async {
      final fixture = Fixture();
      addTearDown(fixture.flow.dispose);
      expect(await fixture.flow.load(), isTrue);
      final marker = fixture.flow.snapshot!.jobs.markers.single;
      expect(marker.position, isNull);
      expect(marker.unpositioned, JobMarkerUnpositioned.unknownClockSkew);
      await open(tester, fixture.timeline());
      expect(paintedAdmissions(tester), [f.seconds(f.at).toDouble()]);
      expect(
        find.byKey(const ValueKey('job-chart-clock-notice')),
        findsOneWidget,
      );
      expect(find.textContaining('offset is unknown'), findsOneWidget);
      expect(fixture.reads, 1);
      expect(outcome.data!.series.single.samples[0].value, 0);
      expect(outcome.data!.series.single.samples[1].kind, SampleKind.nan);
    },
  );

  testWidgets(
    'clicking graph admission shows existing job details; explicit revision action keeps original marker',
    (tester) async {
      final fixture = Fixture();
      addTearDown(fixture.flow.dispose);
      await fixture.flow.load();
      final marker = fixture.flow.snapshot!.jobs.markers.single;
      await open(tester, fixture.timeline());
      await clickMiddle(tester);
      expect(
        find.byKey(ValueKey('job-chart-selected-${marker.jobId}')),
        findsOneWidget,
      );
      expect(find.textContaining(marker.revisionDigest), findsOneWidget);
      expect(fixture.choices, isEmpty);
      await tapKey(tester, 'job-timeline-choose-${marker.jobId}');
      expect(fixture.choices.single, same(marker));
      expect(fixture.reads, 1);
      expect(find.textContaining('secret-sentinel'), findsNothing);
    },
  );

  testWidgets(
    'legacy missing admission cannot draw a line or enable revision-zero navigation',
    (tester) async {
      final row = f.row(revision: 0)..remove('accepted_at');
      final fixture = Fixture(rows: [row]);
      addTearDown(fixture.flow.dispose);
      await fixture.flow.load();
      await open(tester, fixture.timeline());
      expect(paintedAdmissions(tester), isEmpty);
      await tapKey(tester, 'job-chart-jobs');
      final marker = fixture.flow.snapshot!.jobs.markers.single;
      await tapKey(tester, 'job-timeline-row-${marker.jobId}');
      expect(
        tester
            .widget<TextButton>(
              find.byKey(ValueKey('job-timeline-choose-${marker.jobId}')),
            )
            .onPressed,
        isNull,
      );
      expect(fixture.choices, isEmpty);
      expect(marker.provenance, isNull);
    },
  );

  testWidgets(
    'out-of-period admission is listed honestly and never clamped into the plot',
    (tester) async {
      final fixture = Fixture(
        rows: [
          f.row()
            ..['accepted_at'] = f.seconds(
              window.endUtc.add(const Duration(days: 1)),
            ),
        ],
      );
      addTearDown(fixture.flow.dispose);
      await fixture.flow.load();
      await open(tester, fixture.timeline());
      expect(paintedAdmissions(tester), isEmpty);
      await tapKey(tester, 'job-chart-jobs');
      expect(
        find.textContaining('outside the selected period'),
        findsOneWidget,
      );
      await clickMiddle(tester);
      expect(
        find.byKey(ValueKey('job-chart-selected-${f.uuid(100)}')),
        findsNothing,
      );
      expect(fixture.reads, 1);
    },
  );

  testWidgets(
    'retired original scope denies retained graph callback and removes all marks',
    (tester) async {
      final fixture = Fixture();
      addTearDown(fixture.flow.dispose);
      await fixture.flow.load();
      final timeline = fixture.timeline();
      await open(tester, timeline);
      await clickMiddle(tester);
      final button = tester
          .widget<TextButton>(
            find.byKey(ValueKey('job-timeline-choose-${f.uuid(100)}')),
          )
          .onPressed!;
      fixture.current = false;
      button();
      expect(fixture.choices, isEmpty);
      await open(tester, timeline);
      expect(paintedAdmissions(tester), isEmpty);
      expect(find.byType(JobMetricChartAnnotations), findsNothing);
    },
  );

  testWidgets(
    'another accepted window cannot relabel old history even while original owner remains valid',
    (tester) async {
      final fixture = Fixture();
      addTearDown(fixture.flow.dispose);
      await fixture.flow.load();
      await open(
        tester,
        fixture.timeline(),
        requested: MonitoringTimeWindow(
          startUtc: window.startUtc,
          endUtc: window.endUtc.add(const Duration(seconds: 15)),
          stepSeconds: 15,
        ),
      );
      expect(paintedAdmissions(tester), isEmpty);
      expect(find.byType(JobMetricChartAnnotations), findsNothing);
      expect(fixture.flow.snapshot!.selection.endUtc, window.endUtc);
    },
  );

  testWidgets(
    'viewport pan filters nominal lines without rereading jobs or changing accepted window',
    (tester) async {
      final fixture = Fixture(),
          controller = MonitoringTimeWindowController(initial: window);
      addTearDown(fixture.flow.dispose);
      addTearDown(controller.dispose);
      await fixture.flow.load();
      await open(tester, fixture.timeline(), controller: controller);
      expect(paintedAdmissions(tester), hasLength(1));
      controller.zoom(.1, anchor: 0);
      await settle(tester);
      expect(paintedAdmissions(tester), isEmpty);
      expect(fixture.reads, 1);
      expect(fixture.flow.snapshot!.selection.startUtc, window.startUtc);
      controller.resetViewport();
      await settle(tester);
      expect(paintedAdmissions(tester), hasLength(1));
    },
  );

  testWidgets('overlapping admissions never arbitrarily choose one job', (
    tester,
  ) async {
    final fixture = Fixture(rows: [f.row(), f.row(id: 101)]);
    addTearDown(fixture.flow.dispose);
    await fixture.flow.load();
    await open(tester, fixture.timeline());
    await clickMiddle(tester);
    expect(
      find.byKey(ValueKey('job-chart-selected-${f.uuid(100)}')),
      findsNothing,
    );
    expect(
      find.byKey(ValueKey('job-chart-selected-${f.uuid(101)}')),
      findsNothing,
    );
    await tapKey(tester, 'job-chart-jobs');
    expect(find.byType(JobMetricTimelineEntry), findsNWidgets(2));
  });

  testWidgets(
    'explicit local formatting does not change UTC data, query period or admission coordinates',
    (tester) async {
      final fixture = Fixture();
      addTearDown(fixture.flow.dispose);
      await fixture.flow.load();
      final snapshot = fixture.flow.snapshot!;
      await open(tester, fixture.timeline(local: true));
      await clickMiddle(tester);
      final l = tester.element(find.byType(MonitoringResultView)).l10n;
      expect(find.textContaining(l.correlationLocalTime), findsWidgets);
      expect(paintedAdmissions(tester), [f.seconds(f.at).toDouble()]);
      expect(snapshot.selection.startUtc.isUtc, isTrue);
      expect(snapshot.jobs.markers.single.admitted!.utc, f.at);
      expect(fixture.reads, 1);
    },
  );

  testWidgets(
    'graph job details retain known runner reversal and exact device/snapshot provenance',
    (tester) async {
      final row = p.timed();
      ((row['runner_result'] as Map)['result'] as Map)['runner_execution'] =
          p.receipt()..['exitObservedUtc'] = '2026-11-12T11:59:00Z';
      final fixture = Fixture(rows: [row]);
      addTearDown(fixture.flow.dispose);
      await fixture.flow.load();
      await open(tester, fixture.timeline());
      await clickMiddle(tester);
      final l = tester.element(find.byType(MonitoringResultView)).l10n;
      expect(
        find.byKey(ValueKey('job-timeline-clock-reversal-${f.uuid(100)}')),
        findsOneWidget,
      );
      expect(
        find.text(l.correlationTimelineInitiatorDevice(f.uuid(7))),
        findsOneWidget,
      );
      expect(
        find.text(l.correlationTimelineUploadedSnapshot(f.uuid(8))),
        findsOneWidget,
      );
      expect(
        find.text(l.correlationTimelineTargetExecutionUnknown),
        findsOneWidget,
      );
      expect(paintedAdmissions(tester), [f.seconds(f.at).toDouble()]);
    },
  );
  for (final width in [460.0, 1400.0]) {
    testWidgets('RU graph annotations remain readable width=$width', (
      tester,
    ) async {
      await loadAutomationProofFonts(tester);
      final fixture = Fixture(rows: [p.timed()]);
      addTearDown(fixture.flow.dispose);
      await fixture.flow.load();
      await open(
        tester,
        fixture.timeline(),
        locale: const Locale('ru'),
        width: width,
      );
      await clickMiddle(tester);
      expect(paintedAdmissions(tester), hasLength(1));
      expect(tester.takeException(), isNull);
      await captureAutomationWorkspace(
        tester,
        'cor01-graph-${width < 600 ? 'narrow' : 'wide'}-ru',
      );
    });
  }

  testWidgets(
    'ordinary chart remains usable without a correlation owner or any history read',
    (tester) async {
      await open(tester, null);
      expect(paintedAdmissions(tester), isEmpty);
      expect(find.byType(JobMetricChartAnnotations), findsNothing);
      await clickMiddle(tester);
      expect(tester.takeException(), isNull);
    },
  );
}
