import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/bridge/rust_automation.dart';
import 'package:consolecrypt/core/models/step_up.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/src/rust/api/error.dart';
import 'package:consolecrypt/src/rust/frb_generated.dart';
import 'package:flutter_test/flutter_test.dart';

/// Real generated Dart facade, mocked Rust endpoint: no DLL, profile, signing,
/// network or execution. Native enforcement has separate actual Rust gates.
class _Native implements RustLibApi {
  final calls = <String>[];
  String? review, token;
  Uint8List? callerBuffer;
  String? observedPassphrase;
  BridgeError? failure;
  Completer<String>? pending;
  @override
  Future<String> crateApiAutomationAutomationConfirmOwnerRun({
    required String reviewId, Uint8List? passphrase, String? presenceToken,
  }) async {
    calls.add('owner'); review = reviewId; token = presenceToken;
    callerBuffer = passphrase;
    observedPassphrase = passphrase == null ? null : utf8.decode(passphrase);
    if (failure case final error?) throw error;
    return pending?.future ?? '{"job_id":"native-job"}';
  }
  @override
  Future<String> crateApiAutomationAutomationConfirmRun({required String reviewId}) async {
    calls.add('ordinary'); review = reviewId;
    return '{"job_id":"ordinary-job"}';
  }
  @override
  dynamic noSuchMethod(Invocation invocation) => throw UnsupportedError('Unexpected Native endpoint');
}

void main() {
  late _Native api;
  late RustAutomationService service;
  setUp(() { api = _Native(); RustLib.initMock(api: api); service = RustAutomationService(); });
  tearDown(RustLib.dispose);
  test('actual generated owner facade sends only original review and passphrase bytes, then wipes both buffers', () async {
    final secret = SecretText('synthetic-passphrase-кириллица');
    final port = service as AutomationProductionOwnerService;
    final result = await port.confirmOwnerRun('opaque-original-review', proof: PassphraseProof(secret));
    expect(result, {'job_id': 'native-job'});
    expect(api.calls, ['owner']); expect(api.review, 'opaque-original-review');
    expect(api.observedPassphrase, 'synthetic-passphrase-кириллица'); expect(api.token, isNull);
    expect(api.callerBuffer!.every((byte) => byte == 0), isTrue);
    expect(secret.isWiped, isTrue);
  });
  test('wrong-passphrase BridgeError maps to typed error and never falls back or retries', () async {
    api.failure = const BridgeError(code: 'wrong_passphrase', message: 'synthetic fixed diagnostic', details: {});
    final secret = SecretText('synthetic-private');
    await expectLater(service.confirmOwnerRun('same-review', proof: PassphraseProof(secret)),
      throwsA(isA<AppException>().having((error) => error.code, 'code', AppErrorCode.wrongPassphrase)));
    expect(api.calls, ['owner']); expect(api.review, 'same-review');
    expect(api.callerBuffer!.every((byte) => byte == 0), isTrue); expect(secret.isWiped, isTrue);
  });
  test('awaited Native failure wipes the captured passphrase without retry', () async {
    api.pending = Completer<String>(); final secret = SecretText('pending-private');
    final future = service.confirmOwnerRun('original-review', proof: PassphraseProof(secret));
    await Future<void>.delayed(Duration.zero);
    expect(api.calls, ['owner']);
    api.pending!.completeError(const BridgeError(code: 'request_expired', message: 'synthetic fixed diagnostic', details: {}));
    await expectLater(future, throwsA(isA<AppException>()));
    expect(api.calls, ['owner']); expect(secret.isWiped, isTrue);
    expect(api.callerBuffer!.every((byte) => byte == 0), isTrue);
  });
  test('presence token is not converted to passphrase or ordinary approval when Native refuses it', () async {
    api.failure = const BridgeError(code: 'passphrase_required', message: 'synthetic fixed diagnostic', details: {});
    await expectLater(service.confirmOwnerRun('original-review', proof: const PresenceProof('one-use-token')),
      throwsA(isA<AppException>().having((error) => error.code, 'code', AppErrorCode.passphraseRequired)));
    expect(api.calls, ['owner']); expect(api.token, 'one-use-token'); expect(api.callerBuffer, isNull);
  });
  test('ordinary nonproduction facade keeps its original endpoint and wire', () async {
    expect(await service.confirmRun('ordinary-review'), {'job_id': 'ordinary-job'});
    expect(api.calls, ['ordinary']); expect(api.review, 'ordinary-review');
    expect(api.callerBuffer, isNull); expect(api.token, isNull);
  });
}
