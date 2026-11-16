import 'package:consolecrypt/monitoring/agent_metrics_manifest.dart';

import 'agent_association_fixture.dart' as a;
import 'agent_metrics_fixture.dart' as m;

/// SYNTHETIC exact shape of Core view.rs5356de98 + Main Context.publish.
/// No actual signed configuration, Actor admission, service process or runtime capture.
Map<String, dynamic> serviceWire({String phase = 'serviceReview'}) {
  final recorded = phase == 'configurationRecorded';
  return {
    'version': 1,
    'profileId': a.scope.profileId,
    'notificationScopeId': a.scope.nonce,
    'kind': 'finiteCollectorService',
    'phase': phase,
    'associationId': m.metricRecord().id,
    'hostId': m.metricRecord().hostId,
    'hostRevision': m.metricRecord().hostRevision,
    'owner': {
      'kind': 'localProfile',
      'id': a.scope.profileId,
      'deviceId': '80000000-0000-4000-8000-000000000008',
      'signingKeySha256': 'e' * 64,
    },
    'node': {
      'agentId': m.metricRecord().agentId,
      'keyEpoch': '1',
      'publicKey': 'd' * 64,
      'keyFingerprint': m.metricRecord().nodeKeyFingerprint,
    },
    'scope': 'metrics.collect',
    'serviceId': '50000000-0000-4000-8000-000000000005',
    'serviceHash': 'b' * 64,
    'collectorId': '70000000-0000-8000-8000-000000000007',
    'manifestSha256': 'f' * 64,
    'manifest': {
      ...m.manifest(),
      'profile': agentMetricsDiskProfile,
      'bufferMode': 'persistentNativeWal',
      'targets': [
        {
          'id': 'node',
          'url': 'http://127.0.0.1:9100/metrics',
          'kind': 'node_exporter',
          'sample_limit': 1000,
          'series_limit': 256,
          'response_bytes': 1048576,
          'label_limit': 16,
        },
      ],
      'buffer': {'max_bytes': 536870912, 'max_age_seconds': 7200},
    },
    'ownerEpoch': '1',
    'checkpoint': '0',
    'actorRevision': recorded ? '9007199254740993' : '9007199254740992',
    'policyRevision': '1',
    'revocationEpoch': '1',
    'profile': agentMetricsDiskProfile,
    'bufferMode': 'persistentNativeWal',
    'remainingMs': 60000,
    'issuedAtUtc': '2030-01-01T00:00:00Z',
    'expiresAtUtc': '2030-01-02T00:00:00Z',
    'effectiveAuthoritySeconds': 86400,
    'maxAuthoritySeconds': 86400,
    'maxStarts': 32,
    'maxConcurrent': 1,
    'maxBufferBytes': 536870912,
    'maxResidenceSeconds': 7200,
    'maxMemoryBytes': 536870912,
    'continuesWhenClientLocks': true,
    'automaticRenewal': false,
    'osBootPolicy': 'sameBootOnly',
    'osRebootRequiresApproval': true,
    'bootStamp': recorded
        ? {
            'bootId': '90000000-0000-4000-8000-000000000009',
            'createdBoottimeNs': '9007199254740993',
            'deadlineBoottimeNs': '9093599254740993',
          }
        : null,
    'configurationAck': recorded
        ? {'actorRevision': '9007199254740993', 'recordedAtUtc': '2030-01-01T00:00:01Z'}
        : null,
    'runtimeState': 'notObserved',
    'deliveryGuaranteed': false,
    'lossUnknown': true,
    'cleanupState': phase == 'unknown' ? 'unknown' : 'pending',
  };
}
