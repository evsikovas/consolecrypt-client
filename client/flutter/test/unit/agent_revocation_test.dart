import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_association_wire.dart';
import 'package:consolecrypt/monitoring/agent_revocation_controller.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/agent_revocation_fixture.dart' as f;

void main() {
  test('closed revoke details bind original frame record certificate and exact u64 metadata', () {
    final row = f.record(), raw = f.wire()..['actorRevision'] = '18446744073709551615';
    final v = parseAgentRevocationDetails(jsonEncode(raw), f.scope, row);
    expect(v.actorRevision, '18446744073709551615');
    expect(v.localBlocked, false);
    expect(v.remoteRevocationObserved, false);
    for (final edit in <void Function(Map<String, dynamic>)>[
      (m) => m['version'] = 1.0,
      (m) => m['notificationScopeId'] = 'other',
      (m) => m['hostRevision'] = '1',
      (m) => m['certificateDigest'] = 'd' * 64,
      (m) => m['actorAdmission'] = true,
      (m) => m['remoteRevocationObserved'] = true,
      (m) => m['phase'] = 'revoked',
      (m) => m['remainingMs'] = 0,
      (m) => m['actorRevision'] = '18446744073709551616',
      (m) => m['token'] = 'x',
    ]) {
      final changed = f.wire();
      edit(changed);
      expect(() => parseAgentRevocationDetails(jsonEncode(changed), f.scope, row), throwsFormatException);
    }
  });
  test('status supports exact legacy rows and paired revoke projection without inferring success', () {
    final row = f.rowWire();
    final status = f.capturedStatus();
    expect(status.records.single.remoteRevocationObserved, false);
    final raw = {
      'version': 1,
      'profileId': f.scope.profileId,
      'notificationScopeId': f.scope.nonce,
      'hosts': <Object>[],
      'clientIdentity': {'state': 'missing', 'identityId': null, 'certificateSha256': null},
      'associations': [row],
    };
    row.remove('remoteRevocationObserved');
    expect(() => parseAgentAssociationStatus(jsonEncode(raw), f.scope), throwsFormatException);
    row.remove('revocationState');
    expect(parseAgentAssociationStatus(jsonEncode(raw), f.scope).records.single.remoteRevocationObserved, isNull);
    row.addAll({'state': 'revoked', 'remoteRevocationObserved': true, 'revocationState': 'observedRevoked'});
    expect(parseAgentAssociationStatus(jsonEncode(raw), f.scope).records.single.phase, AgentAssociationPhase.revoked);
    row['certificateDigest'] = null;
    row['certificateExpiresUtc'] = null;
    expect(() => parseAgentAssociationStatus(jsonEncode(raw), f.scope), throwsFormatException);
    row['certificateDigest'] = 'c' * 64;
    row['certificateExpiresUtc'] = '2026-11-15T12:00:00Z';
    row['remoteRevocationObserved'] = false;
    expect(() => parseAgentAssociationStatus(jsonEncode(raw), f.scope), throwsFormatException);
  });
  test('cancel retains issued handle before delayed details and never publishes late review', () async {
    final p = f.Port()..issued.pendingDetails = Completer();
    final c = AgentRevocationController(port: p, scope: f.scope, record: f.record(), original: () => true);
    final task = c.prepare();
    await Future<void>.delayed(Duration.zero);
    expect(p.issued.detailsCalls, 1);
    c.cancel();
    expect(p.issued.cancels, 1);
    expect(p.issued.joins, 1);
    expect(c.canConfirm, false);
    p.issued.pendingDetails!.complete(p.issued.value());
    await task;
    expect(c.details, isNull);
    p.issued.finishCleanup();
    await Future<void>.delayed(Duration.zero);
    c.dispose();
  });
  test('unknown confirmation is spent and joined observation never upgrades result to success', () async {
    final p = f.Port()..issued.failConfirm = true;
    final c = AgentRevocationController(port: p, scope: f.scope, record: f.record(), original: () => true);
    await c.prepare();
    await c.confirm();
    await c.confirm();
    expect(p.issued.confirms, 1);
    expect(c.unknown, true);
    expect(c.result, isNull);
    p.issued.finishCleanup(observed: true);
    await Future<void>.delayed(Duration.zero);
    expect(c.result, isNull);
    expect(c.unknown, true);
    c.dispose();
  });
  test('late registration on retired original scope is cancelled and joined without details', () async {
    var original = true;
    final p = f.Port()..pendingPrepare = Completer();
    final c = AgentRevocationController(port: p, scope: f.scope, record: f.record(), original: () => original);
    final task = c.prepare();
    original = false;
    c.retire();
    p.pendingPrepare!.complete(p.issued);
    await task;
    expect(p.issued.cancels, 1);
    expect(p.issued.joins, 1);
    expect(p.issued.detailsCalls, 0);
    expect(c.details, isNull);
    p.issued.finishCleanup();
    c.dispose();
  });
  test('cancellation before prepare is sticky and performs zero Native calls', () async {
    final p = f.Port();
    final c = AgentRevocationController(port: p, scope: f.scope, record: f.record(), original: () => true);
    c.cancel();
    await c.prepare();
    await c.confirm();
    expect(p.prepares, 0);
    expect(p.issued.detailsCalls, 0);
    expect(p.issued.confirms, 0);
    c.dispose();
  });
}
