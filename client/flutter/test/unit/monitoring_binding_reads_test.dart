import 'dart:async';

import 'package:consolecrypt/monitoring/monitoring_binding_models.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';
import 'package:flutter_test/flutter_test.dart';

import '../helpers/monitoring_binding_fakes.dart';
import '../helpers/monitoring_binding_reads_fakes.dart';
import '../helpers/monitoring_fakes.dart';

void main() {
  test(
    'discovery and all ten panel requests send saved identities/time/enum only; no UI resolution or expression',
    () async {
      final requests = <Json>[];
      final service = AppCoreMonitoringService(
        read: (request) async {
          requests.add(request);
          final action = object(request['action']);
          return {
            'sourceId': monitoringFixture.id,
            'sourceRevision': 1,
            'result': action['kind'] == 'bindingDiscover'
                ? bindingDiscoveryJson()
                : bindingPanelJson(panel: MonitoringPanel.values.byName(action['panel'] as String)),
          };
        },
        cancelOperation: (_) async {},
      );
      await service.discover(monitoringFixture, bindingFixture(), 1000);
      expect(object(requests.first['action']), {
        'kind': 'bindingDiscover',
        'bindingId': bindingFixture().id,
        'bindingRevision': 7,
        'time': 1000.0,
      });
      for (final panel in MonitoringPanel.values) {
        final result = await service.readPanel(
          monitoringFixture,
          bindingFixture(),
          panel,
          start: 0,
          end: 1000,
          stepSeconds: 15,
        );
        expect(result.panel, panel);
        expect(result.queries.map((q) => q.metric).toSet(), panel.metrics);
        expect(object(requests.last['action']), {
          'kind': 'bindingPanel',
          'bindingId': bindingFixture().id,
          'bindingRevision': 7,
          'panel': panel.name,
          'start': 0.0,
          'end': 1000.0,
          'stepSeconds': 15.0,
        });
      }
      for (final request in requests) {
        expect(request.keys, unorderedEquals(['operationId', 'sourceId', 'sourceRevision', 'action']));
        expect(request['sourceId'], monitoringFixture.id);
        expect(request['sourceRevision'], 1);
      }
    },
  );

  test('binding response validates saved binding revision and source revision; stale metadata never reads', () async {
    var reads = 0;
    final service = AppCoreMonitoringService(
      read: (_) async {
        reads++;
        return {
          'sourceId': monitoringFixture.id,
          'sourceRevision': 1,
          'result': {...bindingDiscoveryJson(), 'bindingRevision': 8},
        };
      },
      cancelOperation: (_) async {},
    );
    await expectLater(
      service.discover(monitoringFixture, bindingFixture(), 1000),
      throwsA(isA<MonitoringFailure>().having((e) => e.kind, 'kind', MonitoringErrorCode.bindingChanged)),
    );
    await expectLater(
      service.discover(monitoringFixture, bindingFixture(state: MonitoringBindingState.staleSource), 1000),
      throwsA(isA<MonitoringFailure>()),
    );
    await expectLater(
      service.discover(monitoringFixture.withRecord(revision: 2), bindingFixture(), 1000),
      throwsA(isA<MonitoringFailure>()),
    );
    await expectLater(
      service.discover(monitoringFixture, bindingFixture(), double.nan),
      throwsA(isA<MonitoringFailure>()),
    );
    expect(reads, 1);
  });

  test('binding cancellation targets only binding operation IDs; workspace cancellation still cancels all', () async {
    final pendingRaw = Completer<Json>(), pendingBinding = Completer<Json>();
    final requests = <Json>[], cancelled = <String>[];
    final service = AppCoreMonitoringService(
      read: (request) {
        requests.add(request);
        return object(request['action'])['kind'] == 'query' ? pendingRaw.future : pendingBinding.future;
      },
      cancelOperation: (id) async {
        cancelled.add(id);
      },
    );
    final raw = service.query(monitoringFixture, const MonitoringQuery(expression: 'up'));
    final binding = service.discover(monitoringFixture, bindingFixture(), 1000);
    service.cancelBinding();
    expect(cancelled, [requests[1]['operationId']]);
    service.cancel();
    expect(cancelled, contains(requests[0]['operationId']));
    pendingRaw.complete({'sourceId': monitoringFixture.id, 'sourceRevision': 1, 'result': identityOutcome()});
    pendingBinding.complete({'sourceId': monitoringFixture.id, 'sourceRevision': 1, 'result': bindingDiscoveryJson()});
    await Future.wait([raw, binding]);
  });

  test('native original ambiguity indices including duplicate labels and down identity are preserved', () {
    final ambiguous = BindingReadResult.fromJson(bindingDiscoveryJson(state: 'ambiguous', candidates: 2));
    expect(ambiguous.candidateIndices, [0, 1]);
    expect(ambiguous.discovery!.data!.series[0].labels, ambiguous.discovery!.data!.series[1].labels);
    final resolved = BindingReadResult.fromJson(bindingDiscoveryJson());
    expect(resolved.state, BindingReadState.resolved);
    expect(resolved.discovery!.data!.series.single.samples.single.value, 0);
    expect(
      BindingReadResult.fromJson(bindingDiscoveryJson(state: 'missing', candidates: 0)).state,
      BindingReadState.missing,
    );
  });

  test('partial warnings limits and nonfinite samples remain typed; noData stays empty rather than zero', () {
    final partial = BindingReadResult.fromJson(bindingPanelJson(state: 'partial', partial: true, limited: true));
    expect(partial.state, BindingReadState.partial);
    expect(partial.limited, true);
    expect(partial.queries.first.outcome!.warnings, ['Synthetic metric warning']);
    expect(partial.queries.first.outcome!.data!.series.single.samples.map((s) => s.kind), [
      SampleKind.finite,
      SampleKind.nan,
      SampleKind.positiveInfinity,
    ]);
    final empty = BindingReadResult.fromJson(bindingPanelJson(state: 'noData'));
    expect(empty.queries.every((q) => q.outcome!.data!.empty), true);
  });

  test('unknown panel metadata, units, semantics, states and out-of-range resolution fail closed', () {
    final base = bindingPanelJson();
    final wrongUnit = bindingPanelJson();
    ((wrongUnit['queries'] as List).first as Json)['unit'] = 'cpuCores';
    final wrongSemantics = bindingPanelJson();
    ((wrongSemantics['queries'] as List).first as Json)['semantics'] = 'exporter.supplied.authority';
    final wrongPanel = bindingPanelJson()..['panel'] = 'cpu';
    final wrongIndex = bindingDiscoveryJson()..['resolution'] = {'state': 'resolved', 'candidateIndex': 8};
    for (final row in [
      wrongUnit,
      wrongSemantics,
      wrongPanel,
      wrongIndex,
      {...base, 'state': 'futureState'},
      {...base, 'futureAuthority': true},
    ]) {
      expect(
        () => BindingReadResult.fromJson(row),
        throwsA(isA<MonitoringFailure>().having((e) => e.kind, 'kind', MonitoringErrorCode.invalidResponse)),
      );
    }
  });
}
