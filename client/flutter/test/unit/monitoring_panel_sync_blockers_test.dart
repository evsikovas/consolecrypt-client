import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_saved_panel_models.dart';
import 'package:flutter_test/flutter_test.dart';

import '../helpers/monitoring_sync_fixture.dart';

Json blockers() {
  final value = syncSnapshot();
  value['releasedMinimumClientVersion'] = '2.0.0';
  value['readiness'] = {
    'state': 'devicesUnsupported',
    'checkedActiveDevices': 4,
    'blockedDevices': [
      for (var i = 0; i < 4; i++)
        {
          'deviceId': '00000000-0000-4000-8000-${(i + 1).toString().padLeft(12, '0')}',
          'reason': PanelSyncBlockerReason.values[i].name,
        },
    ],
  };
  return value;
}

void main() {
  test('Native blocker identity and each fixed reason stay immutable instead of becoming a count only', () {
    final raw = blockers();
    final value = MonitoringPanelSyncSnapshot.fromJson(raw);
    expect(value.blockedDevices, 4);
    expect(value.deviceBlockers.map((v) => v.reason), PanelSyncBlockerReason.values);
    final first = value.deviceBlockers.first.deviceId;
    ((raw['readiness'] as Json)['blockedDevices'] as List).clear();
    expect(value.deviceBlockers.first.deviceId, first);
    expect(() => value.deviceBlockers.clear(), throwsUnsupportedError);
    expect(value.readiness, PanelSyncReadiness.devicesUnsupported);
  });
  test('unknown and duplicate device reasons cannot be reinterpreted as compatibility', () {
    for (final edit in ['reason', 'duplicate', 'count']) {
      final raw = blockers(), readiness = raw['readiness'] as Json, rows = readiness['blockedDevices'] as List;
      switch (edit) {
        case 'reason':
          (rows.first as Json)['reason'] = 'futureCompatible';
        case 'duplicate':
          (rows.last as Json)['deviceId'] = (rows.first as Json)['deviceId'];
        case 'count':
          readiness['checkedActiveDevices'] = 3;
      }
      expect(() => MonitoringPanelSyncSnapshot.fromJson(raw), throwsA(isA<MonitoringFailure>()));
    }
  });
  test('original nonce is opaque exact UTF8 bounded data and does not establish scope authority', () {
    final nonce = 'original unlock ${String.fromCharCode(0x0430) * 100}';
    final raw = syncSnapshot(nonce: nonce);
    final value = MonitoringPanelSyncSnapshot.fromJson(raw);
    expect(value.notificationScopeId, nonce);
    expect(value.matches(syncProfile, nonce), isTrue);
    expect(value.matches(syncProfile, '$nonce '), isFalse);
    for (final invalid in ['', String.fromCharCode(0x0430) * 129, 'old\nnew']) {
      expect(
        () => MonitoringPanelSyncSnapshot.fromJson(syncSnapshot(nonce: invalid)),
        throwsA(isA<MonitoringFailure>()),
      );
    }
    expect(value.releasedFloor, isNull);
    expect(value.readiness, PanelSyncReadiness.floorUndeclared);
  });
}
