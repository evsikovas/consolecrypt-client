import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/monitoring/job_metric_correlation.dart';
import 'package:consolecrypt/monitoring/job_metric_timeline_flow.dart';
import 'package:consolecrypt/monitoring/job_metric_timeline_view.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_ui_proof.dart';
import '../unit/job_metric_correlation_test.dart' as u;
import '../unit/job_metric_provenance_test.dart' as p;
import 'job_metric_timeline_view_test.dart' as f;

Future<JobMetricTimelineSnapshot> snapshot(Map<String, dynamic> row) async {
  final original = f.Fixture();
  addTearDown(original.flow.dispose);
  final flow = JobMetricTimelineFlow(
    selection: original.selection,
    capture: () async => original.metadata,
    recheck: (metadata) async => original.metadata,
    readJobs: () async => u.catalog([row]),
    isCurrent: () => true,
    nowUtc: () => f.utc,
  );
  addTearDown(flow.dispose);
  expect(await flow.load(), isTrue);
  return flow.snapshot!;
}

Future<void> expand(WidgetTester tester) async {
  await tester.tap(find.byKey(ValueKey('job-timeline-row-${u.uuid(100)}')));
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('saved enrolled device uploaded snapshot and actual revision display without user-name inference', (
    tester,
  ) async {
    final row = p.record()..['current_user'] = 'SYNTHETIC_PRIVATE_ACCOUNT';
    final s = await snapshot(row);
    var chosen = 0;
    await f.pump(tester, f.view(snapshot: s, choose: (_) => chosen++));
    await expand(tester);
    final l = tester.element(find.byType(JobMetricTimelineView)).l10n;
    expect(
      tester.widget<SelectableText>(find.byKey(ValueKey('job-timeline-device-${u.uuid(100)}'))).data,
      l.correlationTimelineInitiatorDevice(u.uuid(7)),
    );
    expect(
      tester.widget<SelectableText>(find.byKey(ValueKey('job-timeline-snapshot-${u.uuid(100)}'))).data,
      l.correlationTimelineUploadedSnapshot(u.uuid(8)),
    );
    expect(find.text(l.correlationTimelineProjectRevision(9)), findsOneWidget);
    expect(find.textContaining('SYNTHETIC_PRIVATE'), findsNothing);
    expect(chosen, 0);
  });
  testWidgets('legacy and malformed provenance never render guessed current device or raw fields', (tester) async {
    for (final row in [
      u.row()..['device_id'] = u.uuid(7),
      p.record()..['correlation_provenance'] = {'version': 2, 'deviceId': 'SYNTHETIC_PRIVATE'},
    ]) {
      final s = await snapshot(row);
      await f.pump(tester, f.view(snapshot: s));
      await expand(tester);
      final l = tester.element(find.byType(JobMetricTimelineView)).l10n;
      expect(find.text(l.correlationTimelineInitiatorUnknown), findsOneWidget);
      expect(find.byKey(ValueKey('job-timeline-device-${u.uuid(100)}')), findsNothing);
      expect(find.textContaining('SYNTHETIC_PRIVATE'), findsNothing);
      await tester.pumpWidget(const SizedBox.shrink());
      await tester.pump();
    }
  });
  testWidgets(
    'process receipt uses host chosen-zone formatter and explicit monotonic envelope with target time unknown',
    (tester) async {
      final s = await snapshot(p.timed()), seen = <DateTime>[];
      String format(DateTime value) {
        seen.add(value);
        return 'CHOSEN-ZONE ${value.toIso8601String()}';
      }

      await f.pump(
        tester,
        JobMetricTimelineView(
          snapshot: s,
          busy: false,
          failure: null,
          onRead: () {},
          onCancel: () {},
          onChoose: (_) {},
          formatTime: format,
          timeZoneLabel: 'Chosen zone',
        ),
      );
      await expand(tester);
      final l = tester.element(find.byType(JobMetricTimelineView)).l10n;
      expect(find.text(l.correlationTimelineRunnerEnvelopeMs(2050)), findsOneWidget);
      expect(find.text(l.correlationTimelineTargetExecutionUnknown), findsOneWidget);
      expect(find.text(l.correlationTimelineRunnerClockUnknown), findsOneWidget);
      expect(seen, contains(DateTime.utc(2026, 11, 12, 12, 0, 1, 123, 456)));
      expect(seen, contains(DateTime.utc(2026, 11, 12, 12, 0, 3, 123, 456)));
      expect(find.byKey(ValueKey('job-timeline-clock-reversal-${u.uuid(100)}')), findsNothing);
    },
  );
  testWidgets(
    'cancelled job keeps factual process observations and wall reversal framed without target-success claim',
    (tester) async {
      final row = p.timed()..['state'] = 'cancelled';
      ((row['runner_result'] as Map)['result'] as Map)['runner_execution'] = {
        ...p.receipt(),
        'exitObservedUtc': '2026-11-12T11:59:59Z',
      };
      final s = await snapshot(row);
      await f.pump(tester, f.view(snapshot: s));
      await expand(tester);
      final banner = tester.widget<InfoBanner>(find.byKey(ValueKey('job-timeline-clock-reversal-${u.uuid(100)}')));
      expect(banner.tone, BannerTone.warning);
      expect(s.jobs.markers.single.state, JobMetricState.cancelled);
      final l = tester.element(find.byType(JobMetricTimelineView)).l10n;
      expect(find.text(l.correlationTimelineRunnerEnvelopeMs(2050)), findsOneWidget);
      expect(find.text(l.correlationTimelineTargetExecutionUnknown), findsOneWidget);
    },
  );
  testWidgets('wrong-parent receipt hides fabricated timestamps and presents typed unavailable', (tester) async {
    final row = p.timed();
    (row['runner_result'] as Map)['job_id'] = u.uuid(999);
    final s = await snapshot(row);
    await f.pump(tester, f.view(snapshot: s));
    await expand(tester);
    final l = tester.element(find.byType(JobMetricTimelineView)).l10n;
    expect(find.text(l.correlationTimelineRunnerObservationUnavailable), findsOneWidget);
    expect(find.textContaining('12:00:01.123456'), findsNothing);
  });
  testWidgets('timeline read failure uses the existing framed error surface', (tester) async {
    await f.pump(tester, f.view(failure: JobMetricTimelineFailure.changed));
    final banner = tester.widget<InfoBanner>(find.byKey(const ValueKey('job-timeline-failure')));
    expect(banner.tone, BannerTone.danger);
  });
  for (final width in [1100.0, 460.0]) {
    testWidgets('recorded device and Runner process RU view at $width', (tester) async {
      await loadAutomationProofFonts(tester);
      final s = await snapshot(p.timed());
      await f.pump(
        tester,
        RepaintBoundary(
          key: const ValueKey('automation-proof-root'),
          child: ContentSurface(child: f.view(snapshot: s)),
        ),
        locale: const Locale('ru'),
        size: Size(width, 1100),
        scale: width == 460 ? 1.6 : 1,
      );
      await expand(tester);
      expect(tester.takeException(), isNull);
      await captureAutomationWorkspace(tester, 'cor01-facts-ru-${width.toInt()}');
      await tester.drag(find.byKey(const ValueKey('job-timeline-rows')), const Offset(0, -2000));
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
      await captureAutomationWorkspace(tester, 'cor01-process-ru-${width.toInt()}');
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
  }
}
