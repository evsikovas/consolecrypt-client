import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/monitoring/agent_rotation.dart';
import 'package:consolecrypt/monitoring/agent_rotation_controller.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/agent_rotation_fake.dart';
import '../support/agent_rotation_fixture.dart' as f;

void main() {
  test('two explicit stages consume same issued handle once and only exact readback publishes selected', () async {
    final scope = f.scope(), record = f.record(), handle = RotationHandle(scope, record), port = RotationPort(handle);
    final c = AgentRotationController(
      port: port,
      scope: scope,
      record: record,
      mode: AgentRotationMode.rotate,
      original: () => true,
    );
    addTearDown(c.dispose);
    expect(port.normal, 0);
    await c.prepare();
    expect(c.canConfirm, false);
    expect(c.canPrepareCandidate, true);
    await c.prepareCandidate();
    expect(handle.candidates, 1);
    expect(handle.confirms, 0);
    expect(c.canConfirm, true);
    await c.confirm();
    await c.confirm();
    await c.prepareCandidate();
    await c.prepare();
    expect(c.result?.selectionObserved, true);
    expect(c.result?.node?.keyEpoch, '2');
    expect(c.result?.localBlocked, false);
    expect(port.normal, 1);
    expect(handle.candidates, 1);
    expect(handle.confirms, 1);
  });
  test('reconcile uses separate factory and never creates a new candidate', () async {
    final scope = f.scope(),
        record = f.record(reconcile: true),
        h = RotationHandle(scope, record, reconcile: true),
        p = RotationPort(h);
    final c = AgentRotationController(
      port: p,
      scope: scope,
      record: record,
      mode: AgentRotationMode.reconcile,
      original: () => true,
    );
    addTearDown(c.dispose);
    await c.prepare();
    expect(c.canPrepareCandidate, false);
    await c.prepareCandidate();
    await c.confirm();
    expect(p.normal, 0);
    expect(p.reconciles, 1);
    expect(h.candidates, 0);
    expect(h.confirms, 1);
    expect(c.result?.selectionObserved, true);
  });
  test('cancel retains exact issued pending-details handle and late metadata cannot resurrect it', () async {
    final s = f.scope(),
        r = f.record(),
        h = RotationHandle(s, r)..pendingDetails = Completer<AgentRotationDetails>(),
        p = RotationPort(h);
    final c = AgentRotationController(
      port: p,
      scope: s,
      record: r,
      mode: AgentRotationMode.rotate,
      original: () => true,
    );
    addTearDown(c.dispose);
    final pending = c.prepare();
    await Future<void>.delayed(Duration.zero);
    c.cancel();
    expect(h.cancels, 1);
    h.pendingDetails!.complete(f.parsed(s, r));
    await pending;
    await c.prepare();
    expect(c.details, isNull);
    expect(c.canConfirm, false);
    expect(p.normal, 1);
    expect(h.joins, 1);
  });
  test('late issued factory after owner retirement is cancelled and joined without publication', () async {
    final s = f.scope(),
        r = f.record(),
        h = RotationHandle(s, r),
        p = RotationPort(h)..pending = Completer<AgentRotationHandle>();
    var current = true;
    final c = AgentRotationController(
      port: p,
      scope: s,
      record: r,
      mode: AgentRotationMode.rotate,
      original: () => current,
    );
    addTearDown(c.dispose);
    final pending = c.prepare();
    current = false;
    c.retire();
    p.pending!.complete(h);
    await pending;
    expect(h.cancels, 1);
    expect(h.joins, 1);
    expect(h.reads, 0);
    expect(c.details, isNull);
  });
  test('candidate response cannot renew original TTL and expired confirm performs no Native call', () async {
    var elapsed = Duration.zero;
    final s = f.scope(), r = f.record(), h = RotationHandle(s, r)..candidateRemaining = 60000, p = RotationPort(h);
    final c = AgentRotationController(
      port: p,
      scope: s,
      record: r,
      mode: AgentRotationMode.rotate,
      original: () => true,
      elapsedForReview: () => elapsed,
    );
    addTearDown(c.dispose);
    await c.prepare();
    elapsed = const Duration(seconds: 30);
    await c.prepareCandidate();
    expect(c.remainingMs, 30000);
    elapsed = const Duration(seconds: 60);
    await c.confirm();
    expect(h.confirms, 0);
    expect(h.reads, 1);
  });
  test('candidate uncertainty is spent and never reinterpreted as selected from cleanup', () async {
    final s = f.scope(),
        r = f.record(),
        h = RotationHandle(s, r)..candidateError = Exception('synthetic lost outcome'),
        p = RotationPort(h);
    final c = AgentRotationController(
      port: p,
      scope: s,
      record: r,
      mode: AgentRotationMode.rotate,
      original: () => true,
    );
    addTearDown(c.dispose);
    await c.prepare();
    await c.prepareCandidate();
    await c.prepareCandidate();
    await c.confirm();
    expect(c.unknown, true);
    expect(c.result, isNull);
    expect(h.candidates, 1);
    expect(h.confirms, 0);
  });
  test('changed final Actor revision or proposed key is not a successful selection', () async {
    final s = f.scope(), r = f.record(), h = RotationHandle(s, r), p = RotationPort(h);
    final wrong = f.details(phase: 'rotated')..['actorRevision'] = '9007199254740995';
    h.confirmation = parseAgentRotationDetails(jsonEncode(wrong), s, r);
    final c = AgentRotationController(
      port: p,
      scope: s,
      record: r,
      mode: AgentRotationMode.rotate,
      original: () => true,
    );
    addTearDown(c.dispose);
    await c.prepare();
    await c.prepareCandidate();
    await c.confirm();
    expect(c.result, isNull);
    expect(c.unknown, true);
  });
  test('actual producer domains: rotation ID is 32hex, counters remain exact u64 decimal strings', () {
    final s = f.scope(), r = f.record();
    final raw = f.details(phase: 'rotationCertificateReview')..['actorRevision'] = '18446744073709551615';
    expect(parseAgentRotationDetails(jsonEncode(raw), s, r).actorRevision, '18446744073709551615');
    raw['rotationId'] = f.associationId;
    expect(() => parseAgentRotationDetails(jsonEncode(raw), s, r), throwsFormatException);
    raw['rotationId'] = 'a' * 32;
    raw['actorRevision'] = '18446744073709551616';
    expect(() => parseAgentRotationDetails(jsonEncode(raw), s, r), throwsFormatException);
  });
  test(
    'closed metadata rejects altered scope, float version, expiry extension, wrong epoch and observed-only claims',
    () {
      final s = f.scope(), r = f.record();
      for (final edit in <void Function(Map<String, dynamic>)>[
        (v) => v['version'] = 1.0,
        (v) => v['scope'] = 'jobs.execute',
        (v) => v['expiryExtension'] = true,
        (v) => v['actorAdmission'] = true,
        (v) => v['notificationScopeId'] = 'another nonce',
        (v) => v['remainingMs'] = 0,
        (v) => v['selectionObserved'] = true,
        (v) => (v['previousNode'] as Map)['keyEpoch'] = '2',
        (v) => v['rawProof'] = 'not accepted',
      ]) {
        final raw = f.details();
        edit(raw);
        expect(() => parseAgentRotationDetails(jsonEncode(raw), s, r), throwsFormatException);
      }
    },
  );
}
