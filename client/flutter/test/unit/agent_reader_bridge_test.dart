import 'dart:async';

import 'package:consolecrypt/core/bridge/rust_agent_reader.dart';
import 'package:consolecrypt/core/bridge/rust_monitoring.dart';
import 'package:consolecrypt/core/security/vault_key_alarm.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/monitoring/agent_reader_port.dart';
import 'package:consolecrypt/src/rust/api/agent_reader.dart' as native;
import 'package:consolecrypt/src/rust/api/error.dart';
import 'package:flutter_test/flutter_test.dart';

BridgeError nativeError({String code = 'invalid_input', String field = 'agent_reader', String? rule}) =>
    BridgeError(code: code, message: 'PRIVATE_NATIVE_PROSE_NEVER_RENDERED', details: {'field': field, 'rule': ?rule});

class OriginalHandle extends Fake implements native.ReaderActionHandle {
  int cancelled = 0, confirmed = 0, consumed = 0;
  final confirmation = Completer<String>();
  final transfers = <List<String?>>[];
  @override
  String details() => '{"epoch":18446744073709551615}';
  @override
  Future<String> confirm() {
    confirmed++;
    return confirmation.future;
  }

  @override
  void cancel() {
    cancelled++;
  }

  @override
  Future<String> consumeRead() async {
    consumed++;
    return 'DATASET';
  }

  @override
  Future<String> transferExport({required String publicDestination, String? invitationDestination}) async {
    transfers.add([publicDestination, invitationDestination]);
    return 'TRANSFERRED';
  }
}

void main() {
  test('production factory exposes actual optional port without initializing Native or making an RPC', () {
    const backend = RustMonitoringBackend();
    final service = backend.createService();
    expect(service, isA<AgentReaderCapability>());
    expect((service as AgentReaderCapability).agentReaderPort, isA<RustAgentReaderPort>());
  });
  test('actual generated interface preserves original handle and exact wire strings through every method', () async {
    final original = OriginalHandle(), requests = <String>[], statuses = <String>[];
    final port = RustAgentReaderPort(
      prepare: (raw) async {
        requests.add(raw);
        return original;
      },
      readStatus: (raw) async {
        statuses.add(raw);
        return 'STATUS';
      },
    );
    expect(requests, isEmpty);
    const request = '{"notificationScopeId":"opaque original"}';
    final held = await port.prepareAction(request);
    expect(requests, [request]);
    expect(held.details(), '{"epoch":18446744073709551615}');
    final pending = held.confirm();
    expect(original.confirmed, 1);
    held.cancel();
    expect(original.cancelled, 1);
    original.confirmation.complete('ACTUAL-RESULT');
    expect(await pending, 'ACTUAL-RESULT');
    expect(await held.consumeRead(), 'DATASET');
    expect(original.consumed, 1);
    expect(
      await held.transferExport(publicDestination: 'C:/OWNED/public', invitationDestination: 'C:/OWNED/private'),
      'TRANSFERRED',
    );
    expect(original.transfers, [
      ['C:/OWNED/public', 'C:/OWNED/private'],
    ]);
    expect(await port.status(request), 'STATUS');
    expect(statuses, [request]);
  });
  setUp(() => vaultKeyAlarm.value = null);
  tearDown(() => vaultKeyAlarm.value = null);
  test('all nine exact Native field rules preserve typed fixed reason without prose', () {
    for (final code in [
      'reader_action_rejected',
      'reader_busy',
      'reader_profile_unsupported',
      'reader_unavailable',
      'reader_outcome_unknown',
      'reader_cancelled',
      'reader_expired',
      'reader_response_invalid',
      'reader_revoke_pending',
    ]) {
      final mapped = readerBridgeFailure(nativeError(rule: code));
      expect(mapped, isA<AgentReaderException>());
      expect((mapped as AgentReaderException).code, code);
      expect(mapped.toString(), isNot(contains('PRIVATE_NATIVE')));
    }
  });
  test('foreign domain/code, missing/future or substring rules remain unavailable', () {
    for (final value in [
      nativeError(field: 'automation', rule: 'reader_busy'),
      nativeError(code: 'internal', rule: 'reader_busy'),
      nativeError(),
      nativeError(rule: 'reader_busy private suffix'),
      nativeError(rule: 'future_ready'),
      StateError('reader_busy'),
    ]) {
      expect((readerBridgeFailure(value) as AgentReaderException).code, 'reader_unavailable');
    }
  });
  test('sync and async wrappers call exact original once and preserve successful result', () async {
    var sync = 0, async = 0;
    expect(
      readerBridgeSync(() {
        sync++;
        return 'ORIGINAL';
      }),
      'ORIGINAL',
    );
    expect(
      await readerBridgeCall(() async {
        async++;
        return 'HELD';
      }),
      'HELD',
    );
    expect(sync, 1);
    expect(async, 1);
    expect(vaultKeyAlarm.value, isNull);
  });
  test('sync opaque errors use the same closed mapper without swallowing a blocking vault alarm', () {
    expect(
      () => readerBridgeSync<String>(() => throw nativeError(rule: 'reader_expired')),
      throwsA(isA<AgentReaderException>().having((v) => v.code, 'code', 'reader_expired')),
    );
    expect(
      () => readerBridgeSync<String>(() => throw nativeError(code: 'vault_key_mismatch', rule: 'reader_busy')),
      throwsA(isA<AppException>().having((v) => v.code, 'code', AppErrorCode.vaultKeyMismatch)),
    );
    expect(vaultKeyAlarm.value?.code, AppErrorCode.vaultKeyMismatch);
  });
  test('async vault mismatch preserves actual original AppException and raises existing alarm', () async {
    const original = AppException(AppErrorCode.vaultKeyMismatch, 'Owned mismatch', args: {'vault_id': 'owned-vault'});
    await expectLater(readerBridgeCall<String>(() async => throw original), throwsA(same(original)));
    expect(vaultKeyAlarm.value, same(original));
  });
  test('async non-reader failures never leak diagnostic text or get retried', () async {
    var calls = 0;
    await expectLater(
      readerBridgeCall<String>(() async {
        calls++;
        throw nativeError(field: 'foreign', rule: 'reader_action_rejected');
      }),
      throwsA(isA<AgentReaderException>().having((v) => v.code, 'code', 'reader_unavailable')),
    );
    expect(calls, 1);
    expect(vaultKeyAlarm.value, isNull);
  });
}
