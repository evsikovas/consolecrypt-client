import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/bridge/rust_monitoring.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_saved_panel_models.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';
import 'package:consolecrypt/src/rust/api/error.dart';
import 'package:consolecrypt/src/rust/frb_generated.dart';
import 'package:flutter_test/flutter_test.dart';

import '../helpers/monitoring_fakes.dart';
import '../helpers/monitoring_saved_panel_fakes.dart';

class _Native implements RustLibApi {
  final mutations = <(String, Json)>[];
  final reads = <Json>[], cancelled = <String>[];
  List<Json> panels = [savedPanelRow()];
  int revision = 2;
  BridgeError? failure;
  Json? response;
  Completer<String>? pending;
  String catalog() => jsonEncode(savedPanelCatalog(revision: revision, panels: panels));
  @override
  Future<String> crateApiMonitoringMonitoringSources() async {
    if (failure case final error?) throw error;
    return catalog();
  }

  Future<String> mutate(String kind, String raw) async {
    final request = Map<String, dynamic>.from(jsonDecode(raw) as Map);
    mutations.add((kind, request));
    if (failure case final error?) throw error;
    revision++;
    panels = kind == 'delete'
        ? []
        : [
            {
              'id': savedPanelId,
              'revision': kind == 'create' ? 1 : (request['expectedPanelRevision'] as int) + 1,
              'definition': request['definition'],
              'state': 'current',
            },
          ];
    return catalog();
  }

  @override
  Future<String> crateApiMonitoringMonitoringPanelCreate({required String requestJson}) =>
      mutate('create', requestJson);
  @override
  Future<String> crateApiMonitoringMonitoringPanelUpdate({required String requestJson}) =>
      mutate('update', requestJson);
  @override
  Future<String> crateApiMonitoringMonitoringPanelDelete({required String requestJson}) =>
      mutate('delete', requestJson);
  @override
  Future<String> crateApiMonitoringMonitoringRead({required String requestJson}) async {
    reads.add(Map<String, dynamic>.from(jsonDecode(requestJson) as Map));
    if (failure case final error?) throw error;
    return pending?.future ??
        jsonEncode(response ?? {'sourceId': monitoringFixture.id, 'sourceRevision': 1, 'result': savedReadEnvelope()});
  }

  @override
  Future<void> crateApiMonitoringMonitoringCancel({required String operationId}) async {
    cancelled.add(operationId);
  }

  @override
  dynamic noSuchMethod(Invocation invocation) =>
      throw UnsupportedError('Unexpected mock Native call ${invocation.memberName}');
}

void main() {
  late _Native api;
  setUp(() {
    api = _Native();
    RustLib.initMock(api: api);
  });
  tearDown(RustLib.dispose);
  test(
    'actual generated Native panel CRUD routes exact CAS JSON and uses returned identities in the same catalog',
    () async {
      api.panels = [];
      final store = const RustMonitoringBackend().createSourceStore() as AppCoreMonitoringSourceStore;
      await store.load();
      final created = await store.savePanel(savedDefinition);
      expect(created.id, savedPanelId);
      expect(created.revision, 1);
      expect(api.mutations.single.$1, 'create');
      expect(api.mutations.single.$2, {'expectedCatalogRevision': 2, 'definition': savedDefinition.toJson()});
      final edited = await store.savePanel(savedDefinition, previous: created);
      expect(edited.revision, 2);
      expect(api.mutations.last.$1, 'update');
      expect(api.mutations.last.$2, {
        'expectedCatalogRevision': 3,
        'definition': savedDefinition.toJson(),
        'id': savedPanelId,
        'expectedPanelRevision': 1,
      });
      await store.deletePanel(edited);
      expect(api.mutations.last.$1, 'delete');
      expect(api.mutations.last.$2, {'id': savedPanelId, 'expectedCatalogRevision': 4, 'expectedPanelRevision': 2});
      expect(store.panels, isEmpty);
      expect(store.readOnly, isFalse);
      expect(api.reads, isEmpty);
    },
  );
  test(
    'actual Native saved read sends only source/panel identity and selected range, and cancels its own operation UUID',
    () async {
      final service = const RustMonitoringBackend().createService() as AppCoreMonitoringService;
      final value = await service.readSavedPanel(monitoringFixture, savedPanel);
      expect(value.unitHint, SavedPanelUnitHint.seconds);
      expect(api.reads.single.keys, unorderedEquals(['operationId', 'sourceId', 'sourceRevision', 'action']));
      expect(api.reads.single['sourceId'], monitoringFixture.id);
      expect(api.reads.single['sourceRevision'], 1);
      expect(api.reads.single['action'], {'kind': 'savedPanel', 'panelId': savedPanelId, 'panelRevision': 1});
      api.pending = Completer<String>();
      final future = service.readSavedPanel(
        monitoringFixture,
        savedPanel,
        window: const SavedPanelRange(start: 0, end: 1000, stepSeconds: 15),
      );
      await Future<void>.value();
      expect(api.reads.last['action'], {
        'kind': 'savedPanel',
        'panelId': savedPanelId,
        'panelRevision': 1,
        'range': {'start': 0.0, 'end': 1000.0, 'stepSeconds': 15.0},
      });
      expect(jsonEncode(api.reads), isNot(contains(savedDefinition.query)));
      service.cancel();
      await Future<void>.value();
      expect(api.cancelled, [api.reads.last['operationId']]);
      api.pending!.complete(
        jsonEncode({'sourceId': monitoringFixture.id, 'sourceRevision': 1, 'result': savedReadEnvelope()}),
      );
      await future;
    },
  );
  test(
    'real bridge scope failures stay typed and mismatched saved response never falls back to an expression query',
    () async {
      final backend = const RustMonitoringBackend(),
          store = backend.createSourceStore() as AppCoreMonitoringSourceStore;
      await store.load();
      api.failure = const BridgeError(code: 'vault_locked', message: 'fixture native diagnostic', details: {});
      await expectLater(
        store.savePanel(savedDefinition),
        throwsA(isA<AppException>().having((e) => e.reason, 'reason', AppErrorReason.vaultLocked)),
      );
      expect(api.mutations.length, 1);
      expect(api.reads, isEmpty);
      api.failure = null;
      api.response = {
        'sourceId': monitoringFixture.id,
        'sourceRevision': 1,
        'result': {...savedReadEnvelope(), 'panelRevision': 2},
      };
      final service = backend.createService() as AppCoreMonitoringService;
      await expectLater(
        service.readSavedPanel(monitoringFixture, savedPanel),
        throwsA(isA<MonitoringFailure>().having((e) => e.kind, 'kind', MonitoringErrorCode.invalidResponse)),
      );
      expect(api.reads.length, 1);
      expect((api.reads.single['action'] as Map)['kind'], 'savedPanel');
    },
  );
}
