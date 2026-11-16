import 'dart:async';

import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/monitoring/job_context_focus.dart';
import 'package:consolecrypt/monitoring/job_context_navigation.dart';
import 'package:consolecrypt/monitoring/job_metric_correlation.dart';
import 'package:consolecrypt/monitoring/job_metrics_chooser.dart';
import 'package:consolecrypt/monitoring/monitoring_host_context.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_ui_proof.dart';
import '../helpers/monitoring_binding_fakes.dart';
import '../helpers/monitoring_binding_reads_fakes.dart';
import '../helpers/monitoring_fakes.dart';
import '../helpers/test_app.dart' show settle, tapKey;
import '../unit/job_metric_correlation_test.dart' as f;

class Reader extends MonitoringBindingReadTestService
    implements MonitoringHostContextReader {
  final captures = <JobContextMetadata>[], checks = <JobContextMetadata>[];
  Completer<JobContextMetadata>? pendingCapture, pendingCheck;
  @override
  bool get hostContextSupported => true;
  @override
  Future<JobContextMetadata> captureHostContext({
    required String profileId,
    required MonitoringSource source,
    required MonitoringBinding binding,
  }) async {
    final metadata = JobContextMetadata.parse({
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
    captures.add(metadata);
    return pendingCapture?.future ?? metadata;
  }

  @override
  Future<JobContextMetadata> recheckHostContext(
    JobContextMetadata original,
  ) async {
    checks.add(original);
    return pendingCheck?.future ?? original;
  }
}

class Jobs implements AutomationJobService {
  int revision = 3, reads = 0;
  String digest = f.digest;
  @override
  Future<AutomationJson> jobs() async {
    reads++;
    return f.catalog([
      f.row(hosts: [bindingHost.id], state: 'failed', revision: revision)
        ..['manifest_digest'] = digest,
    ]);
  }

  @override
  dynamic noSuchMethod(Invocation invocation) =>
      throw StateError('No execution/cancel/install from metric navigation');
}

final scope = JobCorrelationScope(f.profile);

Future<void> pump(
  WidgetTester tester,
  Reader reader,
  Jobs jobs,
  MonitoringBindingTestStore store, {
  bool Function()? current,
  JobCorrelationScope? sessionScope,
  ValueChanged<JobContextFocus>? chosen,
  Locale locale = const Locale('en'),
  double width = 1000,
}) async {
  await tester.binding.setSurfaceSize(Size(width, 1000));
  addTearDown(() => tester.binding.setSurfaceSize(null));
  await loadAutomationProofFonts(tester);
  await tester.pumpWidget(
    MaterialApp(
      locale: locale,
      theme: AppTheme.dark(platform: TargetPlatform.windows),
      supportedLocales: AppLocalizations.supportedLocales,
      localizationsDelegates: const [
        AppLocalizations.delegate,
        ...GlobalMaterialLocalizations.delegates,
      ],
      builder: (_, child) => RepaintBoundary(
        key: const ValueKey('automation-proof-root'),
        child: child!,
      ),
      home: Scaffold(
        body: JobMetricsChooser(
          job: f.row(hosts: [bindingHost.id], state: 'failed', revision: 3),
          profileId: f.profile,
          scope: sessionScope ?? scope,
          sourceStore: store,
          bindings: store,
          reader: reader,
          jobs: jobs,
          hosts: const [bindingHost, otherBindingHost],
          isCurrent: current ?? () => true,
          onChoose: chosen ?? (_) {},
          onCancel: () {},
        ),
      ),
    ),
  );
  await settle(tester);
}

Future<void> choose(WidgetTester tester) async {
  tester
      .widget<GlassSelect<String?>>(
        find.byKey(const ValueKey('job-metrics-host')),
      )
      .onChanged!(bindingHost.id);
  await tester.pump();
  tester
      .widget<GlassSelect<String?>>(
        find.byKey(const ValueKey('job-metrics-source')),
      )
      .onChanged!(monitoringFixture.id);
  await tester.pump();
  tester
      .widget<GlassSelect<String?>>(
        find.byKey(const ValueKey('job-metrics-binding')),
      )
      .onChanged!(bindingFixture().id);
  await settle(tester);
}

void main() {
  testWidgets(
    'opening lists saved metadata only; explicit exact choices capture/check original context then propose windowless focus',
    (tester) async {
      final reader = Reader(),
          jobs = Jobs(),
          store = MonitoringBindingTestStore(rows: [bindingFixture()]);
      JobContextFocus? focus;
      await pump(tester, reader, jobs, store, chosen: (value) => focus = value);
      expect(reader.captures, isEmpty);
      expect(reader.panels, isEmpty);
      expect(reader.queries, 0);
      final hostItems = tester
          .widget<GlassSelect<String?>>(
            find.byKey(const ValueKey('job-metrics-host')),
          )
          .items;
      expect(hostItems.map((item) => item.value), [null, bindingHost.id]);
      expect(
        tester
            .widget<GlassButton>(find.byKey(const ValueKey('job-metrics-open')))
            .onPressed,
        isNull,
      );
      await choose(tester);
      expect(reader.captures, isEmpty);
      await tapKey(tester, 'job-metrics-open');
      expect(focus, isNotNull);
      expect(focus!.metadataOnly, isTrue);
      expect(focus!.originalContext, isNull);
      expect(focus!.metadata.bindingId, bindingFixture().id);
      expect(reader.checks.single, same(reader.captures.single));
      expect(jobs.reads, 1);
      expect(reader.panels, isEmpty);
      expect(reader.queries, 0);
      expect(store.saves, isEmpty);
    },
  );
  testWidgets(
    'stale binding or injected unapproved host never creates an automatic mapping or capture',
    (tester) async {
      final reader = Reader(),
          jobs = Jobs(),
          store = MonitoringBindingTestStore(
            rows: [bindingFixture(sourceRevision: 2)],
          );
      await pump(tester, reader, jobs, store);
      await choose(tester);
      expect(
        tester
            .widget<GlassButton>(find.byKey(const ValueKey('job-metrics-open')))
            .onPressed,
        isNull,
      );
      expect(reader.captures, isEmpty);
      expect(store.saves, isEmpty);
    },
  );
  testWidgets(
    'late initial capture cannot adopt replacement profile/session scope',
    (tester) async {
      final reader = Reader()..pendingCapture = Completer<JobContextMetadata>(),
          jobs = Jobs();
      final store = MonitoringBindingTestStore(rows: [bindingFixture()]);
      JobContextFocus? focus;
      var current = true;
      await pump(
        tester,
        reader,
        jobs,
        store,
        current: () => current,
        chosen: (value) => focus = value,
      );
      await choose(tester);
      await tester.tap(find.byKey(const ValueKey('job-metrics-open')));
      await tester.pump();
      expect(reader.captures, hasLength(1));
      current = false;
      reader.pendingCapture!.complete(reader.captures.single);
      await settle(tester);
      expect(focus, isNull);
      expect(jobs.reads, 0);
      expect(reader.checks, isEmpty);
      expect(reader.panels, isEmpty);
    },
  );
  testWidgets(
    'fresh job revision/digest changes reject the pending handoff rather than selecting latest job facts',
    (tester) async {
      final reader = Reader(), jobs = Jobs()..revision = 4;
      final store = MonitoringBindingTestStore(rows: [bindingFixture()]);
      JobContextFocus? focus;
      await pump(tester, reader, jobs, store, chosen: (value) => focus = value);
      await choose(tester);
      await tapKey(tester, 'job-metrics-open');
      expect(focus, isNull);
      expect(reader.checks, isEmpty);
      expect(reader.queries, 0);
    },
  );
  testWidgets(
    'service replacement during original recheck denies late focus and never opens a metric transport',
    (tester) async {
      final reader = Reader()..pendingCheck = Completer<JobContextMetadata>(),
          jobs = Jobs();
      final store = MonitoringBindingTestStore(rows: [bindingFixture()]);
      JobContextFocus? focus;
      await pump(tester, reader, jobs, store, chosen: (value) => focus = value);
      await choose(tester);
      await tester.tap(find.byKey(const ValueKey('job-metrics-open')));
      await tester.pump();
      expect(reader.checks, hasLength(1));
      await pump(
        tester,
        Reader(),
        jobs,
        store,
        chosen: (value) => focus = value,
      );
      reader.pendingCheck!.complete(reader.captures.single);
      await settle(tester);
      expect(focus, isNull);
      expect(reader.panels, isEmpty);
    },
  );
  for (final width in [460.0, 1400.0]) {
    testWidgets(
      'RU explicit saved binding choices fit width=$width without any automatic query',
      (tester) async {
        final reader = Reader(),
            jobs = Jobs(),
            store = MonitoringBindingTestStore(rows: [bindingFixture()]);
        await pump(
          tester,
          reader,
          jobs,
          store,
          locale: const Locale('ru'),
          width: width,
        );
        await choose(tester);
        expect(reader.captures, isEmpty);
        expect(reader.queries, 0);
        expect(tester.takeException(), isNull);
        await captureAutomationWorkspace(
          tester,
          'cor02-job-metrics-${width < 600 ? 'narrow' : 'wide'}-ru',
        );
      },
    );
  }
}
