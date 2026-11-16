import 'dart:convert';

import 'package:consolecrypt/core/bridge/rust_agent_collector_service_status.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_status.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/agent_collector_service_fixture.dart' as v;
import '../support/agent_collector_service_flow_fixture.dart' as f;

/// SYNTHETIC actual management.rs status shape, never an actual signed readback.
Map<String, dynamic> statusWire({bool local = false, bool revoked = false}) {
  final source = v.serviceWire(phase: 'configurationRecorded');
  return {
    'version': 1,
    'profileId': f.scope.profileId,
    'notificationScopeId': f.scope.nonce,
    'kind': 'finiteCollectorServiceStatus',
    'associationId': f.record.id,
    'hostId': f.record.hostId,
    'hostRevision': f.record.hostRevision,
    'node': source['node'],
    'serviceId': source['serviceId'],
    'serviceHash': source['serviceHash'],
    'collectorId': source['collectorId'],
    'manifestSha256': source['manifestSha256'],
    'policyRevision': '1',
    'revocationEpoch': revoked ? '2' : '1',
    'actorRevision': '18446744073709551615',
    'localConfigurationRecorded': local,
    'remoteConfigurationRecorded': true,
    'remoteRevoked': revoked,
    'expiresAtUtc': source['expiresAtUtc'],
    'bootStamp': {
      'bootId': '90000000-0000-4000-8000-000000000009',
      'createdBoottimeNs': '18446744073709500000',
      'deadlineBoottimeNs': '18446744073709551615',
    },
    'runtimeState': 'notObserved',
    'deliveryGuaranteed': false,
    'lossUnknown': true,
    'cleanupState': 'closedObserved',
  };
}

void main() {
  test(
    'actual status wrapper is explicit readonly; local Unknown staysUnknown despite remote recorded/revoked, no newTTL',
    () async {
      final calls = f.Calls(f.NativeHandle()), base = calls.port, requests = <Map<String, dynamic>>[];
      final port = RustAgentCollectorServiceWithStatus(
        original: base,
        readJson: (raw) async {
          requests.add(jsonDecode(raw) as Map<String, dynamic>);
          return jsonEncode(statusWire(revoked: true));
        },
      );
      expect(requests, isEmpty);
      expect(identical(port.original, base), isTrue);
      final status = await port.readSavedStatus(f.scope, f.record);
      expect(requests.single, {
        'version': 1,
        'profileId': f.scope.profileId,
        'notificationScopeId': f.scope.nonce,
        'associationId': f.record.id,
      });
      expect(status.localConfigurationRecorded, isFalse);
      expect(status.remoteConfigurationRecorded, isTrue);
      expect(status.remoteRevoked, isTrue);
      expect(status.runtimeObserved, isFalse);
      expect(status.actorRevision, '18446744073709551615');
      expect(status.bootStamp.deadlineBoottimeNs, '18446744073709551615');
      expect(status.expiresAtUtc, DateTime.utc(2030, 1, 2));
      expect(calls.prepareRequests, isEmpty);
      expect(calls.catalogRequests, isEmpty);
    },
  );
  test(
    'strict status rejects foreignframe/node, imprecise clocks, optimisticRuntime, missingfields and unknownReady',
    () {
      final valid = statusWire();
      final cases = [
        {...valid, 'version': 1.0},
        {...valid, 'notificationScopeId': 'another'},
        {...valid, 'hostRevision': '2'},
        {...valid, 'remoteConfigurationRecorded': false},
        {...valid, 'runtimeState': 'running'},
        {...valid, 'ready': true},
        {...valid, 'actorRevision': 9007199254740993},
        {
          ...valid,
          'bootStamp': {
            'bootId': '90000000-0000-4000-8000-000000000009',
            'createdBoottimeNs': '2',
            'deadlineBoottimeNs': '1',
          },
        },
        {
          ...valid,
          'node': {...(valid['node'] as Map<String, dynamic>), 'keyEpoch': '99'},
        },
      ];
      for (final raw in cases) {
        expect(() => parseAgentCollectorServiceStatus(jsonEncode(raw), f.scope, f.record), throwsFormatException);
      }
      final historical = {...valid, 'expiresAtUtc': '2020-01-01T00:00:00Z'};
      expect(parseAgentCollectorServiceStatus(jsonEncode(historical), f.scope, f.record).runtimeObserved, isFalse);
    },
  );
}
