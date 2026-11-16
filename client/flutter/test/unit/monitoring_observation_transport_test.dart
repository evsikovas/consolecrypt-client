import 'dart:convert';

import 'package:consolecrypt/monitoring/monitoring_binding_models.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_pinned_observation.dart';
import 'package:consolecrypt/monitoring/monitoring_pinned_transport.dart';
import 'package:consolecrypt/monitoring/monitoring_saved_panel_models.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window.dart';
import 'package:flutter_test/flutter_test.dart';

void check(bool value) {
  if (!value) throw StateError('Transport expectation failed');
}

const source = MonitoringSource(
  id: '11111111-1111-4111-8111-111111111111',
  name: 'Owned Native source',
  endpoint: 'http://metrics.invalid',
  revision: 7,
);
final binding = MonitoringBinding(
  id: '22222222-2222-4222-8222-222222222222',
  name: 'Owned exact binding',
  revision: 2,
  hostId: '44444444-4444-4444-8444-444444444444',
  sourceId: source.id,
  sourceRevision: 7,
  selector: const {'instance': 'owned'},
  state: MonitoringBindingState.current,
);
const saved = SavedMonitoringPanel(
  id: '33333333-3333-4333-8333-333333333333',
  revision: 4,
  definition: SavedPanelDefinition(
    title: 'Owned saved expression',
    sourceId: '11111111-1111-4111-8111-111111111111',
    sourceRevision: 7,
    query: 'native_stored_expression_not_a_ui_fallback',
    unitHint: SavedPanelUnitHint.bytes,
  ),
  state: SavedPanelState.current,
);
Json outcome() => {
  'status': 'success',
  'httpStatus': 200,
  'partial': false,
  'data': {'kind': 'vector', 'series': <Json>[]},
};

class Store implements MonitoringSourceStore, MonitoringBindingStore, MonitoringSavedPanelStore {
  @override
  bool readOnly = false;
  List<MonitoringSource> values = [source];
  @override
  List<MonitoringBinding> bindings = [binding];
  @override
  List<SavedMonitoringPanel> panels = [saved];
  int loads = 0;
  @override
  Future<List<MonitoringSource>> load() async {
    loads++;
    return values;
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => throw StateError('No mutation is allowed in observation transport');
}

class Backend implements MonitoringBackend {
  final store = Store(), requests = <Json>[];
  final cancelled = <String>[];
  int services = 0;
  @override
  MonitoringSourceStore createSourceStore() => store;
  @override
  MonitoringService createService() {
    services++;
    return AppCoreMonitoringService(
      read: (request) async {
        requests.add(request);
        final action = object(request['action']);
        final Json result = switch (action['kind']) {
          'savedPanel' => {
            'kind': 'savedPanel',
            'panelId': saved.id,
            'panelRevision': saved.revision,
            'unitHint': saved.definition.unitHint.name,
            'outcome': outcome(),
          },
          'bindingPanel' => {
            'kind': 'bindingPanel',
            'bindingId': binding.id,
            'bindingRevision': binding.revision,
            'panel': action['panel'],
            'state': 'missing',
            'resolution': {'state': 'missing'},
            'discovery': outcome(),
          },
          'query' || 'queryRange' => outcome(),
          _ => throw StateError('No discovery/extra query is permitted'),
        };
        return {'sourceId': request['sourceId'], 'sourceRevision': request['sourceRevision'], 'result': result};
      },
      cancelOperation: (id) async {
        cancelled.add(id);
      },
    );
  }
}

void main() {
  final window = MonitoringTimeWindow.preset(MonitoringTimePreset.hour1, DateTime.utc(2026, 11, 12));
  test('construct/load context does not query or create a second service', () async {
    final b = Backend();
    final actual = ScopedMonitoringPinTransport(b);
    const p = MonitoringPinPlan.forTarget(
      source: source,
      target: MonitoringSavedPanelTarget(panel: saved),
    );
    check((await actual.loadContext(p))!.matches(p));
    check(b.services == 1 && b.store.loads == 1 && b.requests.isEmpty);
    actual.cancel();
  });
  test('custom range sends one existing queryRange with exact explicit UTC bounds', () async {
    final b = Backend(),
        p = MonitoringPinPlan.forTarget(
          source: source,
          target: MonitoringCustomRangeTarget(
            expression: 'rate(owned_total[5m])',
            windowPolicy: MonitoringFixedObservationWindow(window),
          ),
        );
    final t = ScopedMonitoringPinTransport(b);
    final context = await t.loadContext(p);
    final value = await t.readTarget(p, context!, window);
    check(value is MonitoringObservedQuery && value.window == window && b.requests.length == 1);
    check(
      jsonEncode(b.requests.single['action']) ==
          jsonEncode({
            'kind': 'queryRange',
            'expression': 'rate(owned_total[5m])',
            'start': window.startSeconds,
            'end': window.endSeconds,
            'stepSeconds': 15.0,
          }),
    );
    t.cancel();
  });
  test('saved range uses only Native ID/revision with no stored-expression fallback', () async {
    final b = Backend(),
        p = MonitoringPinPlan.forTarget(
          source: source,
          target: MonitoringSavedPanelTarget(panel: saved, windowPolicy: MonitoringFixedObservationWindow(window)),
        );
    final t = ScopedMonitoringPinTransport(b);
    final context = await t.loadContext(p);
    final value = await t.readTarget(p, context!, window), action = object(b.requests.single['action']);
    check(
      value is MonitoringObservedSavedPanel &&
          value.window == window &&
          value.read.unitHint == SavedPanelUnitHint.bytes,
    );
    check(
      jsonEncode(action) ==
          jsonEncode({
            'kind': 'savedPanel',
            'panelId': saved.id,
            'panelRevision': 4,
            'range': {'start': window.startSeconds, 'end': window.endSeconds, 'stepSeconds': 15.0},
          }),
    );
    check(
      !jsonEncode(b.requests).contains(saved.definition.query) && !jsonEncode(b.requests).contains(source.endpoint),
    );
    t.cancel();
  });
  test('saved current values omit range without inventing history bounds', () async {
    final b = Backend(),
        p = const MonitoringPinPlan.forTarget(
          source: source,
          target: MonitoringSavedPanelTarget(panel: saved),
        );
    final t = ScopedMonitoringPinTransport(b);
    final context = await t.loadContext(p);
    final value = await t.readTarget(p, context!, null);
    check(value.window == null && !object(b.requests.single['action']).containsKey('range'));
    t.cancel();
  });
  test('binding uses one Native panel action and accepts missing as missing, not zero or synthetic queries', () async {
    final b = Backend(),
        p = MonitoringPinPlan.forTarget(
          source: source,
          target: MonitoringBindingPanelTarget(
            binding: binding,
            panel: MonitoringPanel.cpu,
            windowPolicy: MonitoringFixedObservationWindow(window),
          ),
        );
    final t = ScopedMonitoringPinTransport(b);
    final context = await t.loadContext(p);
    final value = await t.readTarget(p, context!, window), action = object(b.requests.single['action']);
    check(
      value is MonitoringObservedBindingPanel &&
          value.read.state == BindingReadState.missing &&
          value.read.queries.isEmpty,
    );
    check(action['kind'] == 'bindingPanel' && action['bindingId'] == binding.id && action['bindingRevision'] == 2);
    check(action['panel'] == 'cpu' && action['end'] == window.endSeconds && !action.containsKey('expression'));
    t.cancel();
  });
  test('missing Native saved record is denied before any HTTP/read action', () async {
    final b = Backend()..store.panels = [],
        p = const MonitoringPinPlan.forTarget(
          source: source,
          target: MonitoringSavedPanelTarget(panel: saved),
        );
    final t = ScopedMonitoringPinTransport(b);
    final context = await t.loadContext(p);
    check(!context!.matches(p));
    try {
      await t.readTarget(p, context, null);
      throw StateError('Expected denied record');
    } on MonitoringFailure catch (error) {
      check(error.kind == MonitoringErrorCode.sourceChanged);
    }
    check(b.requests.isEmpty);
    t.cancel();
  });
  test('read-only catalog denies observation instead of treating it as an empty current catalog', () async {
    final b = Backend()..store.readOnly = true;
    final actual = ScopedMonitoringPinTransport(b),
        p = const MonitoringPinPlan.forTarget(
          source: source,
          target: MonitoringSavedPanelTarget(panel: saved),
        );
    try {
      await actual.loadContext(p);
      throw StateError('Expected schema denial');
    } on MonitoringFailure catch (error) {
      check(error.kind == MonitoringErrorCode.readOnly);
    }
    check(b.requests.isEmpty);
    actual.cancel();
  });
}
