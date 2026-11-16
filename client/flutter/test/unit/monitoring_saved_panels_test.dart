import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_saved_panel_models.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';
import 'package:flutter_test/flutter_test.dart';

import '../helpers/monitoring_fakes.dart';
import '../helpers/monitoring_saved_panel_fakes.dart';

void main() {
  test('strict bounded definitions preserve exact query and explicit unknown user hint', () {
    final json = savedDefinition.toJson();
    json['query'] = '  up\n';
    json['unitHint'] = 'unknown';
    final definition = SavedPanelDefinition.fromJson(json);
    expect(definition.query, '  up\n');
    expect(definition.unitHint, SavedPanelUnitHint.unknown);
    for (final mutation in <Json>[
      {...json, 'endpoint': 'http://untrusted.invalid'},
      {...json, 'unitHint': 'cpuCores'},
      {...json, 'query': 'q' * 16385},
      {...json, 'query': 'é' * 8193},
      {...json, 'query': 'a\u0000'},
      {...json, 'title': 'x' * 121},
      {...json, 'sourceRevision': 0},
      {...json, 'sourceId': 'caller-id'},
    ]) {
      expect(() => SavedPanelDefinition.fromJson(mutation), throwsA(anything));
    }
  });
  test('catalog v3 uses same source cache and Native assigned panel IDs for exact CAS create update delete', () async {
    final calls = <(String, Json)>[];
    final store = AppCoreMonitoringSourceStore((method, args) async {
      calls.add((method, args));
      return switch (method) {
        'monitoring_sources' => savedPanelCatalog(version: 2, revision: 1),
        'monitoring_panel_create' => savedPanelCatalog(),
        'monitoring_panel_update' => savedPanelCatalog(revision: 3, panels: [savedPanelRow(revision: 2)]),
        'monitoring_panel_delete' => savedPanelCatalog(revision: 4, panels: []),
        _ => throw StateError('unexpected fixture RPC'),
      };
    });
    await store.load();
    expect(store.panels, isEmpty);
    final created = await store.savePanel(savedDefinition);
    expect(created.id, savedPanelId);
    expect(jsonDecode(calls[1].$2['requestJson'] as String), {
      'expectedCatalogRevision': 1,
      'definition': savedDefinition.toJson(),
    });
    final updated = await store.savePanel(savedDefinition, previous: created);
    expect(jsonDecode(calls[2].$2['requestJson'] as String), {
      'expectedCatalogRevision': 2,
      'id': savedPanelId,
      'expectedPanelRevision': 1,
      'definition': savedDefinition.toJson(),
    });
    expect(updated.revision, 2);
    await store.deletePanel(updated);
    expect(jsonDecode(calls[3].$2['requestJson'] as String), {
      'expectedCatalogRevision': 3,
      'id': savedPanelId,
      'expectedPanelRevision': 2,
    });
    expect(store.panels, isEmpty);
  });
  test('source is protected while referenced by panel; source writes retain shared panels', () async {
    final calls = <String>[];
    final store = AppCoreMonitoringSourceStore((method, args) async {
      calls.add(method);
      return savedPanelCatalog();
    });
    await store.load();
    await expectLater(store.delete(monitoringFixture), throwsA(isA<MonitoringFailure>()));
    expect(calls, ['monitoring_sources']);
    await store.upsert(monitoringFixture);
    expect(store.panels.single.id, savedPanelId);
  });
  test('future malformed duplicate and overquota panels invalidate cache and block all writes', () async {
    for (final json in <Json>[
      {...savedPanelCatalog(), 'schemaVersion': 4},
      {...savedPanelCatalog(version: 2), 'panels': <Json>[]},
      {...savedPanelCatalog(), 'panels': null},
      {...savedPanelCatalog(), 'extra': true},
      savedPanelCatalog(panels: [savedPanelRow(), savedPanelRow()]),
      savedPanelCatalog(panels: List.generate(65, (_) => savedPanelRow())),
      savedPanelCatalog(
        panels: [
          {...savedPanelRow(), 'future': true},
        ],
      ),
    ]) {
      var calls = 0;
      final store = AppCoreMonitoringSourceStore((method, args) async {
        calls++;
        return json;
      });
      await expectLater(store.load(), throwsA(isA<MonitoringFailure>()));
      expect(store.readOnly, isTrue);
      expect(store.panels, isEmpty);
      await expectLater(store.savePanel(savedDefinition), throwsA(isA<MonitoringFailure>()));
      expect(calls, 1);
    }
  });
  test('saved read emits only identity and exact selected window; no caller query fallback', () async {
    Json? request;
    final service = AppCoreMonitoringService(
      read: (json) async {
        request = json;
        return {'sourceId': monitoringFixture.id, 'sourceRevision': 1, 'result': savedReadEnvelope()};
      },
      cancelOperation: (_) async {},
    );
    final read = await service.readSavedPanel(
      monitoringFixture,
      savedPanel,
      window: const SavedPanelRange(start: 1000, end: 2000, stepSeconds: 15),
    );
    expect(request!['action'], {
      'kind': 'savedPanel',
      'panelId': savedPanelId,
      'panelRevision': 1,
      'range': {'start': 1000.0, 'end': 2000.0, 'stepSeconds': 15.0},
    });
    expect(read.unitHint, SavedPanelUnitHint.seconds);
    expect(read.outcome.partial, isTrue);
    expect(read.outcome.data!.series.single.samples.single.kind, SampleKind.nan);
    await service.readSavedPanel(monitoringFixture, savedPanel);
    expect(object(request!['action']).containsKey('range'), isFalse);
  });
  test('stale source or invalid window is rejected before transport', () async {
    var reads = 0;
    final service = AppCoreMonitoringService(
      read: (_) async {
        reads++;
        return {};
      },
      cancelOperation: (_) async {},
    );
    await expectLater(
      service.readSavedPanel(monitoringFixture.withRecord(revision: 2), savedPanel),
      throwsA(isA<MonitoringFailure>()),
    );
    for (final range in [
      const SavedPanelRange(start: 0, end: double.infinity, stepSeconds: 1),
      const SavedPanelRange(start: 2, end: 1, stepSeconds: 1),
      const SavedPanelRange(start: 0, end: 1, stepSeconds: 0),
    ]) {
      await expectLater(
        service.readSavedPanel(monitoringFixture, savedPanel, window: range),
        throwsA(isA<MonitoringFailure>()),
      );
    }
    expect(reads, 0);
  });
  test('result identity revision and unit metadata are verified without converting samples', () {
    for (final mutation in <Json>[
      {...savedReadEnvelope(), 'panelId': 'different'},
      {...savedReadEnvelope(), 'panelRevision': 2},
      {...savedReadEnvelope(), 'unitHint': 'percent'},
      {...savedReadEnvelope(), 'query': 'evil'},
    ]) {
      expect(() => SavedPanelRead.fromJson(mutation, savedPanel), throwsA(anything));
    }
    expect(
      SavedPanelRead.fromJson(savedReadEnvelope(), savedPanel).outcome.data!.series.single.samples.single.kind,
      SampleKind.nan,
    );
  });
  test('saved read shares the existing pending operation cancellation owner', () async {
    final pending = Completer<Json>(), cancelled = <String>[];
    final service = AppCoreMonitoringService(
      read: (_) => pending.future,
      cancelOperation: (id) async {
        cancelled.add(id);
      },
    );
    final result = service.readSavedPanel(monitoringFixture, savedPanel);
    service.cancel();
    expect(cancelled, hasLength(1));
    pending.complete({'sourceId': monitoringFixture.id, 'sourceRevision': 1, 'result': savedReadEnvelope()});
    await result;
    service.cancel();
    expect(cancelled, hasLength(1));
  });
}
