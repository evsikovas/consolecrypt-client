import 'dart:async';

import 'package:consolecrypt/monitoring/agent_association_controller.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/agent_association_fixture.dart' as f;

AgentAssociationController controller(
  f.Port p, {
  bool Function()? original,
  Duration Function()? elapsed,
}) => AgentAssociationController(
  port: p,
  scope: f.scope,
  original: original ?? () => true,
  elapsedForReview: elapsed,
);
Future<void> select(AgentAssociationController c) async {
  await c.readStatus();
  c.select(c.status!.hosts.single);
}

void main() {
  test('opening does no work and explicit stages never auto-approve or grant admission', () async {
    final p = f.Port(), c = controller(f.Port());
    c.dispose();
    final owner = controller(p);
    expect(p.statuses, 0);
    await select(owner);
    expect(p.issued.probes, 0);
    await owner.probe();
    expect(p.issued.offers, 0);
    await owner.approve();
    expect(owner.review!.stage, AgentAssociationReviewStage.certificate);
    expect(p.issued.certificates, 0);
    await owner.approve();
    expect(owner.result!.associationId, f.associationId);
    expect(p.registrations, 1);
    expect(p.issued.joins, 1);
    p.issued.retired.complete();
    owner.dispose();
  });
  test('cancel before registration ACK cancels and joins only late issued handle without starting SSH', () async {
    final p = f.Port()..pendingRegistration = Completer(), c = controller(p);
    await select(c);
    final operation = c.probe();
    c.cancel();
    p.pendingRegistration!.complete(p.issued);
    await operation;
    expect(p.issued.probes, 0);
    expect(p.issued.cancels, 1);
    expect(p.issued.joins, 1);
    expect(c.review, isNull);
    expect(p.issued.retired.isCompleted, isFalse);
    p.issued.retired.complete();
    c.dispose();
  });
  test(
    'pending SSH probe cancel and original owner loss discard late metadata',
    () async {
      var current = true;
      final p = f.Port()..issued.pendingProbe = Completer();
      final c = controller(p, original: () => current);
      await select(c);
      final operation = c.probe();
      await Future<void>.delayed(Duration.zero);
      expect(p.issued.probes, 1);
      current = false;
      p.issued.pendingProbe!.complete(f.review());
      await operation;
      expect(p.issued.cancels, 1);
      expect(c.review, isNull);
      expect(c.current, isFalse);
      p.issued.retired.complete();
      c.dispose();
    },
  );
  test('same original deadline is never renewed by offer ACK and expired certificate cannot dispatch', () async {
    var elapsed = Duration.zero;
    final p = f.Port(), c = controller(p, elapsed: () => elapsed);
    await select(c);
    await c.probe();
    elapsed = const Duration(seconds: 59);
    await c.approve();
    expect(c.remainingMs, 1000);
    elapsed = const Duration(seconds: 60);
    expect(c.canApprove, isFalse);
    await c.approve();
    expect(p.issued.certificates, 0);
    c.cancel();
    p.issued.retired.complete();
    c.dispose();
  });
  test(
    'ambiguous offer is sticky unknown and spent handle cannot retry',
    () async {
      final p = f.Port()..issued.failOffer = true, c = controller(p);
      await select(c);
      await c.probe();
      await c.approve();
      expect(c.unknown, isTrue);
      expect(c.review, isNull);
      expect(p.issued.cancels, 1);
      await c.approve();
      expect(p.issued.offers, 1);
      p.issued.retired.complete();
      c.dispose();
    },
  );
  test('dispose while certificate pending keeps exact cancellation and refuses late Associated', () async {
    final p = f.Port()..issued.pendingCertificate = Completer(),
        c = controller(p);
    await select(c);
    await c.probe();
    await c.approve();
    final operation = c.approve();
    c.dispose();
    p.issued.pendingCertificate!.complete(f.result());
    await operation;
    expect(p.issued.cancels, 1);
    expect(c.result, isNull);
    p.issued.retired.complete();
  });
  test('public metadata retains exact decimal domains and rejects invalid owner identity references', () {
    expect(f.review().ownerEpoch, '18446744073709551615');
    expect(f.selectedHost.revision, '9007199254740993');
    expect(
      () => AgentAssociationHost(
        id: f.hostId,
        revision: '9223372036854775808',
        name: 'bad revision',
      ),
      throwsFormatException,
    );
    expect(
      () => associationNumber('18446744073709551616'),
      throwsFormatException,
    );
    expect(() => associationNumber('01'), throwsFormatException);
  });
  test('TLS identity creation requires separate review and one-use approval without SSH or automatic status', () async {
    final p = f.Port()..identityState = 'missing', c = controller(p);
    await c.readStatus();
    expect(p.identityPrepares, 0);
    await c.prepareIdentity();
    expect(p.identity.confirms, 0);
    expect(c.identityReview, isNotNull);
    await c.createIdentity();
    expect(c.identityResult!.identityId, f.identityId);
    expect(p.registrations, 0);
    expect(p.statuses, 1);
    expect(c.cleanup, isNull);
    expect(c.awaitingCleanup, isTrue);
    await c.createIdentity();
    expect(p.identity.confirms, 1);
    p.identity.retired.complete();
    await Future<void>.delayed(Duration.zero);
    expect(c.cleanup!.state, 'closedObserved');
    c.dispose();
  });
  test('cancel during actual identity request keeps Unknown and cannot fake Stored or cleanup', () async {
    final p = f.Port()..identityState = 'missing';
    p.identity.pending = Completer();
    final c = controller(p);
    await c.readStatus();
    await c.prepareIdentity();
    final creating = c.createIdentity();
    c.cancel();
    expect(c.unknown, isTrue);
    expect(p.identity.cancels, 1);
    expect(c.cleanup, isNull);
    p.identity.pending!.complete(
      AgentClientIdentityResult(
        scope: f.scope,
        identityId: f.identityId,
        certificateSha256: 'b' * 64,
      ),
    );
    await creating;
    expect(c.identityResult, isNull);
    expect(p.identity.retired.isCompleted, isFalse);
    p.identity.retired.complete();
    c.dispose();
  });
}
