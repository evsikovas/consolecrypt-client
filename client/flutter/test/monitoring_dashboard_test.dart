import 'dart:async';

import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/monitoring/monitoring_binding_models.dart';
import 'package:consolecrypt/monitoring/monitoring_dashboard.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_result_view.dart';
import 'package:consolecrypt/monitoring/monitoring_saved_panel_models.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import 'helpers/monitoring_binding_fakes.dart';
import 'helpers/monitoring_binding_reads_fakes.dart';
import 'helpers/monitoring_fakes.dart';
import 'helpers/monitoring_saved_panel_fakes.dart';

const _otherSource = MonitoringSource(
  id: '22222222-2222-4222-8222-222222222222',
  name: 'Second host',
  endpoint: 'http://127.0.0.1:9091',
  revision: 1,
);
const _otherPanelId = '44444444-4444-4444-8444-444444444444';

SavedMonitoringPanel _panelFor(MonitoringSource source, {String id = savedPanelId}) => SavedMonitoringPanel(
  id: id,
  revision: 1,
  state: SavedPanelState.current,
  definition: SavedPanelDefinition(
    title: 'Graph for ${source.name}',
    sourceId: source.id,
    sourceRevision: source.revision,
    query: 'node_load1',
    unitHint: SavedPanelUnitHint.count,
  ),
);

SavedPanelRead _sample(SavedPanelRange range, double value) => SavedPanelRead(
  QueryOutcome(
    status: 'success',
    httpStatus: 200,
    effectiveStepSeconds: range.stepSeconds,
    data: QueryData(
      'matrix',
      series: [
        MonitoringSeries(
          {'instance': 'saved-host'},
          [
            MonitoringSample(range.start, SampleKind.finite, value),
            MonitoringSample(range.end, SampleKind.finite, value + 1),
          ],
        ),
      ],
    ),
  ),
  SavedPanelUnitHint.count,
);

class _Service extends MonitoringTestService implements MonitoringSavedPanelReader {
  final reads = <(MonitoringSource, SavedMonitoringPanel, SavedPanelRange)>[];
  Future<SavedPanelRead> Function(MonitoringSource, SavedMonitoringPanel, SavedPanelRange)? respond;
  @override
  Future<SavedPanelRead> readSavedPanel(
    MonitoringSource source,
    SavedMonitoringPanel panel, {
    SavedPanelRange? window,
  }) {
    expect(window, isNotNull, reason: 'A dashboard must request historical samples for its graph');
    reads.add((source, panel, window!));
    return respond?.call(source, panel, window) ?? Future.value(_sample(window, 42));
  }
}

class _BindingService extends MonitoringTestService implements MonitoringBindingReader {
  final reads = <(MonitoringSource, MonitoringBinding, MonitoringPanel, SavedPanelRange)>[];
  Future<BindingReadResult> Function(MonitoringSource, MonitoringBinding, MonitoringPanel, SavedPanelRange)? respond;
  BindingReadResult samples(
    MonitoringBinding binding,
    MonitoringPanel panel,
    SavedPanelRange range, {
    double value = 42,
  }) {
    final json = bindingPanelJson(panel: panel);
    json['bindingId'] = binding.id;
    json['bindingRevision'] = binding.revision;
    final plan = (json['preparation'] as Json)['plan'] as Json;
    plan['sourceId'] = binding.sourceId;
    plan['hostId'] = binding.hostId;
    plan['bindingRevision'] = binding.revision;
    plan['start'] = range.start;
    plan['end'] = range.end;
    plan['stepSeconds'] = range.stepSeconds;
    for (final query in json['queries'] as List<Json>) {
      final outcome = query['outcome'] as Json;
      final series = (outcome['data'] as Json)['series'] as List;
      (series.single as Json)['samples'] = [
        {
          'timestamp': range.start,
          'value': {'kind': 'finite', 'value': value},
        },
        {
          'timestamp': range.end,
          'value': {'kind': 'finite', 'value': value + 1},
        },
      ];
    }
    return BindingReadResult.fromJson(json);
  }

  @override
  Future<BindingReadResult> readPanel(
    MonitoringSource source,
    MonitoringBinding binding,
    MonitoringPanel panel, {
    required double start,
    required double end,
    required double stepSeconds,
  }) {
    final range = SavedPanelRange(start: start, end: end, stepSeconds: stepSeconds);
    reads.add((source, binding, panel, range));
    return respond?.call(source, binding, panel, range) ?? Future.value(samples(binding, panel, range));
  }

  @override
  Future<BindingReadResult> discover(MonitoringSource source, MonitoringBinding binding, double time) =>
      throw StateError('No manual discovery step');
  @override
  void cancelBinding() {}
}

class _FocusedStore extends SavedPanelTestStore implements MonitoringBindingStore {
  @override
  final List<MonitoringBinding> bindings = [];
  @override
  Future<MonitoringBinding> saveBinding(MonitoringBinding binding, {bool create = false}) =>
      throw StateError('Read-only dashboard');
  @override
  Future<void> deleteBinding(MonitoringBinding binding) => throw StateError('Read-only dashboard');
}

class _FocusedService extends _BindingService implements MonitoringSavedPanelReader {
  final savedReads = <(MonitoringSource, SavedMonitoringPanel)>[];
  @override
  Future<SavedPanelRead> readSavedPanel(
    MonitoringSource source,
    SavedMonitoringPanel panel, {
    SavedPanelRange? window,
  }) async {
    savedReads.add((source, panel));
    return _sample(window!, 777);
  }
}

MonitoringBinding _focusBinding(String id, String name, String hostId, {String? sourceId}) => MonitoringBinding(
  id: id,
  name: name,
  hostId: hostId,
  sourceId: sourceId ?? monitoringFixture.id,
  revision: 1,
  sourceRevision: 1,
  selector: {'job': name, 'instance': '127.0.0.1:9100'},
  state: MonitoringBindingState.current,
);

Widget _app(
  MonitoringService service,
  MonitoringSourceStore store, {
  required bool Function() isCurrent,
  String? requestedSourceId,
  String? requestedHostId,
  List<String> requestedBindingIds = const [],
  bool active = true,
  bool keepHostSelection = false,
}) => MaterialApp(
  theme: AppTheme.dark(platform: TargetPlatform.windows),
  locale: const Locale('en'),
  supportedLocales: AppLocalizations.supportedLocales,
  localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
  home: Scaffold(
    body: MonitoringDashboard(
      service: service,
      store: store,
      isCurrent: isCurrent,
      requestedSourceId: requestedSourceId,
      requestedHostId: requestedHostId,
      requestedBindingIds: requestedBindingIds,
      active: active,
      keepHostSelection: keepHostSelection,
    ),
  ),
);

Future<void> _pump(WidgetTester tester) async {
  for (var i = 0; i < 8; i++) {
    await tester.pump(const Duration(milliseconds: 30));
  }
}

Future<void> _open(
  WidgetTester tester,
  MonitoringService service,
  MonitoringSourceStore store, {
  bool Function()? isCurrent,
  String? requestedSourceId,
  String? requestedHostId,
  List<String> requestedBindingIds = const [],
  bool keepHostSelection = false,
}) async {
  await tester.binding.setSurfaceSize(const Size(1200, 1000));
  addTearDown(() async {
    await tester.pumpWidget(const SizedBox());
    await tester.binding.setSurfaceSize(null);
  });
  await tester.pumpWidget(
    _app(
      service,
      store,
      isCurrent: isCurrent ?? () => true,
      requestedSourceId: requestedSourceId,
      requestedHostId: requestedHostId,
      requestedBindingIds: requestedBindingIds,
      keepHostSelection: keepHostSelection,
    ),
  );
  await _pump(tester);
}

void main() {
  testWidgets('a host view keeps its graphs when an unrelated source is requested', (tester) async {
    const hostId = '11111111-1111-4111-8111-111111111111';
    final store = _FocusedStore()..sources = [monitoringFixture, _otherSource];
    store.bindings.addAll([
      _focusBinding('33333333-3333-4333-8333-333333333333', 'First source server', hostId),
      _focusBinding(
        '66666666-6666-4666-8666-666666666666',
        'Other host',
        '55555555-5555-4555-8555-555555555555',
        sourceId: _otherSource.id,
      ),
    ]);
    final service = _FocusedService();
    await _open(
      tester,
      service,
      store,
      requestedHostId: hostId,
      requestedSourceId: monitoringFixture.id,
      keepHostSelection: true,
    );
    expect(service.reads, hasLength(4));
    tester
        .widget<DropdownButtonFormField<String>>(find.byKey(const ValueKey('monitoring-dashboard-source')))
        .onChanged!(_otherSource.id);
    await _pump(tester);
    expect(service.reads, hasLength(4));
    expect(service.savedReads, isEmpty);
    expect(find.text('Other host'), findsNothing);
    expect(find.byType(MonitoringResultView), findsNWidgets(8));
    expect(tester.takeException(), isNull);
  });

  testWidgets('host shortcut chooses its matching source and never reads unrelated host graphs', (tester) async {
    const hostId = '11111111-1111-4111-8111-111111111111';
    final store = _FocusedStore()
      ..sources = [monitoringFixture, _otherSource]
      ..panels = [_panelFor(monitoringFixture), _panelFor(_otherSource, id: _otherPanelId)];
    final unrelated = _focusBinding(
      '33333333-3333-4333-8333-333333333333',
      'Unrelated server',
      '55555555-5555-4555-8555-555555555555',
    );
    final chosen = _focusBinding(
      '66666666-6666-4666-8666-666666666666',
      'Chosen server',
      hostId,
      sourceId: _otherSource.id,
    );
    store.bindings.addAll([unrelated, chosen]);
    final service = _FocusedService();
    await _open(tester, service, store, requestedHostId: hostId);
    expect(service.reads, hasLength(4));
    expect(service.reads.every((r) => r.$1.id == _otherSource.id && r.$2.id == chosen.id), isTrue);
    expect(service.savedReads, isEmpty);
    expect(find.text(unrelated.name), findsNothing);
    expect(find.byType(MonitoringResultView), findsNWidgets(8));
    expect(tester.takeException(), isNull);
  });

  testWidgets('host with no binding never falls back to another host or arbitrary saved panels', (tester) async {
    final store = _FocusedStore()..panels = [_panelFor(monitoringFixture)];
    store.bindings.add(
      _focusBinding('33333333-3333-4333-8333-333333333333', 'Unrelated server', '55555555-5555-4555-8555-555555555555'),
    );
    final service = _FocusedService();
    await _open(tester, service, store, requestedHostId: '11111111-1111-4111-8111-111111111111');
    expect(service.reads, isEmpty);
    expect(service.savedReads, isEmpty);
    expect(find.byType(MonitoringResultView), findsNothing);
    expect(find.text('Unrelated server'), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('saved graphs immediately request a one-hour range and render actual returned samples', (tester) async {
    final service = _Service(), store = SavedPanelTestStore();
    await _open(tester, service, store);
    expect(service.reads, hasLength(1));
    final (source, panel, range) = service.reads.single;
    expect(source.fingerprint, monitoringFixture.fingerprint);
    expect(panel.id, savedPanelId);
    expect(range.end - range.start, 3600);
    expect(range.stepSeconds, 15);
    final chart = tester.widget<MonitoringResultView>(find.byKey(const ValueKey('dashboard-chart-$savedPanelId')));
    expect(chart.result.data!.series.single.samples.map((p) => p.value), [42, 43]);
    expect(chart.requestedWindow!.startSeconds, range.start);
    expect(chart.requestedWindow!.endSeconds, range.end);
    expect(service.queries, 0);
    expect(store.saves, isEmpty);
    expect(tester.takeException(), isNull);
  });

  testWidgets('default selection chooses a source that actually has saved graphs', (tester) async {
    final service = _Service(), store = SavedPanelTestStore()..sources = [_otherSource, monitoringFixture];
    await _open(tester, service, store);
    expect(service.reads.single.$1.id, monitoringFixture.id);
    expect(find.byKey(const ValueKey('dashboard-chart-$savedPanelId')), findsOneWidget);
  });

  for (final emptyCatalog in [false, true]) {
    testWidgets('no saved panels has a truthful empty state and never reads emptyCatalog=$emptyCatalog', (
      tester,
    ) async {
      final service = _Service(), store = SavedPanelTestStore()..panels = [];
      if (emptyCatalog) store.sources = [];
      await _open(tester, service, store);
      final l = tester.element(find.byType(MonitoringDashboard)).l10n;
      expect(find.text(l.monitoringSetupNoGraphs), findsOneWidget);
      expect(find.byType(MonitoringResultView), findsNothing);
      expect(service.reads, isEmpty);
      expect(service.queries, 0);
      expect(find.byType(LinearProgressIndicator), findsNothing);
    });
  }

  testWidgets('saved panels for a stale source revision are never queried as current graphs', (tester) async {
    final service = _Service(), store = SavedPanelTestStore()..sources = [monitoringFixture.withRecord(revision: 2)];
    await _open(tester, service, store);
    expect(service.reads, isEmpty);
    expect(find.byType(MonitoringResultView), findsNothing);
    expect(find.text(savedDefinition.title), findsNothing);
  });

  testWidgets('a failed refresh removes previous successful samples and reports a safe error', (tester) async {
    final service = _Service(), store = SavedPanelTestStore();
    await _open(tester, service, store);
    expect(find.byType(MonitoringResultView), findsOneWidget);
    service.respond = (_, _, _) => Future.error(const MonitoringFailure.local(MonitoringErrorCode.transport));
    await tester.pump(const Duration(seconds: 16));
    await _pump(tester);
    expect(service.reads, hasLength(2));
    expect(find.byType(MonitoringResultView), findsNothing);
    expect(find.byType(InfoBanner), findsOneWidget);
    expect(find.text(savedDefinition.title), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('switching hosts discards a late previous-host response while rendering the new host', (tester) async {
    final pending = Completer<SavedPanelRead>(), service = _Service();
    final firstPanel = _panelFor(monitoringFixture), secondPanel = _panelFor(_otherSource, id: _otherPanelId);
    final store = SavedPanelTestStore()
      ..sources = [monitoringFixture, _otherSource]
      ..panels = [firstPanel, secondPanel];
    service.respond = (source, _, range) =>
        source.id == monitoringFixture.id ? pending.future : Future.value(_sample(range, 222));
    await _open(tester, service, store);
    expect(service.reads.single.$1.id, monitoringFixture.id);
    final originalRange = service.reads.single.$3;
    tester
        .widget<DropdownButtonFormField<String>>(find.byKey(const ValueKey('monitoring-dashboard-source')))
        .onChanged!(_otherSource.id);
    await _pump(tester);
    expect(service.reads, hasLength(2));
    expect(service.reads.last.$1.id, _otherSource.id);
    pending.complete(_sample(originalRange, 111));
    await _pump(tester);
    expect(find.byKey(const ValueKey('dashboard-chart-$savedPanelId')), findsNothing);
    final chart = tester.widget<MonitoringResultView>(find.byKey(const ValueKey('dashboard-chart-$_otherPanelId')));
    expect(chart.result.data!.series.single.samples.first.value, 222);
    expect(find.text(firstPanel.definition.title), findsNothing);
    expect(find.text(secondPanel.definition.title), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('requested source takes priority over another host with saved panels', (tester) async {
    final service = _Service();
    final store = SavedPanelTestStore()
      ..sources = [monitoringFixture, _otherSource]
      ..panels = [_panelFor(monitoringFixture), _panelFor(_otherSource, id: _otherPanelId)];
    await _open(tester, service, store, requestedSourceId: _otherSource.id);
    expect(service.reads.single.$1.id, _otherSource.id);
    expect(find.byKey(const ValueKey('dashboard-chart-$_otherPanelId')), findsOneWidget);
    expect(find.byKey(const ValueKey('dashboard-chart-$savedPanelId')), findsNothing);
  });

  testWidgets('an inactive dashboard cannot start its first metrics read', (tester) async {
    final service = _Service(), store = SavedPanelTestStore();
    await _open(tester, service, store, isCurrent: () => false);
    await tester.pump(const Duration(seconds: 31));
    await _pump(tester);
    expect(service.reads, isEmpty);
    expect(find.byType(MonitoringResultView), findsNothing);
  });

  testWidgets('becoming inactive prevents periodic and manual refresh reads', (tester) async {
    var active = true;
    final service = _Service(), store = SavedPanelTestStore();
    await _open(tester, service, store, isCurrent: () => active);
    expect(service.reads, hasLength(1));
    active = false;
    await tester.pump(const Duration(seconds: 31));
    await _pump(tester);
    tester.widget<IconButton>(find.byType(IconButton).first).onPressed!();
    await _pump(tester);
    expect(service.reads, hasLength(1));
  });

  testWidgets('a pending response cannot publish a graph after its original view becomes inactive', (tester) async {
    var active = true;
    final pending = Completer<SavedPanelRead>(), service = _Service(), store = SavedPanelTestStore();
    service.respond = (_, _, _) => pending.future;
    await _open(tester, service, store, isCurrent: () => active);
    final range = service.reads.single.$3;
    active = false;
    pending.complete(_sample(range, 666));
    await _pump(tester);
    expect(find.byType(MonitoringResultView), findsNothing);
    await tester.pump(const Duration(seconds: 31));
    expect(service.reads, hasLength(1));
    expect(tester.takeException(), isNull);
  });

  testWidgets('hidden route invalidates a pending read and returning reloads and resumes graphs', (tester) async {
    final pending = Completer<SavedPanelRead>(), service = _Service(), store = SavedPanelTestStore();
    service.respond = (_, _, _) => pending.future;
    await _open(tester, service, store);
    final firstRange = service.reads.single.$3;
    await tester.pumpWidget(_app(service, store, isCurrent: () => true, active: false));
    await _pump(tester);
    pending.complete(_sample(firstRange, 111));
    await _pump(tester);
    await tester.pump(const Duration(seconds: 31));
    expect(service.reads, hasLength(1));
    expect(find.byType(MonitoringResultView), findsNothing);
    service.respond = (_, _, range) => Future.value(_sample(range, 999));
    await tester.pumpWidget(_app(service, store, isCurrent: () => true, active: true));
    await _pump(tester);
    expect(service.reads, hasLength(2));
    final chart = tester.widget<MonitoringResultView>(find.byType(MonitoringResultView));
    expect(chart.result.data!.series.single.samples.first.value, 999);
    await tester.pump(const Duration(seconds: 16));
    await _pump(tester);
    expect(service.reads, hasLength(3));
    expect(tester.takeException(), isNull);
  });

  testWidgets('binding-only setup automatically renders native built-in host graphs without custom saved panels', (
    tester,
  ) async {
    final service = _BindingService(), binding = bindingFixture();
    final store = MonitoringBindingTestStore(rows: [binding]);
    expect(store, isNot(isA<MonitoringSavedPanelStore>()));
    await _open(tester, service, store);
    expect(service.reads.map((r) => r.$3), [
      MonitoringPanel.hostOverview,
      MonitoringPanel.filesystem,
      MonitoringPanel.diskIo,
      MonitoringPanel.network,
    ]);
    expect(service.reads.every((r) => r.$1.id == monitoringFixture.id && r.$2.id == binding.id), isTrue);
    expect(service.reads.every((r) => r.$4.end - r.$4.start == 3600 && r.$4.stepSeconds == 15), isTrue);
    expect(find.byType(MonitoringResultView), findsNWidgets(8));
    for (final metric in {
      ...MonitoringPanel.hostOverview.metrics,
      ...MonitoringPanel.filesystem.metrics,
      ...MonitoringPanel.diskIo.metrics,
      ...MonitoringPanel.network.metrics,
    }) {
      final chart = tester.widget<MonitoringResultView>(
        find.byKey(ValueKey('dashboard-builtin-${binding.id}-${metric.name}')),
      );
      expect(chart.unit, metric.unit);
      expect(chart.result.data!.series.single.samples.first.value, 42);
    }
    expect(service.queries, 0);
    expect(store.saves, isEmpty);
    expect(find.text(tester.element(find.byType(MonitoringDashboard)).l10n.monitoringSetupNoGraphs), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('Docker fallback occurs only on missing host metrics then remains a Docker read on refresh', (
    tester,
  ) async {
    final service = _BindingService(), binding = bindingFixture();
    final store = MonitoringBindingTestStore(rows: [binding]);
    service.respond = (_, selected, panel, range) async => panel == MonitoringPanel.hostOverview
        ? BindingReadResult(
            bindingId: selected.id,
            bindingRevision: selected.revision,
            state: BindingReadState.unavailable,
            kind: 'bindingPanel',
            panel: panel,
            preparationReason: BindingPreparationReason.missingMetrics,
          )
        : service.samples(selected, panel, range);
    await _open(tester, service, store);
    expect(service.reads.map((r) => r.$3), [MonitoringPanel.hostOverview, MonitoringPanel.docker]);
    expect(find.byType(MonitoringResultView), findsNWidgets(2));
    final cpu = tester.widget<MonitoringResultView>(
      find.byKey(ValueKey('dashboard-builtin-${binding.id}-containerCpu')),
    );
    expect(cpu.unit, MonitoringUnit.cpuCores);
    await tester.pump(const Duration(seconds: 16));
    await _pump(tester);
    expect(service.reads.map((r) => r.$3), [
      MonitoringPanel.hostOverview,
      MonitoringPanel.docker,
      MonitoringPanel.docker,
    ]);
    expect(tester.takeException(), isNull);
  });

  for (final reason in [BindingPreparationReason.probeFailed, BindingPreparationReason.selectorConflict]) {
    testWidgets('host $reason does not trigger a speculative Docker read', (tester) async {
      final service = _BindingService(), binding = bindingFixture();
      final store = MonitoringBindingTestStore(rows: [binding]);
      service.respond = (_, selected, panel, _) async => BindingReadResult(
        bindingId: selected.id,
        bindingRevision: selected.revision,
        state: BindingReadState.unavailable,
        kind: 'bindingPanel',
        panel: panel,
        preparationReason: reason,
        preparationError: const MonitoringFailure.local(MonitoringErrorCode.transport),
      );
      await _open(tester, service, store);
      expect(service.reads.map((r) => r.$3), [MonitoringPanel.hostOverview]);
      expect(find.byType(MonitoringResultView), findsNothing);
      expect(find.byType(InfoBanner), findsOneWidget);
      expect(tester.takeException(), isNull);
    });
  }

  testWidgets('failed built-in refresh clears earlier successful graphs rather than showing stale success', (
    tester,
  ) async {
    final service = _BindingService(), store = MonitoringBindingTestStore(rows: [bindingFixture()]);
    await _open(tester, service, store);
    expect(find.byType(MonitoringResultView), findsNWidgets(8));
    service.respond = (_, _, _, _) => Future.error(const MonitoringFailure.local(MonitoringErrorCode.transport));
    await tester.pump(const Duration(seconds: 16));
    await _pump(tester);
    expect(find.byType(MonitoringResultView), findsNothing);
    expect(find.byType(InfoBanner), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('stale bindings never produce current graphs for another source revision', (tester) async {
    final service = _BindingService();
    final store = MonitoringBindingTestStore(rows: [bindingFixture(state: MonitoringBindingState.staleSource)]);
    await _open(tester, service, store);
    expect(service.reads, isEmpty);
    expect(find.byType(MonitoringResultView), findsNothing);
  });

  testWidgets('a missing requested source never falls back to another host graph and can be explicitly changed', (
    tester,
  ) async {
    final service = _Service(), store = SavedPanelTestStore();
    await _open(tester, service, store, requestedSourceId: _otherSource.id);
    expect(service.reads, isEmpty);
    expect(find.byType(MonitoringResultView), findsNothing);
    expect(find.byType(InfoBanner), findsOneWidget);
    tester
        .widget<DropdownButtonFormField<String>>(find.byKey(const ValueKey('monitoring-dashboard-source')))
        .onChanged!(monitoringFixture.id);
    await _pump(tester);
    expect(service.reads, hasLength(1));
    expect(service.reads.single.$1.id, monitoringFixture.id);
    expect(find.byKey(const ValueKey('dashboard-chart-$savedPanelId')), findsOneWidget);
    expect(find.byType(InfoBanner), findsNothing);
    expect(tester.takeException(), isNull);
  });

  for (final route in [SourceRoute.agentTunnel, SourceRoute.agentDestination]) {
    testWidgets('$route stays in the explicit Advanced read flow and never starts dashboard polling', (tester) async {
      final service = _Service(), store = SavedPanelTestStore();
      store.sources = [
        MonitoringSource(
          id: monitoringFixture.id,
          name: 'Explicit Agent stream',
          endpoint: monitoringFixture.endpoint,
          revision: 1,
          route: route,
          routeId: route == SourceRoute.agentTunnel ? 'approved-finite-stream' : null,
          hostId: route == SourceRoute.agentDestination ? '11111111-1111-4111-8111-111111111111' : null,
          associationId: route == SourceRoute.agentDestination ? '22222222-2222-4222-8222-222222222222' : null,
          destinationIp: route == SourceRoute.agentDestination ? '127.0.0.1' : null,
          destinationPort: route == SourceRoute.agentDestination ? 19090 : null,
        ),
      ];
      await _open(tester, service, store);
      expect(service.reads, isEmpty);
      await tester.pump(const Duration(seconds: 31));
      await _pump(tester);
      expect(service.reads, isEmpty);
      expect(tester.widget<IconButton>(find.byType(IconButton).first).onPressed, isNull);
      expect(find.byType(MonitoringResultView), findsNothing);
      final l = tester.element(find.byType(MonitoringDashboard)).l10n;
      expect(find.text(l.monitoringAgentRouteHint), findsOneWidget);
      expect(tester.takeException(), isNull);
    });
  }

  for (final switchSource in [false, true]) {
    testWidgets('wizard focus filters host/binding/custom panels then explicit source switch=$switchSource', (
      tester,
    ) async {
      final service = _FocusedService(), store = _FocusedStore();
      final chosen = _focusBinding('11111111-1111-4111-8111-111111111111', 'Chosen metrics', bindingHost.id);
      final sameHostOtherBinding = _focusBinding(
        '22222222-2222-4222-8222-222222222222',
        'Unselected same-host metrics',
        bindingHost.id,
      );
      final otherHostSameSource = _focusBinding(
        '33333333-3333-4333-8333-333333333333',
        'Other host sharing source',
        otherBindingHost.id,
      );
      final otherSourceBinding = _focusBinding(
        '44444444-4444-4444-8444-444444444444',
        'Explicit other source',
        otherBindingHost.id,
        sourceId: _otherSource.id,
      );
      store.sources = [monitoringFixture, _otherSource];
      store.bindings.addAll([sameHostOtherBinding, otherHostSameSource, chosen, otherSourceBinding]);
      store.panels = [savedPanel, _panelFor(_otherSource, id: _otherPanelId)];
      await _open(
        tester,
        service,
        store,
        requestedSourceId: monitoringFixture.id,
        requestedHostId: bindingHost.id,
        requestedBindingIds: [chosen.id],
      );
      expect(service.reads, hasLength(4));
      expect(service.reads.every((r) => r.$1.id == monitoringFixture.id && r.$2.id == chosen.id), isTrue);
      expect(
        service.savedReads,
        isEmpty,
        reason: 'Unrelated custom queries on a shared source must not be run for the wizard host',
      );
      expect(find.byType(MonitoringResultView), findsNWidgets(8));
      expect(find.text(chosen.name), findsOneWidget);
      for (final label in [
        sameHostOtherBinding.name,
        otherHostSameSource.name,
        otherSourceBinding.name,
        savedDefinition.title,
      ]) {
        expect(find.text(label), findsNothing);
      }
      if (switchSource) {
        tester
            .widget<DropdownButtonFormField<String>>(find.byKey(const ValueKey('monitoring-dashboard-source')))
            .onChanged!(_otherSource.id);
        await _pump(tester);
        expect(service.reads, hasLength(8));
        expect(
          service.reads.skip(4).every((r) => r.$1.id == _otherSource.id && r.$2.id == otherSourceBinding.id),
          isTrue,
        );
        expect(service.savedReads.single.$1.id, _otherSource.id);
        expect(service.savedReads.single.$2.id, _otherPanelId);
        expect(find.text(chosen.name), findsNothing);
        expect(find.text(otherSourceBinding.name), findsOneWidget);
        expect(find.byType(MonitoringResultView), findsNWidgets(9));
      }
      expect(store.saves, isEmpty);
      expect(tester.takeException(), isNull);
    });
  }
}
