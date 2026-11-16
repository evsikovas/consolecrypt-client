import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/monitoring/job_context_navigation.dart';
import 'package:consolecrypt/monitoring/job_metric_correlation.dart';
import 'package:consolecrypt/monitoring/job_metric_timeline_flow.dart';
import 'package:consolecrypt/monitoring/job_metric_timeline_view.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

String id(int n) => '00000000-0000-0000-0000-${n.toRadixString(16).padLeft(12, '0')}';
final utc = DateTime.utc(2026, 11, 12, 12);
final digest = List.filled(64, 'a').join();

/// Synthetic history data, not a job execution/admission/identity witness.
final class Fixture {
  Fixture({this.count = 1, this.revision = 1}) {
    selection = JobMetricTimelineSelection(
      scope: JobCorrelationScope(id(1)),
      hostId: id(2),
      sourceId: id(3),
      sourceRevision: 3,
      bindingId: id(4),
      bindingRevision: 4,
      startUtc: utc.subtract(const Duration(minutes: 10)),
      endUtc: utc.add(const Duration(minutes: 10)),
    );
    metadata = JobContextMetadata.parse({
      'version': 1,
      'profileId': id(1),
      'notificationScopeId': id(900),
      'hostId': id(2),
      'hostRevision': 7,
      'protocol': 'ssh',
      'sourceId': id(3),
      'sourceRevision': 3,
      'bindingId': id(4),
      'bindingRevision': 4,
    })!;
    flow = JobMetricTimelineFlow(
      selection: selection,
      capture: () async => metadata,
      recheck: (original) async => metadata,
      isCurrent: () => current,
      nowUtc: () => utc,
      readJobs: () async => {
        'schema_version': 1,
        'read_only': false,
        'jobs': [
          for (var i = 0; i < count; i++)
            {
              'version': 1,
              'job_id': id(100 + i),
              'project_id': id(5),
              'notification_revision': revision,
              'state': 'succeeded',
              'host_ids': [id(2)],
              'manifest_digest': digest,
              'accepted_at': utc.millisecondsSinceEpoch ~/ 1000,
              'updated_at': utc.millisecondsSinceEpoch ~/ 1000,
              'manifest': {
                'version': 1,
                'job_id': id(100 + i),
                'tenant_id': id(1),
                'project_id': id(5),
                'revision_digest': digest,
                'targets': [
                  {'host_id': id(2), 'address': 'never-render.invalid', 'username': 'secret-sentinel'},
                ],
              },
            },
        ],
      },
    );
  }
  final int count;
  int revision;
  bool current = true;
  late final JobMetricTimelineSelection selection;
  late JobContextMetadata metadata;
  late final JobMetricTimelineFlow flow;
}

Future<void> pump(
  WidgetTester tester,
  Widget child, {
  Locale locale = const Locale('en'),
  Size size = const Size(1100, 900),
  double scale = 1,
}) async {
  await tester.binding.setSurfaceSize(size);
  addTearDown(() => tester.binding.setSurfaceSize(null));
  await tester.pumpWidget(
    MaterialApp(
      locale: locale,
      supportedLocales: AppLocalizations.supportedLocales,
      localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
      theme: AppTheme.dark(platform: TargetPlatform.windows),
      builder: (context, child) => MediaQuery(
        data: MediaQuery.of(context).copyWith(textScaler: TextScaler.linear(scale)),
        child: child!,
      ),
      home: Scaffold(body: SingleChildScrollView(child: child)),
    ),
  );
  await tester.pump();
}

JobMetricTimelineView view({
  JobMetricTimelineSnapshot? snapshot,
  bool busy = false,
  bool enabled = true,
  JobMetricTimelineFailure? failure,
  VoidCallback? read,
  VoidCallback? cancel,
  ValueChanged<JobMetricMarker>? choose,
}) => JobMetricTimelineView(
  snapshot: snapshot,
  busy: busy,
  enabled: enabled,
  failure: failure,
  onRead: read ?? () {},
  onCancel: cancel ?? () {},
  onChoose: choose ?? (_) {},
  formatTime: (value) => value.toIso8601String(),
  timeZoneLabel: 'UTC',
);

void main() {
  testWidgets('no accepted panel disables Read while an already busy read can still be cancelled', (tester) async {
    var reads = 0, cancels = 0;
    await pump(tester, view(enabled: false, read: () => reads++));
    await tester.tap(find.byKey(const ValueKey('job-timeline-read')));
    expect(reads, 0);
    await pump(tester, view(enabled: false, busy: true, cancel: () => cancels++));
    await tester.tap(find.byKey(const ValueKey('job-timeline-cancel')));
    expect(cancels, 1);
  });
  testWidgets('view never fetches automatically and read/cancel are explicit callbacks', (tester) async {
    var reads = 0, cancels = 0;
    await pump(tester, view(read: () => reads++));
    await tester.pump(const Duration(seconds: 3));
    expect(reads, 0);
    await tester.tap(find.byKey(const ValueKey('job-timeline-read')));
    expect(reads, 1);
    await pump(tester, view(busy: true, cancel: () => cancels++));
    await tester.tap(find.byKey(const ValueKey('job-timeline-cancel')));
    expect(cancels, 1);
    expect(reads, 1);
    expect(tester.takeException(), isNull);
  });

  testWidgets('only metadata is rendered; whole-job outcome and unknown execution/skew are explicit', (tester) async {
    final f = Fixture();
    addTearDown(f.flow.dispose);
    expect(await f.flow.load(), isTrue);
    await pump(tester, view(snapshot: f.flow.snapshot));
    await tester.tap(find.byKey(ValueKey('job-timeline-row-${id(100)}')));
    await tester.pumpAndSettle();
    expect(find.textContaining('whole job'), findsOneWidget);
    expect(find.textContaining('Execution start and finish are unknown'), findsOneWidget);
    expect(find.textContaining('Initiator: not recorded'), findsOneWidget);
    expect(find.textContaining('Clock alignment is unknown'), findsOneWidget);
    expect(find.textContaining('Application admission time:'), findsOneWidget);
    expect(find.textContaining('secret-sentinel'), findsNothing);
    expect(find.textContaining('never-render.invalid'), findsNothing);
    expect(find.textContaining(digest), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('old captured row callback is rejected after actual accepted history revision advances', (tester) async {
    final f = Fixture();
    addTearDown(f.flow.dispose);
    expect(await f.flow.load(), isTrue);
    final old = f.flow.snapshot!;
    JobMetricTimelineChoice? choice;
    await pump(tester, view(snapshot: old, choose: (row) => choice = f.flow.choose(old, row)));
    await tester.tap(find.byKey(ValueKey('job-timeline-row-${id(100)}')));
    await tester.pumpAndSettle();
    final oldCallback = tester.widget<TextButton>(find.byKey(ValueKey('job-timeline-choose-${id(100)}'))).onPressed!;
    f.revision = 2;
    expect(await f.flow.load(), isTrue);
    final current = f.flow.snapshot!;
    await pump(tester, view(snapshot: current, choose: (row) => choice = f.flow.choose(current, row)));
    oldCallback();
    expect(choice, isNull);
    expect(current.jobs.markers.single.notificationRevision, 2);
    f.current = false;
    expect(f.flow.choose(current, current.jobs.markers.single), isNull);
    expect(tester.takeException(), isNull);
  });

  testWidgets('history projection stays bounded to128 rows without hidden expansion', (tester) async {
    final f = Fixture(count: 129);
    addTearDown(f.flow.dispose);
    expect(await f.flow.load(), isTrue);
    final snapshot = f.flow.snapshot!;
    expect(snapshot.jobs.markers.length, 128);
    await pump(tester, view(snapshot: snapshot));
    final list = tester.widget<ListView>(find.byKey(const ValueKey('job-timeline-rows')));
    expect(list.childrenDelegate.estimatedChildCount, 128);
    expect(find.textContaining('latest 128 matching jobs'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('legacy revision-zero row displays history but disables context choice', (tester) async {
    final f = Fixture(revision: 0);
    addTearDown(f.flow.dispose);
    expect(await f.flow.load(), isTrue);
    await pump(tester, view(snapshot: f.flow.snapshot));
    await tester.tap(find.byKey(ValueKey('job-timeline-row-${id(100)}')));
    await tester.pumpAndSettle();
    expect(tester.widget<TextButton>(find.byKey(ValueKey('job-timeline-choose-${id(100)}'))).onPressed, isNull);
    expect(tester.takeException(), isNull);
  });

  testWidgets('Russian narrow large-font layout preserves unknown timing and explicit read-only choice', (
    tester,
  ) async {
    final f = Fixture();
    addTearDown(f.flow.dispose);
    expect(await f.flow.load(), isTrue);
    await pump(
      tester,
      view(snapshot: f.flow.snapshot),
      locale: const Locale('ru'),
      size: const Size(420, 1100),
      scale: 1.8,
    );
    await tester.ensureVisible(find.byKey(ValueKey('job-timeline-row-${id(100)}')));
    await tester.tap(find.byKey(ValueKey('job-timeline-row-${id(100)}')));
    await tester.pumpAndSettle();
    expect(find.textContaining('Инициатор в этой истории не записан'), findsOneWidget);
    expect(find.textContaining('Начало и завершение выполнения неизвестны'), findsOneWidget);
    expect(find.textContaining('Просмотреть контекст задания'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });
}
