import 'dart:async';

import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/monitoring/job_metric_correlation.dart';
import 'package:consolecrypt/monitoring/monitoring_bindings.dart';
import 'package:consolecrypt/monitoring/monitoring_exporter_dialog.dart';
import 'package:consolecrypt/monitoring/monitoring_exporter_review.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/monitoring_binding_fakes.dart';
import '../helpers/test_app.dart' show settle;

String id(int n) => '00000000-0000-4000-8000-${n.toString().padLeft(12, '0')}';
Json metadata() => {
  'kind': 'exporterContext',
  'version': 1,
  'profileId': id(1),
  'notificationScopeId': id(2),
  'hostId': id(3),
  'hostRevision': 4,
  'protocol': 'ssh',
  'sourceId': id(5),
  'sourceRevision': 6,
  'jobId': id(7),
  'jobRevision': 8,
  'projectId': id(9),
  'revisionDigest': 'a' * 64,
};
final source = MonitoringSource(id: id(5), name: 'Owned metrics', endpoint: 'https://fixture.invalid', revision: 6);

class Jobs implements AutomationJobService, AutomationService {
  int reads = 0;
  @override
  Future<AutomationJson> jobs() async {
    reads++;
    return {
      'jobs': [
        {
          'job_id': id(7),
          'notification_revision': 8,
          'state': 'succeeded',
          'host_ids': [id(3)],
        },
      ],
    };
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => throw StateError('execution must not be called');
}

class Reader implements MonitoringExporterReader, MonitoringBindingReader {
  @override
  dynamic noSuchMethod(Invocation invocation) => throw StateError('panel read is explicit only');
  @override
  void cancelBinding() {}
  int captures = 0, verifications = 0, cancels = 0;
  Completer<MonitoringExporterObservation>? held;
  bool unavailable = false;
  @override
  Future<MonitoringExporterContext> exporterContext(
    MonitoringSource source, {
    required String profileId,
    required String hostId,
    required String jobId,
    required int jobRevision,
  }) async {
    captures++;
    return MonitoringExporterContext.fromJson(metadata());
  }

  @override
  Future<MonitoringExporterObservation> verifyExporter(
    MonitoringSource source,
    MonitoringExporterContext original,
    MonitoringExporterKind exporter,
    Map<String, String> selector, {
    int maxAgeSeconds = 120,
  }) async {
    verifications++;
    expect(selector, {'job': 'owned', 'instance': 'reviewed:9100'});
    if (held != null) return held!.future;
    return result(original, unavailable: unavailable);
  }

  MonitoringExporterObservation result(MonitoringExporterContext original, {bool unavailable = false}) =>
      MonitoringExporterObservation.fromJson(
        {
          ...metadata(),
          'kind': 'exporterVerification',
          'exporter': 'nodeExporter',
          'observation': unavailable
              ? {'state': 'unavailable', 'reason': 'staleSeries'}
              : {'state': 'observed', 'maxSampleAgeSeconds': 1.0, 'evaluatedAtSourceSeconds': 1700000000.0},
        },
        original,
        MonitoringExporterKind.nodeExporter,
        120,
      );
  @override
  void cancelExporter() {
    cancels++;
  }
}

class Store extends MonitoringBindingTestStore implements MonitoringExporterBindingStore {
  Store() : super(initial: [source]);
  final saved = <MonitoringBinding>[];
  final origins = <MonitoringExporterContext>[];
  @override
  Future<MonitoringBinding> saveExporterBinding(MonitoringBinding binding, MonitoringExporterContext original) async {
    saved.add(binding);
    origins.add(original);
    final result = MonitoringBinding(
      id: id(20),
      name: binding.name,
      hostId: binding.hostId,
      sourceId: binding.sourceId,
      revision: 1,
      sourceRevision: binding.sourceRevision,
      selector: binding.selector,
      state: MonitoringBindingState.current,
    );
    this.bindings = [...this.bindings, result];
    return result;
  }
}

Future<void> open(
  WidgetTester tester,
  Jobs jobs,
  Reader reader,
  Store store, {
  bool Function()? current,
  Locale locale = const Locale('en'),
}) async {
  await tester.binding.setSurfaceSize(const Size(1100, 1000));
  addTearDown(() => tester.binding.setSurfaceSize(null));
  await tester.pumpWidget(
    MaterialApp(
      theme: AppTheme.dark(platform: TargetPlatform.windows),
      locale: locale,
      supportedLocales: AppLocalizations.supportedLocales,
      localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
      home: Scaffold(
        body: Center(
          child: MonitoringExporterDialog(
            profileId: id(1),
            source: source,
            reader: reader,
            store: store,
            jobs: jobs,
            hosts: [MonitoringHostRef(id: id(3), name: 'Owned host')],
            isCurrent: current ?? () => true,
            onSaved: (_) {},
          ),
        ),
      ),
    ),
  );
  await settle(tester);
}

Future<void> choose(WidgetTester tester) async {
  await tester.tap(find.byKey(const ValueKey('exporter-host')));
  await settle(tester);
  await tester.tap(find.text('Owned host').last);
  await settle(tester);
  await tester.tap(find.byKey(const ValueKey('exporter-load-jobs')));
  await settle(tester);
  await tester.tap(find.byKey(ValueKey('exporter-job-${id(3)}-1')));
  await settle(tester);
  await tester.tap(find.text(id(7)).last);
  await settle(tester);
  await tester.enterText(find.byKey(const ValueKey('exporter-name')), 'Owned binding');
  await tester.enterText(find.byKey(const ValueKey('exporter-job-label')), 'owned');
  await tester.enterText(find.byKey(const ValueKey('exporter-instance')), 'reviewed:9100');
  await settle(tester);
}

Future<void> bindings(WidgetTester tester, Widget Function(StateSetter) build) async {
  await tester.binding.setSurfaceSize(const Size(1100, 1000));
  addTearDown(() => tester.binding.setSurfaceSize(null));
  await tester.pumpWidget(
    MaterialApp(
      theme: AppTheme.dark(platform: TargetPlatform.windows),
      supportedLocales: AppLocalizations.supportedLocales,
      localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
      home: Scaffold(body: StatefulBuilder(builder: (context, set) => build(set))),
    ),
  );
  await settle(tester);
}

void main() {
  testWidgets('opening is passive; explicit verification then separate save repeats original observation', (
    tester,
  ) async {
    final jobs = Jobs(), reader = Reader(), store = Store();
    await open(tester, jobs, reader, store);
    expect(jobs.reads, 0);
    expect(reader.captures, 0);
    expect(reader.verifications, 0);
    expect(store.saved, isEmpty);
    await choose(tester);
    expect(jobs.reads, 1);
    expect(reader.captures, 0);
    await tester.tap(find.byKey(const ValueKey('exporter-verify')));
    await settle(tester);
    expect(reader.captures, 1);
    expect(reader.verifications, 1);
    expect(store.saved, isEmpty);
    await tester.tap(find.byKey(const ValueKey('exporter-save')));
    await settle(tester);
    expect(reader.captures, 1);
    expect(reader.verifications, 2);
    expect(store.saved.single.hostId, id(3));
    expect(store.origins.single.bindingOrigin['notificationScopeId'], id(2));
    expect(store.saved.single.selector, {'job': 'owned', 'instance': 'reviewed:9100'});
    expect(tester.takeException(), isNull);
  });
  testWidgets('unavailable fresh-series result cannot save or invent binding', (tester) async {
    final jobs = Jobs(), reader = Reader()..unavailable = true, store = Store();
    await open(tester, jobs, reader, store);
    await choose(tester);
    await tester.tap(find.byKey(const ValueKey('exporter-verify')));
    await settle(tester);
    expect(tester.widget<GlassButton>(find.byKey(const ValueKey('exporter-save'))).onPressed, isNull);
    expect(store.saved, isEmpty);
    expect(reader.verifications, 1);
  });
  testWidgets('original profile boundary suppresses a pending successful result', (tester) async {
    var current = true;
    final jobs = Jobs(), reader = Reader()..held = Completer(), store = Store();
    await open(tester, jobs, reader, store, current: () => current);
    await choose(tester);
    await tester.tap(find.byKey(const ValueKey('exporter-verify')));
    await tester.pump();
    expect(reader.verifications, 1);
    current = false;
    reader.held!.complete(reader.result(MonitoringExporterContext.fromJson(metadata())));
    await settle(tester);
    expect(store.saved, isEmpty);
    expect(tester.widget<GlassButton>(find.byKey(const ValueKey('exporter-save'))).onPressed, isNull);
  });
  testWidgets('editing selectors invalidates the previous observation and original frame', (tester) async {
    final jobs = Jobs(), reader = Reader(), store = Store();
    await open(tester, jobs, reader, store);
    await choose(tester);
    await tester.tap(find.byKey(const ValueKey('exporter-verify')));
    await settle(tester);
    await tester.enterText(find.byKey(const ValueKey('exporter-instance')), 'new-reviewed:9100');
    await settle(tester);
    expect(tester.widget<GlassButton>(find.byKey(const ValueKey('exporter-save'))).onPressed, isNull);
    expect(store.saved, isEmpty);
  });

  testWidgets('saved exporter binding offers a distinct Open panel without reading it automatically', (tester) async {
    final jobs = Jobs(), reader = Reader(), store = Store(), scope = JobCorrelationScope(id(1));
    await bindings(
      tester,
      (set) => MonitoringBindings(
        store: store,
        sourceStore: store,
        source: source,
        sourceIds: {source.id},
        hosts: [MonitoringHostRef(id: id(3), name: 'Owned host')],
        reader: reader,
        correlationScope: scope,
        correlationCurrent: () => true,
        correlationAutomation: jobs,
        onChanged: () async {},
      ),
    );
    await tester.tap(find.byKey(const ValueKey('monitoring-exporter-review')));
    await settle(tester);
    await choose(tester);
    await tester.tap(find.byKey(const ValueKey('exporter-verify')));
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('exporter-save')));
    await settle(tester);
    expect(store.saved.length, 1);
    expect(find.byType(MonitoringExporterDialog), findsNothing);
    expect(find.byKey(const ValueKey('monitoring-binding-reader')), findsNothing);
    expect(find.text('Open panel'), findsOneWidget);
    await tester.tap(find.byKey(ValueKey('monitoring-binding-inspect-${id(20)}')));
    await settle(tester);
    expect(find.byKey(const ValueKey('monitoring-binding-reader')), findsOneWidget);
    expect(jobs.reads, 1);
    expect(reader.verifications, 2);
    expect(tester.takeException(), isNull);
  });

  for (final change in ['jobs', 'readerAndStore']) {
    testWidgets('same-key $change replacement retires old dialog and restores binding actions', (tester) async {
      var jobs = Jobs(), reader = Reader()..held = Completer<MonitoringExporterObservation>(), store = Store();
      final oldReader = reader, oldStore = store, scope = JobCorrelationScope(id(1));
      late StateSetter rebuild;
      await bindings(tester, (set) {
        rebuild = set;
        return MonitoringBindings(
          key: const ValueKey('fixed-bindings'),
          store: store,
          sourceStore: store,
          source: source,
          sourceIds: {source.id},
          hosts: [MonitoringHostRef(id: id(3), name: 'Owned host')],
          reader: reader,
          correlationScope: scope,
          correlationCurrent: () => true,
          correlationAutomation: jobs,
          onChanged: () async {},
        );
      });
      await tester.tap(find.byKey(const ValueKey('monitoring-exporter-review')));
      await settle(tester);
      await choose(tester);
      await tester.tap(find.byKey(const ValueKey('exporter-verify')));
      await tester.pump();
      expect(oldReader.verifications, 1);
      rebuild(() {
        if (change == 'jobs') {
          jobs = Jobs();
        } else {
          reader = Reader();
          store = Store();
        }
      });
      await settle(tester);
      expect(find.byType(MonitoringExporterDialog), findsNothing);
      final button = tester.widget<TextButton>(find.byKey(const ValueKey('monitoring-exporter-review')));
      expect(button.onPressed, isNotNull);
      oldReader.held!.complete(oldReader.result(MonitoringExporterContext.fromJson(metadata())));
      await settle(tester);
      expect(oldStore.saved, isEmpty);
      expect(store.saved, isEmpty);
      expect(tester.takeException(), isNull);
    });
  }
}
