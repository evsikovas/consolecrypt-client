import 'dart:convert';

import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_association_wire.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/agent_association_fixture.dart' as f;
import '../support/agent_revocation_fixture.dart' as revoke;

Map<String, dynamic> record({String epoch = '1', String rotation = 'notRequested', String state = 'associated'}) => {
  'associationId': f.associationId,
  'hostId': f.hostId,
  'hostRevision': '1',
  'agentId': f.nodeId,
  'nodeKeyFingerprint': 'a' * 64,
  'certificateDigest': 'b' * 64,
  'certificateExpiresUtc': '2026-11-15T12:00:00Z',
  'state': state,
  'revocationState': state == 'revoked' ? 'observedRevoked' : 'notRequested',
  'remoteRevocationObserved': state == 'revoked',
  'keyEpoch': epoch,
  'rotationState': rotation,
};

AgentAssociationRecord parse(Map<String, dynamic> row) {
  final body = f.statusWire();
  body['associations'] = [row];
  return parseAgentAssociationStatus(jsonEncode(body), f.scope).records.single;
}

void main() {
  test('current Native status preserves epoch and rotation metadata', () {
    final old = parse(record());
    expect(old.nodeKeyEpoch, '1');
    expect(old.rotationState, 'notRequested');
    final selected = parse(record(epoch: '2', rotation: 'selectedObserved'));
    expect(selected.nodeKeyEpoch, '2');
    expect(selected.phase, AgentAssociationPhase.associated);
    expect(parse(record(epoch: '2', rotation: 'selectedObserved', state: 'revoked')).phase,
        AgentAssociationPhase.revoked);
    expect(parse(record(rotation: 'unknown', state: 'blocked')).rotationState, 'unknown');
  });
  test('legacy status has no invented epoch or rotation result', () {
    final row = record()..remove('keyEpoch')..remove('rotationState');
    final legacy = parse(row);
    expect(legacy.nodeKeyEpoch, isNull);
    expect(legacy.rotationState, isNull);
  });
  test('revocation review remains bound to the selected epoch two status', () {
    final row = revoke.rowWire()
      ..['keyEpoch'] = '2'
      ..['rotationState'] = 'selectedObserved';
    final original = parse(row);
    final review = revoke.wire();
    expect(() => parseAgentRevocationDetails(jsonEncode(review), f.scope, original),
        throwsFormatException);
    (review['node'] as Map<String, dynamic>)['keyEpoch'] = '2';
    expect(parseAgentRevocationDetails(jsonEncode(review), f.scope, original).nodeKeyEpoch, '2');
  });
  test('partial, unknown and contradictory rotation tuples remain unavailable', () {
    for (final row in [
      record()..remove('keyEpoch'),
      record()..remove('rotationState'),
      record(epoch: '3', rotation: 'selectedObserved'),
      record(epoch: '1', rotation: 'selectedObserved'),
      record(epoch: '2', rotation: 'notRequested'),
      record(rotation: 'unknown'),
      record(rotation: 'future'),
      record()..['callerGrant'] = true,
    ]) {
      expect(() => parse(row), throwsFormatException);
    }
  });
}
