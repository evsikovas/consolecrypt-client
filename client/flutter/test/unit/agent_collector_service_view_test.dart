import 'dart:convert';

import 'package:consolecrypt/monitoring/agent_collector_service_view.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/agent_association_fixture.dart' as a;
import '../support/agent_collector_service_fixture.dart' as f;
import '../support/agent_metrics_fixture.dart' as m;

void main() {
  test('one original60s clock cannot be reset by details and policy expiry never moves with UI time', () {
    var elapsed = Duration.zero;
    final clock = AgentCollectorServiceReviewClock(elapsed: () => elapsed), record = m.metricRecord();
    final before = parseAgentCollectorServiceView(jsonEncode(f.serviceWire()), a.scope, record);
    clock.acceptNativeRemaining(before.remainingMs);
    elapsed = const Duration(seconds: 10);
    clock.acceptNativeRemaining(60000);
    expect(clock.remainingMs, 50000);
    elapsed = const Duration(seconds: 20);
    clock.acceptNativeRemaining(5000);
    expect(clock.remainingMs, 5000);
    elapsed = const Duration(seconds: 24);
    clock.acceptNativeRemaining(60000);
    expect(clock.remainingMs, 1000);
    elapsed = const Duration(seconds: 25);
    expect(clock.remainingMs, 0);
    final recorded = parseAgentCollectorServiceView(
      jsonEncode(f.serviceWire(phase: 'configurationRecorded')),
      a.scope,
      record,
    );
    expect(before.sameOriginal(recorded), isTrue);
    expect(before.expiresAtUtc, DateTime.utc(2030, 1, 2));
    elapsed = const Duration(days: 1);
    expect(clock.remainingMs, 0);
    expect(before.expiresAtUtc, recorded.expiresAtUtc);
  });
  test('actual closed producer has five service phases and configuration is never observed runtime', () {
    final record = m.metricRecord();
    for (final phase in AgentCollectorServicePhase.values) {
      final view = parseAgentCollectorServiceView(jsonEncode(f.serviceWire(phase: phase.name)), a.scope, record);
      expect(view.phase, phase);
      expect(view.runtimeObserved, isFalse);
      expect(view.deliveryGuaranteed, isFalse);
      expect(view.continuesWhenClientLocks, isTrue);
      expect(view.catalog.manifest.persistentDisk, isTrue);
      expect(view.configurationRecorded, phase == AgentCollectorServicePhase.configurationRecorded);
      expect(view.bootStamp != null, view.configurationRecorded);
      expect(view.configurationAck != null, view.configurationRecorded);
      expect(view.effectiveAuthoritySeconds, 86400);
      expect(view.catalog.manifest.maxBufferAgeSeconds, 7200);
      expect(view.remainingMs, 60000);
    }
  });
  test('bootStamp and all revision u64s preserve exact decimal display above JS integer precision', () {
    final raw = f.serviceWire(phase: 'configurationRecorded');
    raw['ownerEpoch'] = '18446744073709551615';
    raw['actorRevision'] = '18446744073709551615';
    raw['bootStamp'] = {
      'bootId': '90000000-0000-4000-8000-000000000009',
      'createdBoottimeNs': '18446744073709500000',
      'deadlineBoottimeNs': '18446744073709551615',
    };
    raw['configurationAck'] = {'actorRevision': '18446744073709551615', 'recordedAtUtc': '2030-01-01T00:00:01Z'};
    final view = parseAgentCollectorServiceView(jsonEncode(raw), a.scope, m.metricRecord());
    expect(view.ownerEpoch, '18446744073709551615');
    expect(view.actorRevision, '18446744073709551615');
    expect(view.bootStamp!.deadlineBoottimeNs, '18446744073709551615');
    expect(view.bootStamp!.createdBoottimeNs, '18446744073709500000');
  });
  test('service review denies foreign original frame/Host and old interactive session fields', () {
    final cases = [
      {...f.serviceWire(), 'version': 1.0},
      {...f.serviceWire(), 'notificationScopeId': 'foreign'},
      {...f.serviceWire(), 'hostId': '10000000-0000-4000-8000-000000000099'},
      {...f.serviceWire(), 'hostRevision': '2'},
      {...f.serviceWire(), 'scope': 'tunnel.open'},
      {...f.serviceWire(), 'actorAdmission': true},
      {...f.serviceWire(), 'crashReplay': false},
      {...f.serviceWire(), 'runtimeState': 'running'},
      {...f.serviceWire(), 'automaticRenewal': true},
      {...f.serviceWire(), 'continuesWhenClientLocks': false},
      {...f.serviceWire(), 'maxStarts': 33},
      {...f.serviceWire(), 'maxAuthoritySeconds': 300},
      {...f.serviceWire(), 'remainingMs': 60001},
      {...f.serviceWire(), 'effectiveAuthoritySeconds': 7200},
    ];
    for (final raw in cases) {
      expect(() => parseAgentCollectorServiceView(jsonEncode(raw), a.scope, m.metricRecord()), throwsFormatException);
    }
  });
  test(
    'paired signed projection cannot become runtime or exist outside configurationRecorded; malformed clocks deny',
    () {
      final configured = f.serviceWire(phase: 'configurationRecorded');
      final cases = [
        {...configured, 'configurationAck': null},
        {...configured, 'bootStamp': null},
        {...configured, 'phase': 'unknown'},
        {
          ...configured,
          'configurationAck': {'actorRevision': '1', 'recordedAtUtc': '2030-01-01T00:00:01Z'},
        },
        {
          ...configured,
          'bootStamp': {
            'bootId': '90000000-0000-4000-8000-000000000009',
            'createdBoottimeNs': 9007199254740993,
            'deadlineBoottimeNs': '9093599254740993',
          },
        },
        {
          ...configured,
          'bootStamp': {
            'bootId': '90000000-0000-4000-8000-000000000009',
            'createdBoottimeNs': '0',
            'deadlineBoottimeNs': '1',
          },
        },
        {
          ...configured,
          'bootStamp': {
            'bootId': '90000000-0000-4000-8000-000000000009',
            'createdBoottimeNs': '1',
            'deadlineBoottimeNs': '86400000000002',
          },
        },
      ];
      for (final raw in cases) {
        expect(() => parseAgentCollectorServiceView(jsonEncode(raw), a.scope, m.metricRecord()), throwsFormatException);
      }
    },
  );
}
