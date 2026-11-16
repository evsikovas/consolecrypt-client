import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_association_wire.dart';
import 'package:crypto/crypto.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/agent_association_fixture.dart' as f;

String _actualCapture(String name, String digest, {String directory = 'agent_association_native'}) {
  final bytes = File('test/fixtures/$directory/$name.json').readAsBytesSync();
  expect(sha256.convert(bytes).toString(), digest);
  expect(bytes.length, lessThanOrEqualTo(name == 'status' ? 65536 : 16384));
  return utf8.decode(bytes);
}

// Recorded fixture context only: a captured response never establishes live authority.
RuntimeNoticeScope _recordedFrame(String raw) {
  final body = jsonDecode(raw) as Map<String, dynamic>;
  return RuntimeNoticeScope.fromProfile({
    'id': body['profileId'],
    'active': true,
    'vault_state': 'unlocked',
    'notification_scope_id': body['notificationScopeId'],
  })!;
}

void main() {
  test(
    'genuine SSH enrollment offer claim final and cleanup bytes preserve original bindings without Actor admission',
    () {
      final statusRaw = _actualCapture(
        'status',
        '7f26dc27be26a94e9a462c84c3ac56a12824de6ac99f8a68cc818d724e96aecc',
        directory: 'agent_association_paired',
      );
      final offerRaw = _actualCapture(
        'offer',
        'bf8109efcfe161df823be6b08dd284f05493ed0995f44111f2d53b666471cbb7',
        directory: 'agent_association_paired',
      );
      final claimRaw = _actualCapture(
        'claim',
        '46d9bad23036a9dc38e4056317c22c6b018876d4af19a57bf43689a469d3930b',
        directory: 'agent_association_paired',
      );
      final associatedRaw = _actualCapture(
        'associated',
        '9c8527c314a2d2fdfbe6716975dd5de9e94ad4722e3ad3424354dd6efdc3eb73',
        directory: 'agent_association_paired',
      );
      final cleanupRaw = _actualCapture(
        'cleanup',
        'e6264e1edc87c091b93094e3f79d2ffea49a5d4472734780ecf2c891fc7f734c',
        directory: 'agent_association_paired',
      );
      final original = _recordedFrame(statusRaw);
      final status = parseAgentAssociationStatus(statusRaw, original);
      final host = status.hosts.singleWhere((h) => h.id == '01a1d80e-f90f-761e-873d-755d215675f1');
      final offer = parseAgentAssociationReview(offerRaw, original, host, AgentAssociationReviewStage.offer);
      final claim = parseAgentAssociationReview(claimRaw, original, host, AgentAssociationReviewStage.certificate);
      final result = parseAgentAssociationResult(associatedRaw, original, host);
      expect(offer.ownerId, original.profileId);
      expect(offer.nodeContextId, isNot(offer.ownerId));
      expect(claim.ownerId, offer.ownerId);
      expect(claim.nodeFingerprint, offer.nodeFingerprint);
      expect(claim.clientIdentityId, offer.clientIdentityId);
      expect(claim.hostRevision, '1');
      expect(claim.maxCertificateLifetimeSeconds, 579);
      expect(claim.remainingMs, lessThan(offer.remainingMs));
      expect(result.agentId, offer.agentId);
      expect(result.associationId, claim.associationId);
      expect(result.certificateDigest, 'a45ced67198fe147cdb8032c0dc75768f812aab9862a0dfbae6575fde52ffc07');
      expect(result.checkpoint, '2');
      expect(result.certificateExpiresUtc, DateTime.parse('2026-11-14T03:19:02Z'));
      expect((jsonDecode(associatedRaw) as Map<String, dynamic>)['actorAdmission'], isFalse);
      final cleanup = parseAgentAssociationCleanup(cleanupRaw);
      expect(cleanup.phase, 'associated');
      expect(cleanup.state, 'closedObserved');
      expect(status.records.single.phase, AgentAssociationPhase.unknown);
      expect(status.records.single.certificateDigest, isNull);
      expect(
        () => parseAgentAssociationReview(associatedRaw, original, host, AgentAssociationReviewStage.offer),
        throwsFormatException,
      );
      expect(() => parseAgentAssociationResult(offerRaw, original, host), throwsFormatException);
      expect(() => parseAgentAssociationResult(associatedRaw, f.scope, host), throwsFormatException);
      final forged = jsonDecode(associatedRaw) as Map<String, dynamic>;
      forged['actorAdmission'] = true;
      expect(() => parseAgentAssociationResult(jsonEncode(forged), original, host), throwsFormatException);
    },
  );

  test('untouched Native status and identity review parse; idle metadata cannot become an offer', () {
    final raw = _actualCapture('status', 'fca5273d62f5dbc1863c5e8ebffa03b5769b3cafc380b8b480e4471c63ad3752');
    final scope = _recordedFrame(raw);
    final status = parseAgentAssociationStatus(raw, scope);
    expect(status.clientIdentityState, AgentClientIdentityState.missing);
    expect(status.clientIdentityId, isNull);
    expect(status.records, isEmpty);
    expect(status.hosts.single.name, 'Owned saved Native SSH');
    expect(status.hosts.single.revision, '1');
    final reviewRaw = _actualCapture('tls_review', '397629e8464eedcd11e5ca100ef0d8b8e821ba264bf7b2b0689c4b13d2816e1f');
    final review = parseAgentClientIdentityReview(reviewRaw, scope);
    expect(review.ownerKind, 'localProfile');
    expect(review.ownerId, scope.profileId);
    expect(review.remainingMs, 59999);
    expect(review.deviceId, '01a1d72e-26cb-7243-b315-e686cbb8695a');
    final idle = _actualCapture('idle', '63be8885e829cb045484160d47fdd277ba64099ad3503456c43880f135000c59');
    expect(
      () => parseAgentAssociationReview(idle, scope, status.hosts.single, AgentAssociationReviewStage.offer),
      throwsFormatException,
    );
    expect(() => parseAgentAssociationResult(idle, scope, status.hosts.single), throwsFormatException);
    expect(() => parseAgentClientIdentityReview(reviewRaw, f.scope), throwsFormatException);
  });
  test('actual stored TLS parses in its own scope and cannot complete another recorded owner review', () {
    final raw = _actualCapture('tls_stored', 'c05c343cba6342811f9fe649e837d1e7adb2f54728e1144365c3d1613da8d1c3');
    final scope = _recordedFrame(raw);
    final stored = parseAgentClientIdentityResult(raw, scope);
    expect(stored.identityId, 'e8bf152f-a0b0-4002-9944-8ec5b19dc48d');
    expect(stored.certificateSha256, 'bd37f6e8ca0f0d1465be3d37e31da4cb7320e2bda63c3549160febcd27d91718');
    final reviewRaw = _actualCapture('tls_review', '397629e8464eedcd11e5ca100ef0d8b8e821ba264bf7b2b0689c4b13d2816e1f');
    expect(scope.same(_recordedFrame(reviewRaw)), isFalse);
    expect(() => parseAgentClientIdentityResult(raw, _recordedFrame(reviewRaw)), throwsFormatException);
    expect(() => parseAgentClientIdentityReview(raw, scope), throwsFormatException);
  });

  test('closed Native contract preserves exact u64 and distinct Native versus Node identity domains', () {
    final review = f.review();
    expect(review.ownerEpoch, '18446744073709551615');
    expect(review.hostRevision, '9007199254740993');
    expect(review.ownerId, f.profileId);
    expect(review.nodeContextId, isNot(f.profileId));
    expect(review.ownerSigningKeySha256, 'e' * 64);
    expect(f.result().certificateExpiresUtc!.isUtc, isTrue);
    final status = parseAgentAssociationStatus(jsonEncode(f.statusWire()), f.scope);
    expect(status.clientIdentityState, AgentClientIdentityState.storedMetadata);
    expect(status.hosts.single.revision, '9007199254740993');
  });
  test('future malformed or foreign operation and authority fields fail closed', () {
    for (final change in [
      {'version': 1.0},
      {'version': 2},
      {'notificationScopeId': 'different'},
      {'hostRevision': '1'},
      {'ownerEpoch': 9007199254740993},
      {'ownerEpoch': '18446744073709551616'},
      {'remainingMs': 0},
      {'actorAdmission': true},
      {'remainingMs': 60001},
      {'maxCertificateLifetimeSeconds': 1},
      {
        'ownerPin': {'key': 'PRIVATE'},
      },
      {'certificateDigest': 'a' * 64},
    ]) {
      final raw = f.operationWire()..addAll(change);
      expect(
        () => parseAgentAssociationReview(jsonEncode(raw), f.scope, f.selectedHost, AgentAssociationReviewStage.offer),
        throwsFormatException,
        reason: '$change',
      );
    }
    final rawNumber = jsonEncode(f.operationWire()).replaceFirst('"18446744073709551615"', '18446744073709551615');
    expect(
      () => parseAgentAssociationReview(rawNumber, f.scope, f.selectedHost, AgentAssociationReviewStage.offer),
      throwsFormatException,
    );
    final claim = f.operationWire(phase: 'claimReview', maximum: 0);
    expect(
      () => parseAgentAssociationReview(
        jsonEncode(claim),
        f.scope,
        f.selectedHost,
        AgentAssociationReviewStage.certificate,
      ),
      throwsFormatException,
    );
  });
  test('local status never accepts partial identities unexpected certs or unbounded choices', () {
    for (final identity in [
      {'state': 'missing', 'identityId': f.identityId, 'certificateSha256': 'b' * 64},
      {'state': 'storedMetadata', 'identityId': null, 'certificateSha256': null},
      {'state': 'pending', 'identityId': f.identityId, 'certificateSha256': null},
      {'state': 'ready', 'identityId': f.identityId, 'certificateSha256': 'b' * 64},
    ]) {
      final raw = f.statusWire()..['clientIdentity'] = identity;
      expect(() => parseAgentAssociationStatus(jsonEncode(raw), f.scope), throwsFormatException);
    }
    final raw = f.statusWire();
    raw['hosts'] = List.generate(65, (_) => (f.statusWire()['hosts'] as List).single);
    expect(() => parseAgentAssociationStatus(jsonEncode(raw), f.scope), throwsFormatException);
  });
  test('separate creation has no generated identity before consent and cleanup is explicit observation', () {
    final review = parseAgentClientIdentityReview(jsonEncode(f.identityWire()), f.scope);
    expect(review.ownerId, f.profileId);
    expect(review.remainingMs, 60000);
    for (final change in [
      {'algorithm': 'rsa'},
      {'validityDays': 30.0},
      {'identityId': f.identityId},
      {'privateKey': 'FORBIDDEN'},
      {'remainingMs': 0},
    ]) {
      expect(
        () => parseAgentClientIdentityReview(jsonEncode(f.identityWire()..addAll(change)), f.scope),
        throwsFormatException,
      );
    }
    final cleanup = parseAgentAssociationCleanup('{"version":1,"phase":"cancelled","cleanupState":"pending"}');
    expect(cleanup.state, 'pending');
    expect(cleanup.state, isNot('closedObserved'));
    expect(
      () => parseAgentAssociationCleanup('{"version":1,"phase":"cancelled","cleanupState":"closed"}'),
      throwsFormatException,
    );
  });
}
