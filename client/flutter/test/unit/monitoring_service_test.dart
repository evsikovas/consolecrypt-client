import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/monitoring/monitoring_failure_policy.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';
import 'package:flutter_test/flutter_test.dart';

const source = MonitoringSource(id: 'fixture', name: 'Fixture', endpoint: 'http://127.0.0.1:9090');
Json outcome({String kind = 'matrix'}) => {
  'status': 'success',
  'httpStatus': 200,
  'data': {
    'kind': kind,
    'series': [
      {
        'labels': {'job': 'fixture'},
        'samples': [
          {
            'timestamp': 1000,
            'value': {'kind': 'finite', 'value': 2},
          },
          {
            'timestamp': 1015,
            'value': {'kind': 'nan'},
          },
          {
            'timestamp': 1030,
            'value': {'kind': 'positiveInfinity'},
          },
          {
            'timestamp': 1045,
            'value': {'kind': 'negativeInfinity'},
          },
        ],
      },
    ],
  },
  'warnings': ['partial fixture warning'],
  'infos': ['fixture annotation'],
  'partial': true,
  'error': null,
  'effectiveStepSeconds': 15,
};

void main() {
  test('existing Native metadata-cache operation is explicit source-revision scoped and acknowledged', () async {
    final requests = <Json>[];
    final service = AppCoreMonitoringService(read: (request) async {
      requests.add(request);
      return {'sourceId': source.id, 'sourceRevision': source.revision, 'result': {'cleared': true}};
    }, cancelOperation: (_) async {});
    expect(service, isA<MonitoringMetadataCache>());
    expect(service.labelCacheTtl, const Duration(seconds: 60));
    expect(requests, isEmpty);
    await service.clearCache(source);
    expect(requests, hasLength(1));
    expect(requests.single['sourceId'], source.id);
    expect(requests.single['sourceRevision'], source.revision);
    expect(requests.single['action'], {'kind': 'clearCache'});
    expect(requests.single.keys.toSet(), {'operationId', 'sourceId', 'sourceRevision', 'action'});
  });
  test('cache clearing rejects malformed or foreign acknowledgments without another operation', () async {
    for (final ack in <Json>[{}, {'cleared': false}, {'cleared': 'true'}, {'cleared': true, 'ready': true}]) {
      var calls = 0;
      final service = AppCoreMonitoringService(read: (_) async {
        calls++;
        return {'sourceId': source.id, 'sourceRevision': source.revision, 'result': ack};
      }, cancelOperation: (_) async {});
      await expectLater(service.clearCache(source), throwsA(isA<MonitoringFailure>()));
      expect(calls, 1);
    }
    final service = AppCoreMonitoringService(read: (_) async => {
      'sourceId': source.id, 'sourceRevision': source.revision + 1, 'result': {'cleared': true},
    }, cancelOperation: (_) async {});
    await expectLater(service.clearCache(source), throwsA(isA<MonitoringFailure>()));
  });

  test('sshHost route roundtrip is a separate exact Host ID namespace and preserves source settings', () {
    const selected = MonitoringSource(id: 'source', name: 'Saved route',
      endpoint: 'http://metrics.example.test/base', route: SourceRoute.sshHost,
      hostId: 'a1111111-1111-4111-8111-111111111111', auth: SourceAuth.basicRef,
      credentialId: 'credential-reference', customCaPem: 'opaque CA fixture');
    selected.validate();
    final json = selected.toJson();
    expect(json['route'], {'kind': 'sshHost', 'hostId': selected.hostId});
    expect(MonitoringSource.fromJson(json).fingerprint, selected.fingerprint);
    final saved = selected.withRecord(revision: 9);
    expect(saved.hostId, selected.hostId);
    expect(saved.routeId, isNull);
    expect(saved.endpoint, selected.endpoint);
    expect(saved.credentialId, selected.credentialId);
    expect(saved.customCaPem, selected.customCaPem);
    final changed = MonitoringSource.fromJson({...json, 'route': {
      'kind': 'sshHost', 'hostId': 'a2222222-2222-4222-8222-222222222222'}});
    expect(changed.fingerprint, isNot(selected.fingerprint));
  });
  test('sshHost never adopts legacy routeId or nil malformed host references', () {
    final base = source.toJson();
    for (final route in [
      {'kind': 'sshHost', 'routeId': 'a1111111-1111-4111-8111-111111111111'},
      {'kind': 'sshHost', 'hostId': ''},
      {'kind': 'sshHost', 'hostId': 'legacy-route-id'},
      {'kind': 'sshHost', 'hostId': '00000000-0000-0000-0000-000000000000'},
      {'kind': 'sshHost', 'hostId': 'a1111111-1111-4111-8111-111111111111', 'routeId': 'legacy'},
    ]) {
      expect(() => MonitoringSource.fromJson({...base, 'route': route}), throwsA(isA<MonitoringFailure>()));
    }
    for (final kind in [SourceRoute.sshTunnel, SourceRoute.agentTunnel]) {
      final old = MonitoringSource.fromJson({...base, 'route': {'kind': kind.name, 'routeId': 'old-reference'}});
      expect(old.toJson()['route'], {'kind': kind.name, 'routeId': 'old-reference'});
      expect(old.hostId, isNull);
    }
  });

  test('server error types cannot borrow Native-only retry categories', () {
    for (final type in ['transport', 'busy', 'unknown_future_type']) {
      final server = MonitoringFailure.fromJson({'type': type, 'message': 'Synthetic API type', 'httpStatus': 503});
      expect(server.isServerMessage, true);
      expect(monitoringTransientFailure(server), false);
    }
    for (final status in [null, 200, 401]) {
      final server = MonitoringFailure.fromJson({
        'type': 'timeout',
        'message': 'Synthetic API timeout',
        'httpStatus': ?status,
      });
      expect(monitoringTransientFailure(server), false);
    }
    expect(monitoringTransientFailure(const MonitoringFailure.local(MonitoringErrorCode.transport)), true);
    expect(monitoringTransientFailure(const MonitoringFailure.local(MonitoringErrorCode.busy)), true);
    expect(
      monitoringTransientFailure(
        MonitoringFailure.fromJson({'type': 'timeout', 'message': 'Synthetic API timeout', 'httpStatus': 503}),
      ),
      true,
    );
  });
  test('literal Native ApiError uses its envelope status for transient classification', () {
    QueryOutcome response(int status, Json error) => QueryOutcome.fromJson({
      'status': 'error',
      'httpStatus': status,
      'data': null,
      'warnings': <String>[],
      'infos': <String>[],
      'partial': false,
      'error': error,
    });
    final unavailable = response(503, {'type': 'unavailable', 'message': 'Synthetic fixture'}).error!;
    expect(unavailable.httpStatus, 503);
    expect(unavailable.isServerMessage, true);
    expect(monitoringTransientFailure(unavailable), true);
    expect(monitoringTransientFailure(response(401, {'type': 'unavailable', 'message': 'Denied'}).error), false);
    expect(
      monitoringTransientFailure(response(503, {'type': 'unknown_future_type', 'message': 'Unknown'}).error),
      false,
    );
    final nativeCode = response(503, {
      'code': 'unavailable',
      'message': 'Unknown native code',
      'httpStatus': 503,
    }).error!;
    expect(nativeCode.isServerMessage, false);
    expect(monitoringTransientFailure(nativeCode), false);
    expect(monitoringTransientFailure(response(503, {'code': 'invalid_response', 'message': 'Schema'}).error), false);
    final labels = LabelValues.fromJson({
      'values': <String>[],
      'warnings': <String>[],
      'infos': <String>[],
      'partial': false,
      'cached': false,
      'httpStatus': 429,
      'error': {'type': 'unavailable', 'message': 'Synthetic limit'},
    });
    expect(labels.error!.httpStatus, 429);
    expect(monitoringTransientFailure(labels.error), true);
  });
  test('contract source roundtrip includes only refs and rejects endpoint authority smuggling', () {
    const ref = MonitoringSource(
      id: 'id',
      name: 'Ref',
      endpoint: 'https://metrics.example.test/prometheus',
      auth: SourceAuth.bearerRef,
      credentialId: 'credential-reference',
      route: SourceRoute.sshTunnel,
      routeId: 'route-reference',
      customCaPem: 'fixture certificate',
    );
    expect(MonitoringSource.fromJson(ref.toJson()).fingerprint, ref.fingerprint);
    expect(ref.toJson()['auth'], {'kind': 'bearerRef', 'credentialId': 'credential-reference'});
    for (final endpoint in [
      'https://user:password@metrics.test',
      'https://metrics.test?token=x',
      'https://metrics.test#fragment',
      'file:///tmp/metrics',
    ]) {
      expect(
        () => MonitoringSource(id: 'id', name: 'Name', endpoint: endpoint).validate(),
        throwsA(isA<MonitoringFailure>()),
      );
    }
  });
  test('typed range preserves nonfinite samples warnings partial and effective step', () {
    final value = QueryOutcome.fromJson(outcome());
    expect(value.partial, true);
    expect(value.warnings, ['partial fixture warning']);
    expect(value.data!.series.single.samples.map((s) => s.display), ['2.0', 'NaN', '+Inf', '-Inf']);
    expect(value.data!.series.single.samples[1].value, isNull);
    expect(value.effectiveStepSeconds, 15);
  });
  test('both probe endpoints are required for compatibility', () {
    final caps = SourceCapabilities.fromJson({
      'query': {'supported': true, 'httpStatus': 200, 'warnings': <String>[]},
      'queryRange': {
        'supported': false,
        'httpStatus': 404,
        'warnings': <String>[],
        'error': {'code': 'invalid_response', 'message': 'Not supported'},
      },
    });
    expect(caps.compatible, false);
    expect(caps.queryRange.error!.code, 'invalid_response');
  });
  test('AppCore read sends only native saved identity and revision, validates response identity', () async {
    Json? captured;
    final saved = source.withRecord(revision: 7);
    final service = AppCoreMonitoringService(
      read: (request) async {
        captured = request;
        return {'sourceId': saved.id, 'sourceRevision': 7, 'result': outcome()};
      },
      cancelOperation: (_) async {},
    );
    await service.query(saved, const MonitoringQuery(expression: 'up'));
    expect(captured!.keys, unorderedEquals(['operationId', 'sourceId', 'sourceRevision', 'action']));
    expect(captured!['sourceRevision'], 7);
    expect(
      captured!['operationId'],
      matches(RegExp(r'^[a-f0-9]{8}-[a-f0-9]{4}-4[a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$')),
    );
    expect(captured!.containsKey('endpoint'), false);
    final stale = AppCoreMonitoringService(
      read: (_) async => {'sourceId': source.id, 'sourceRevision': 8, 'result': outcome()},
      cancelOperation: (_) async {},
    );
    await expectLater(
      stale.query(saved, const MonitoringQuery(expression: 'up')),
      throwsA(isA<MonitoringFailure>().having((e) => e.code, 'code', 'source_changed')),
    );
  });
  test('AppCore cancellation addresses only the pending operation UUID', () async {
    final pending = Completer<Json>();
    final cancellations = <String>[];
    String? operation;
    final service = AppCoreMonitoringService(
      read: (request) {
        operation = request['operationId'] as String;
        return pending.future;
      },
      cancelOperation: (id) async {
        cancellations.add(id);
      },
    );
    final request = service.query(source, const MonitoringQuery(expression: 'up'));
    service.cancel();
    expect(cancellations, [operation]);
    pending.complete({'sourceId': source.id, 'sourceRevision': 0, 'result': outcome()});
    await request;
    service.cancel();
    expect(cancellations.length, 1);
  });
  test('AppCore source writes use catalog/source CAS and native assigned identity', () async {
    final calls = <(String, Json)>[];
    Json record(String id, int revision) => {
      'id': id,
      'revision': revision,
      'readOnly': false,
      'configuration': {...source.toJson(), 'id': id},
    };
    Json catalog(int revision, List<Json> records) => {
      'schemaVersion': 1,
      'revision': revision,
      'readOnly': false,
      'sources': records,
    };
    final store = AppCoreMonitoringSourceStore((method, arguments) async {
      calls.add((method, arguments));
      return switch (method) {
        'monitoring_sources' => catalog(1, [record('existing', 2)]),
        'monitoring_source_create' => catalog(2, [record('existing', 2), record('native-created', 1)]),
        'monitoring_source_update' => catalog(3, [record('existing', 2), record('native-created', 2)]),
        'monitoring_source_delete' => catalog(4, [record('existing', 2)]),
        _ => throw StateError('Unexpected method'),
      };
    });
    await store.load();
    final saved = await store.upsert(source, create: true);
    expect(saved.id, 'native-created');
    expect(saved.revision, 1);
    expect(calls[1].$2['expectedCatalogRevision'], 1);
    expect(object(jsonDecode(calls[1].$2['configurationJson'] as String))['id'], '');
    final updated = await store.upsert(saved);
    expect(calls[2].$2['expectedCatalogRevision'], 2);
    expect(calls[2].$2['expectedSourceRevision'], 1);
    await store.delete(updated);
    expect(calls.last.$2, {'id': 'native-created', 'expectedCatalogRevision': 3, 'expectedSourceRevision': 2});
  });
}
