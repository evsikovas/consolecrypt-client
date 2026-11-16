import 'dart:async';

import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_pinned_transport.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';
import 'package:flutter_test/flutter_test.dart';

import '../helpers/monitoring_fakes.dart';

class _NativeBackend implements MonitoringBackend {
  final store = MonitoringTestStore();
  final requests = <Json>[];
  final pending = <Completer<Json>>[];
  final cancelled = <String>[];
  @override
  MonitoringService createService() => AppCoreMonitoringService(
    read: (request) {
      requests.add(request);
      final result = Completer<Json>();
      pending.add(result);
      return result.future;
    },
    cancelOperation: (id) async => cancelled.add(id),
  );
  @override
  MonitoringSourceStore createSourceStore() => store;
  void complete(int index) => pending[index].complete({
    'sourceId': requests[index]['sourceId'],
    'sourceRevision': requests[index]['sourceRevision'],
    'result': {
      'status': 'success',
      'httpStatus': 200,
      'partial': false,
      'data': {'kind': 'vector', 'series': <Json>[]},
    },
  });
}

void main() {
  test(
    'actual AppCore service cancellation is isolated by request UUID and pin emits only an instant action',
    () async {
      final backend = _NativeBackend();
      final normal = backend.createService(), pin = ScopedMonitoringPinTransport(backend);
      final current = await pin.loadSource(monitoringFixture.id);
      expect(current, same(monitoringFixture));
      final first = normal.query(monitoringFixture, const MonitoringQuery(expression: 'normal'));
      final second = pin.readCurrent(current!, 'up{job="pinned"}');
      expect(backend.requests[1]['action'], {'kind': 'query', 'expression': 'up{job="pinned"}'});
      expect(backend.requests[1]['sourceRevision'], 1);
      expect(backend.requests[1].keys, unorderedEquals(['operationId', 'sourceId', 'sourceRevision', 'action']));
      expect(backend.requests[0]['operationId'], isNot(backend.requests[1]['operationId']));
      normal.cancel();
      await Future<void>.value();
      expect(backend.cancelled, [backend.requests[0]['operationId']]);
      backend.complete(1);
      expect((await second).status, 'success');
      backend.complete(0);
      await first;
      final third = pin.readCurrent(current, 'up');
      pin.cancel();
      await Future<void>.value();
      expect(backend.cancelled.last, backend.requests[2]['operationId']);
      backend.complete(2);
      await third;
    },
  );
  test('transport rechecks authoritative catalog and never treats unknown schema as an empty current source', () async {
    final backend = _NativeBackend(), transport = ScopedMonitoringPinTransport(_NativeBackend());
    backend.store.sources = [];
    expect(await ScopedMonitoringPinTransport(backend).loadSource(monitoringFixture.id), isNull);
    backend.store.readOnly = true;
    await expectLater(
      ScopedMonitoringPinTransport(backend).loadSource(monitoringFixture.id),
      throwsA(isA<MonitoringFailure>()),
    );
    expect(backend.requests, isEmpty);
    transport.cancel();
  });
}
