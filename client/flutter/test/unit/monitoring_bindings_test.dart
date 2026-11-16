import 'dart:convert';

import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';
import 'package:flutter_test/flutter_test.dart';

const hostId = '11111111-1111-4111-8111-111111111111';
const sourceId = '22222222-2222-4222-8222-222222222222';
const bindingId = '33333333-3333-4333-8333-333333333333';
const source = MonitoringSource(id: sourceId, name: 'Fixture', endpoint: 'http://metrics.test', revision: 2);
Json bindingRow({
  String id = bindingId,
  int revision = 1,
  int sourceRevision = 2,
  String state = 'current',
  Map<String, String> selector = const {'job': 'node', 'instance': 'explicit:9100'},
}) => {
  'id': id,
  'name': 'Node exporter',
  'revision': revision,
  'sourceRevision': sourceRevision,
  'binding': {'version': 1, 'hostId': hostId, 'sourceId': sourceId, 'revision': revision, 'selector': selector},
  'state': state,
};
Json catalog({int version = 2, int revision = 4, List<Json>? bindings}) => {
  'schemaVersion': version,
  'revision': revision,
  'readOnly': false,
  'sources': [
    {'id': source.id, 'revision': source.revision, 'readOnly': false, 'configuration': source.toJson()},
  ],
  'bindings': bindings ?? [bindingRow()],
};
MonitoringBinding draft({
  String id = '',
  int revision = 0,
  int sourceRevision = 2,
  Map<String, String> selector = const {'job': 'node', 'instance': 'explicit:9100'},
}) => MonitoringBinding(
  id: id,
  name: 'Node exporter',
  hostId: hostId,
  sourceId: sourceId,
  revision: revision,
  sourceRevision: sourceRevision,
  selector: selector,
  state: MonitoringBindingState.current,
);

void main() {
  test('first explicit binding create uses shared catalog CAS and Native assigned ID; update/delete retain stable identity', () async {
    final calls = <(String, Json)>[];
    final store = AppCoreMonitoringSourceStore((method, args) async {
      calls.add((method, args));
      return switch (method) {
        'monitoring_sources' => catalog(version: 1, revision: 3, bindings: []),
        'monitoring_binding_create' => catalog(),
        'monitoring_binding_update' => catalog(revision: 5, bindings: [bindingRow(revision: 2)]),
        'monitoring_binding_delete' => catalog(revision: 6, bindings: []),
        'monitoring_source_update' => catalog(revision: 7, bindings: []),
        _ => throw StateError('Unexpected RPC'),
      };
    });
    await store.load();
    expect(calls.map((c) => c.$1), ['monitoring_sources']);
    expect(store.bindings, isEmpty);
    final saved = await store.saveBinding(draft(), create: true);
    final create = object(jsonDecode(calls[1].$2['requestJson'] as String));
    expect(create, {
      'expectedCatalogRevision': 3,
      'name': 'Node exporter',
      'hostId': hostId,
      'sourceId': sourceId,
      'expectedSourceRevision': 2,
      'selector': {'job': 'node', 'instance': 'explicit:9100'},
    });
    expect(saved.id, bindingId);
    expect(saved.revision, 1);
    final updated = await store.saveBinding(saved);
    final update = object(jsonDecode(calls[2].$2['requestJson'] as String));
    expect(update['id'], bindingId);
    expect(update['expectedCatalogRevision'], 4);
    expect(update['expectedBindingRevision'], 1);
    expect(updated.revision, 2);
    await store.deleteBinding(updated);
    expect(object(jsonDecode(calls[3].$2['requestJson'] as String)), {
      'id': bindingId,
      'expectedCatalogRevision': 5,
      'expectedBindingRevision': 2,
    });
    await store.upsert(source);
    expect(calls.last.$2['expectedCatalogRevision'], 6);
  });

  test('source delete is blocked by every binding reference, including missing host orphans', () async {
    var writes = 0;
    final store = AppCoreMonitoringSourceStore((method, _) async {
      if (method != 'monitoring_sources') writes++;
      return catalog(bindings: [bindingRow(state: 'missingHost')]);
    });
    await store.load();
    await expectLater(
      store.delete(source),
      throwsA(isA<MonitoringFailure>().having((e) => e.kind, 'kind', MonitoringErrorCode.sourceInUse)),
    );
    expect(writes, 0);
  });

  test('source update preserves Native pinned binding revision and stale state without automatic repair', () async {
    final calls = <String>[];
    final store = AppCoreMonitoringSourceStore((method, _) async {
      calls.add(method);
      return catalog(
        revision: calls.length + 3,
        bindings: [bindingRow(sourceRevision: 1, state: 'staleSource')],
      );
    });
    await store.load();
    await store.upsert(source);
    expect(store.bindings.single.sourceRevision, 1);
    expect(store.bindings.single.state, MonitoringBindingState.staleSource);
    expect(calls, ['monitoring_sources', 'monitoring_source_update']);
  });

  test('multiple named selectors may share one host/source; exact duplicates make the whole view read-only', () async {
    final valid = catalog(
      bindings: [
        bindingRow(),
        bindingRow(id: '44444444-4444-4444-8444-444444444444', selector: {'job': 'cadvisor'}),
      ],
    );
    var value = valid;
    final store = AppCoreMonitoringSourceStore((_, _) async => value);
    await store.load();
    expect(store.bindings.length, 2);
    value = catalog(
      bindings: [
        bindingRow(),
        bindingRow(id: '44444444-4444-4444-8444-444444444444', selector: {'instance': 'explicit:9100', 'job': 'node'}),
      ],
    );
    await expectLater(store.load(), throwsA(isA<MonitoringFailure>()));
    expect(store.readOnly, true);
  });

  test('unknown schema/record fields never write or repair raw catalog through CRUD', () async {
    final malformed = <Json>[
      catalog(version: 7),
      catalog(version: 1),
      catalog(
        bindings: [
          {...bindingRow(), 'futureField': true},
        ],
      ),
      catalog(
        bindings: [
          {
            ...bindingRow(),
            'binding': {...object(bindingRow()['binding']), 'version': 2},
          },
        ],
      ),
      catalog(
        bindings: [
          {...bindingRow(), 'revision': 2},
        ],
      ),
      catalog(
        bindings: [
          {...bindingRow(), 'state': 'futureState'},
        ],
      ),
    ];
    for (final raw in malformed) {
      final before = jsonEncode(raw);
      final calls = <String>[];
      final store = AppCoreMonitoringSourceStore((method, _) async {
        calls.add(method);
        return raw;
      });
      await expectLater(store.load(), throwsA(isA<MonitoringFailure>()));
      expect(store.readOnly, true);
      await expectLater(store.saveBinding(draft(), create: true), throwsA(isA<MonitoringFailure>()));
      await expectLater(store.upsert(source), throwsA(isA<MonitoringFailure>()));
      expect(calls, ['monitoring_sources']);
      expect(jsonEncode(raw), before);
    }
  });

  test('manual exact selectors retain escaping and Unicode; bounds and metric name selectors fail before RPC', () {
    final exact = draft(selector: {'instance': 'literal "quote" \\path\nvalue', 'job': 'экспортер'});
    exact.validate(create: true);
    expect(exact.selector['instance'], 'literal "quote" \\path\nvalue');
    for (final labels in <Map<String, String>>[
      {},
      {'__name__': 'up'},
      {'job': ''},
      {'job=~': '.*'},
      {'1bad': 'node'},
      {'job': 'x' * 4097},
      {for (var i = 0; i < 33; i++) 'label$i': 'v'},
    ]) {
      expect(() => draft(selector: labels).validate(create: true), throwsA(isA<MonitoringFailure>()));
    }
    for (final state in MonitoringBindingState.values) {
      expect(MonitoringBinding.fromRecord(bindingRow(state: state.name)).state, state);
    }
  });
}
