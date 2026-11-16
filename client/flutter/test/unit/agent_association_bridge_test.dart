import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/core/bridge/rust_agent_association.dart';
import 'package:consolecrypt/core/bridge/rust_monitoring.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/security/vault_key_alarm.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/monitoring/agent_association_controller.dart';
import 'package:consolecrypt/monitoring/agent_association_dialog.dart';
import 'package:consolecrypt/monitoring/agent_association_port.dart';
import 'package:consolecrypt/monitoring/agent_reader_port.dart';
import 'package:consolecrypt/monitoring/agent_revocation_controller.dart';
import 'package:consolecrypt/src/rust/api/agent_node_association.dart' as native;
import 'package:consolecrypt/src/rust/api/error.dart';
import 'package:consolecrypt/src/rust/frb_generated.dart';
import 'package:crypto/crypto.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../support/agent_association_fixture.dart' as f;
import '../support/agent_revocation_fixture.dart' as r;

// Actual generated Dart API, mocked endpoint/opaque objects; no DLL or Agent.
final class _Operation extends Fake implements native.AgentNodeAssociationHandle {
  final calls = <String>[];
  int? lifetime;
  Completer<String>? pendingProbe;
  BridgeError? cancelFailure;
  String? offerReply, certificateReply;
  String probeReply = jsonEncode(f.operationWire());
  String cleanup = '{"version":1,"phase":"unknown","cleanupState":"unknown"}';
  @override
  Future<String> probe() {
    calls.add('probe');
    return pendingProbe?.future ?? Future.value(probeReply);
  }

  @override
  Future<String> approveOffer() async {
    calls.add('offer');
    return offerReply ?? jsonEncode(f.operationWire(phase: 'claimReview'));
  }

  @override
  Future<String> approveCertificate({required int lifetimeSeconds}) async {
    calls.add('certificate');
    lifetime = lifetimeSeconds;
    return certificateReply ?? jsonEncode(f.operationWire(phase: 'associated'));
  }

  @override
  void cancel() {
    calls.add('cancel');
    if (cancelFailure case final failure?) throw failure;
  }

  @override
  Future<String> joined() async {
    calls.add('joined');
    return cleanup;
  }
}

final class _Identity extends Fake implements native.AgentNodeClientIdentityHandle {
  String? detailsReply;
  String cleanup = '{"version":1,"phase":"stored","cleanupState":"closedObserved"}';
  final calls = <String>[];
  @override
  Future<String> details() async {
    calls.add('details');
    return detailsReply ?? jsonEncode(f.identityWire());
  }

  @override
  Future<String> confirmCreation() async {
    calls.add('create');
    return jsonEncode(f.identityWire(stored: true));
  }

  @override
  void cancel() {
    calls.add('cancel');
  }

  @override
  Future<String> joined() async {
    calls.add('joined');
    return cleanup;
  }
}

final class _Revocation extends Fake implements native.AgentNodeRevocationHandle {
  final calls = <String>[];
  String detailsReply = jsonEncode(r.wire()), confirmReply = jsonEncode(r.wire(phase: 'revoked'));
  Completer<String>? pendingDetails;
  BridgeError? failure, cancelFailure;
  @override
  Future<String> details() async {
    calls.add('details');
    if (failure case final error?) throw error;
    return pendingDetails?.future ?? detailsReply;
  }

  @override
  Future<String> confirmOnce() async {
    calls.add('confirm');
    if (failure case final error?) throw error;
    return confirmReply;
  }

  @override
  void cancel() {
    calls.add('cancel');
    if (cancelFailure case final error?) throw error;
  }

  @override
  Future<String> joined() async {
    calls.add('joined');
    return '{"version":1,"phase":"unknown","remoteRevocationObserved":false,"cleanupState":"unknown"}';
  }
}

final class _Api implements RustLibApi {
  String? statusReply;
  final calls = <String>[];
  final requests = <Map<String, dynamic>>[];
  final original = _Operation(), identity = _Identity();
  final revocation = _Revocation();
  native.AgentNodeRevocationHandle? revocationReplacement;
  native.AgentNodeAssociationHandle? replacement;
  Completer<native.AgentNodeAssociationHandle>? pendingRegistration;
  BridgeError? failure;
  @override
  Future<String> crateApiAgentNodeAssociationAgentNodeAssociationStatus({required String requestJson}) async {
    calls.add('status');
    requests.add(jsonDecode(requestJson) as Map<String, dynamic>);
    if (failure case final error?) throw error;
    return statusReply ?? jsonEncode(f.statusWire());
  }

  @override
  Future<native.AgentNodeAssociationHandle> crateApiAgentNodeAssociationAgentNodeAssociationRegister({
    required String requestJson,
  }) async {
    calls.add('register');
    requests.add(jsonDecode(requestJson) as Map<String, dynamic>);
    if (failure case final error?) throw error;
    return pendingRegistration?.future ?? replacement ?? original;
  }

  @override
  Future<native.AgentNodeClientIdentityHandle> crateApiAgentNodeAssociationAgentNodeClientIdentityPrepare({
    required String requestJson,
  }) async {
    calls.add('identity');
    requests.add(jsonDecode(requestJson) as Map<String, dynamic>);
    return identity;
  }

  @override
  Future<native.AgentNodeRevocationHandle> crateApiAgentNodeAssociationAgentNodeAssociationRevokePrepare({
    required String requestJson,
  }) async {
    calls.add('revoke');
    requests.add(jsonDecode(requestJson) as Map<String, dynamic>);
    if (failure case final error?) throw error;
    return revocationReplacement ?? revocation;
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => throw UnsupportedError('Unexpected Native endpoint');
}

AgentAssociationPort _production() =>
    (const RustMonitoringBackend().createService() as AgentAssociationCapability).agentAssociationPort!;
Map<String, Object> get _frame => {'version': 1, 'profileId': f.scope.profileId, 'notificationScopeId': f.scope.nonce};

void main() {
  test('real service factory is lazy and keeps Reader capability alongside real association port', () {
    final service = const RustMonitoringBackend().createService();
    expect((service as AgentAssociationCapability).agentAssociationPort, isA<RustAgentAssociationPort>());
    expect((service as AgentReaderCapability).agentReaderPort, isNotNull);
  });
  group('actual generated association API', () {
    late _Api api;
    setUp(() {
      api = _Api();
      RustLib.initMock(api: api);
      vaultKeyAlarm.value = null;
    });
    tearDown(() {
      RustLib.dispose();
      vaultKeyAlarm.value = null;
    });

    testWidgets(
      'genuine paired Native offer claim and final render through issued production handle with two explicit approvals',
      (t) async {
        String capture(String name, String digest) {
          final bytes = File('test/fixtures/agent_association_paired/$name.json').readAsBytesSync();
          expect(sha256.convert(bytes).toString(), digest);
          return utf8.decode(bytes);
        }

        final statusRaw = capture('status', '7f26dc27be26a94e9a462c84c3ac56a12824de6ac99f8a68cc818d724e96aecc');
        final offerRaw = capture('offer', 'bf8109efcfe161df823be6b08dd284f05493ed0995f44111f2d53b666471cbb7');
        final claimRaw = capture('claim', '46d9bad23036a9dc38e4056317c22c6b018876d4af19a57bf43689a469d3930b');
        final associatedRaw = capture('associated', '9c8527c314a2d2fdfbe6716975dd5de9e94ad4722e3ad3424354dd6efdc3eb73');
        final cleanupRaw = capture('cleanup', 'e6264e1edc87c091b93094e3f79d2ffea49a5d4472734780ecf2c891fc7f734c');
        final frame = jsonDecode(statusRaw) as Map<String, dynamic>;
        // Recorded fixture only; no live Node authority comes from these bytes.
        final original = RuntimeNoticeScope.fromProfile({
          'id': frame['profileId'],
          'active': true,
          'vault_state': 'unlocked',
          'notification_scope_id': frame['notificationScopeId'],
        })!;
        api.statusReply = statusRaw;
        api.original.probeReply = offerRaw;
        api.original.offerReply = claimRaw;
        api.original.certificateReply = associatedRaw;
        api.original.cleanup = cleanupRaw;
        final port = _production();
        await t.pumpWidget(
          MaterialApp(
            supportedLocales: AppLocalizations.supportedLocales,
            localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
            home: Scaffold(
              body: AgentAssociationDialog(port: port, scope: original, original: () => true),
            ),
          ),
        );
        Future<void> tap(String key) async {
          final target = find.byKey(ValueKey(key));
          await t.ensureVisible(target);
          await t.tap(target);
          await t.pump();
          await t.pump(const Duration(milliseconds: 300));
        }

        expect(api.calls, isEmpty);
        await tap('agent-association-status');
        t.widget<GlassSelect<String?>>(find.byKey(const ValueKey('agent-association-host'))).onChanged!(
          '01a1d80e-f90f-761e-873d-755d215675f1',
        );
        await t.pump();
        await tap('agent-association-probe');
        expect(api.original.calls, ['probe']);
        expect(find.text('4de5cdbd2c7a479b7587167587d72087798edc22422458f26e167d8b268bf8b3'), findsOneWidget);
        expect(find.byKey(const ValueKey('agent-association-associated')), findsNothing);
        await tap('agent-association-approve');
        expect(api.original.calls, ['probe', 'offer']);
        expect(api.original.lifetime, isNull);
        expect(find.text('80e3ddffa8c68c4f30b7944b1f305d3a71f768c9065e7f29a84b1d1461da255a'), findsOneWidget);
        final lifetime = t.widget<GlassSelect<int>>(find.byKey(const ValueKey('agent-association-lifetime'))).value;
        expect(lifetime, inInclusiveRange(1, 579));
        await tap('agent-association-approve');
        expect(api.original.lifetime, lifetime);
        expect(api.original.calls, ['probe', 'offer', 'certificate', 'joined']);
        expect(find.byKey(const ValueKey('agent-association-associated')), findsOneWidget);
        expect(find.text('a45ced67198fe147cdb8032c0dc75768f812aab9862a0dfbae6575fde52ffc07'), findsOneWidget);
        expect(find.text('2026-11-14T03:19:02.000Z'), findsOneWidget);
        expect(find.textContaining('not Tunnel admission'), findsOneWidget);
        await t.pump(const Duration(seconds: 2));
        expect(api.calls, ['status', 'register']);
        expect(api.identity.calls, isEmpty);
        expect(api.requests.last['profileId'], original.profileId);
        expect(api.requests.last['notificationScopeId'], original.nonce);
        expect(api.requests.last['expectedHostRevision'], '1');
        await t.pumpWidget(const SizedBox());
        expect(t.takeException(), isNull);
      },
    );

    testWidgets('actual Native review bytes render through the production factory without implicit creation or SSH', (
      t,
    ) async {
      String capture(String name, String digest) {
        final bytes = File('test/fixtures/agent_association_native/$name.json').readAsBytesSync();
        expect(sha256.convert(bytes).toString(), digest);
        return utf8.decode(bytes);
      }

      final status = capture('status', 'fca5273d62f5dbc1863c5e8ebffa03b5769b3cafc380b8b480e4471c63ad3752');
      final review = capture('tls_review', '397629e8464eedcd11e5ca100ef0d8b8e821ba264bf7b2b0689c4b13d2816e1f');
      final recorded = jsonDecode(status) as Map<String, dynamic>;
      // Test-only recorded context; this grants no production scope or Node authority.
      final scope = RuntimeNoticeScope.fromProfile({
        'id': recorded['profileId'],
        'active': true,
        'vault_state': 'unlocked',
        'notification_scope_id': recorded['notificationScopeId'],
      })!;
      api.statusReply = status;
      api.identity.detailsReply = review;
      api.identity.cleanup = '{"version":1,"phase":"cancelled","cleanupState":"unknown"}';
      final port = _production();
      expect(api.calls, isEmpty);
      await t.pumpWidget(
        MaterialApp(
          supportedLocales: AppLocalizations.supportedLocales,
          localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
          home: Scaffold(
            body: AgentAssociationDialog(port: port, scope: scope, original: () => true),
          ),
        ),
      );
      Future<void> tap(String key) async {
        final target = find.byKey(ValueKey(key));
        await t.ensureVisible(target);
        await t.tap(target);
        await t.pump();
        await t.pump(const Duration(milliseconds: 300));
      }

      await tap('agent-association-status');
      expect(api.identity.calls, isEmpty);
      await tap('agent-identity-prepare');
      final owner = (jsonDecode(review) as Map<String, dynamic>)['owner'] as Map<String, dynamic>;
      expect(find.text('localProfile: ${scope.profileId}'), findsOneWidget);
      expect(find.text(owner['deviceId'] as String), findsOneWidget);
      expect(find.text(owner['signingKeySha256'] as String), findsOneWidget);
      expect(find.byKey(const ValueKey('agent-identity-confirm')), findsOneWidget);
      expect(find.byKey(const ValueKey('agent-identity-stored')), findsNothing);
      await t.pump(const Duration(seconds: 2));
      expect(api.calls, ['status', 'identity']);
      expect(api.identity.calls, ['details']);
      expect(api.original.calls, isEmpty);
      expect(
        api.requests.every((r) => r['profileId'] == scope.profileId && r['notificationScopeId'] == scope.nonce),
        isTrue,
      );
      await t.pumpWidget(const SizedBox());
      await t.pump();
      expect(api.identity.calls, ['details', 'cancel', 'joined']);
      expect(t.takeException(), isNull);
    });

    test('actual factory revocation sends only four original refs and retains the exact issued handle', () async {
      final port = _production();
      expect(port, isA<AgentAssociationRevocationPort>());
      expect(api.calls, isEmpty);
      final held = await (port as AgentAssociationRevocationPort).prepareRevocation(f.scope, r.record());
      expect(api.requests.single, {..._frame, 'associationId': f.associationId});
      expect(api.revocation.calls, isEmpty);
      final replacement = _Revocation();
      api.revocationReplacement = replacement;
      expect((await held.details()).localBlocked, false);
      expect((await held.confirmOnce()).remoteRevocationObserved, true);
      held.cancel();
      expect((await held.joined()).cleanupState, 'unknown');
      expect(api.calls, ['revoke']);
      expect(api.revocation.calls, ['details', 'confirm', 'cancel', 'joined']);
      expect(replacement.calls, isEmpty);
      expect(api.original.calls, isEmpty);
      expect(api.identity.calls, isEmpty);
    });
    test('actual issued revocation is cancelled during async details and suppresses its late Native DTO', () async {
      api.revocation.pendingDetails = Completer();
      final c = AgentRevocationController(
        port: _production() as AgentAssociationRevocationPort,
        scope: f.scope,
        record: r.record(),
        original: () => true,
      );
      final task = c.prepare();
      await Future<void>.delayed(Duration.zero);
      expect(api.revocation.calls, ['details']);
      c.cancel();
      expect(api.revocation.calls, ['details', 'cancel', 'joined']);
      api.revocation.pendingDetails!.complete(api.revocation.detailsReply);
      await task;
      expect(c.details, isNull);
      expect(c.canConfirm, false);
      expect(api.calls, ['revoke']);
      c.dispose();
    });
    test('revocation actual generated errors preserve vault alarm and foreign metadata never confirms', () async {
      final held = await (_production() as AgentAssociationRevocationPort).prepareRevocation(f.scope, r.record());
      api.revocation.detailsReply = jsonEncode(r.wire()..['notificationScopeId'] = 'foreign');
      await expectLater(held.details(), throwsFormatException);
      expect(api.revocation.calls, ['details']);
      const failure = BridgeError(code: 'vault_key_mismatch', message: 'PRIVATE_PROSE', details: {});
      api.revocation.failure = failure;
      await expectLater(
        held.confirmOnce(),
        throwsA(isA<AppException>().having((e) => e.code, 'code', AppErrorCode.vaultKeyMismatch)),
      );
      expect(vaultKeyAlarm.value?.code, AppErrorCode.vaultKeyMismatch);
      vaultKeyAlarm.value = null;
      api.revocation.cancelFailure = failure;
      expect(held.cancel, throwsA(isA<AppException>()));
      expect(vaultKeyAlarm.value?.code, AppErrorCode.vaultKeyMismatch);
      expect(api.calls, ['revoke']);
    });

    test('production factory sends only original frame and exact saved revision; no SSH on registration', () async {
      final port = _production();
      expect(api.calls, isEmpty);
      final status = await port.status(f.scope);
      expect(api.requests.single, _frame);
      final held = await port.registerProbe(f.scope, status.hosts.single);
      expect(api.requests.last, {..._frame, 'hostId': f.hostId, 'expectedHostRevision': '9007199254740993'});
      expect(api.original.calls, isEmpty);
      expect((await held.probe()).ownerEpoch, '18446744073709551615');
      expect(api.original.calls, ['probe']);
    });

    test('all methods keep the original issued object after a replacement becomes available', () async {
      final port = _production(), replacement = _Operation();
      final held = await port.registerProbe(f.scope, f.selectedHost);
      api.replacement = replacement;
      await held.probe();
      await held.approveOffer();
      final result = await held.approveCertificate(lifetimeSeconds: 59);
      expect(result.associationId, f.associationId);
      expect(api.original.lifetime, 59);
      held.cancel();
      expect((await held.joined()).state, 'unknown');
      expect(api.calls, ['register']);
      expect(api.original.calls, ['probe', 'offer', 'certificate', 'cancel', 'joined']);
      expect(replacement.calls, isEmpty);
    });

    test('real factory late registration ACK is cancelled without starting its probe', () async {
      api.pendingRegistration = Completer<native.AgentNodeAssociationHandle>();
      final c = AgentAssociationController(port: _production(), scope: f.scope, original: () => true);
      await c.readStatus();
      c.select(c.status!.hosts.single);
      final pending = c.probe();
      c.cancel();
      api.pendingRegistration!.complete(api.original);
      await pending;
      expect(api.calls, ['status', 'register']);
      expect(api.original.calls, ['cancel', 'joined']);
      expect(c.review, isNull);
      c.dispose();
    });

    test('separate actual identity handle does not create before explicit approval or auto-probe afterward', () async {
      final held = await _production().prepareClientIdentity(f.scope);
      expect(api.requests.single, _frame);
      expect(api.identity.calls, isEmpty);
      expect((await held.details()).ownerId, f.profileId);
      expect(api.identity.calls, ['details']);
      expect((await held.confirmCreation()).identityId, f.identityId);
      expect((await held.joined()).state, 'closedObserved');
      held.cancel();
      expect(api.calls, ['identity']);
      expect(api.original.calls, isEmpty);
      expect(api.identity.calls, ['details', 'create', 'joined', 'cancel']);
    });

    test('foreign response and invented cleanup never become a successful review or observed close', () async {
      api.original.probeReply = jsonEncode(f.operationWire()..['notificationScopeId'] = 'different');
      final held = await _production().registerProbe(f.scope, f.selectedHost);
      await expectLater(held.probe(), throwsFormatException);
      api.original.cleanup = '{"version":1,"phase":"cancelled","cleanupState":"closed"}';
      await expectLater(held.joined(), throwsFormatException);
      expect(api.calls, ['register']);
      expect(api.original.calls, ['probe', 'joined']);
    });

    test('sync cancel and async actual errors preserve the blocking vault-key alarm without fallback', () async {
      const failure = BridgeError(code: 'vault_key_mismatch', message: 'PRIVATE_PROSE', details: {});
      final held = await _production().registerProbe(f.scope, f.selectedHost);
      api.original.cancelFailure = failure;
      expect(held.cancel, throwsA(isA<AppException>().having((e) => e.code, 'code', AppErrorCode.vaultKeyMismatch)));
      expect(vaultKeyAlarm.value?.code, AppErrorCode.vaultKeyMismatch);
      vaultKeyAlarm.value = null;
      api.failure = failure;
      await expectLater(
        _production().status(f.scope),
        throwsA(isA<AppException>().having((e) => e.code, 'code', AppErrorCode.vaultKeyMismatch)),
      );
      expect(vaultKeyAlarm.value?.code, AppErrorCode.vaultKeyMismatch);
      expect(api.calls, ['register', 'status']);
    });
  });
}
