import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/bridge/rust_agent_reader.dart';
import 'package:consolecrypt/core/bridge/rust_monitoring.dart';
import 'package:consolecrypt/core/bridge/rust_monitoring_consent.dart';
import 'package:consolecrypt/core/security/vault_key_alarm.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/monitoring/agent_reader_port.dart';
import 'package:consolecrypt/monitoring/monitoring_panel_sync_consent.dart';
import 'package:consolecrypt/src/rust/api/error.dart';
import 'package:consolecrypt/src/rust/api/monitoring_consent.dart' as native;
import 'package:flutter_test/flutter_test.dart';

import '../support/panel_policy_fixture.dart' as f;

class Issued extends Fake implements native.MonitoringSettingsConsentHandle {
  int detailsCalls = 0, confirms = 0, rechecks = 0, cancels = 0;
  Completer<String>? pendingDetails, pendingConfirm;
  BridgeError? cancelError;
  @override
  Future<String> details() async {
    detailsCalls++;
    return pendingDetails?.future ?? jsonEncode(f.policy());
  }

  @override
  Future<String> confirm() async {
    confirms++;
    return pendingConfirm?.future ?? f.result();
  }

  @override
  Future<String> recheckCheckpoint() async {
    rechecks++;
    return f.result(recheck: true);
  }

  @override
  void cancel() {
    cancels++;
    if (cancelError case final error?) throw error;
  }
}

void main() {
  setUp(() => vaultKeyAlarm.value = null);
  tearDown(() => vaultKeyAlarm.value = null);
  test('const production factory enables original held port without a request and preserves Reader capability', () {
    const backend = RustMonitoringBackend();
    final store = backend.createSourceStore();
    expect(store, isA<MonitoringPanelSyncConsentCapability>());
    expect((store as MonitoringPanelSyncConsentCapability).panelSyncConsentPort, isA<RustMonitoringConsentPort>());
    expect((backend.createService() as AgentReaderCapability).agentReaderPort, isA<RustAgentReaderPort>());
  });
  test('exact scope JSON and async details use original generated handle; cancel does not wait for confirm', () async {
    final original = Issued()..pendingConfirm = Completer<String>();
    final requests = <String>[];
    final port = RustMonitoringConsentPort(
      prepareNative: (raw) async {
        requests.add(raw);
        return original;
      },
    );
    expect(requests, isEmpty);
    final raw = jsonEncode({'version': 1, 'profileId': f.syncProfile, 'notificationScopeId': f.syncNonce});
    final held = await port.prepare(raw);
    expect(requests, [raw]);
    expect(await held.details(), jsonEncode(f.policy()));
    final pending = held.confirm();
    expect(original.confirms, 1);
    held.cancel();
    expect(original.cancels, 1);
    original.pendingConfirm!.complete(f.result());
    expect(await pending, f.result());
    expect(original.rechecks, 0);
  });
  test('separate checkpoint recheck forwards only to its original issued handle', () async {
    final original = Issued(), foreign = Issued();
    final port = RustMonitoringConsentPort(prepareNative: (_) async => original);
    final held = await port.prepare('ORIGINAL');
    expect(await held.recheckCheckpoint(), f.result(recheck: true));
    expect(original.rechecks, 1);
    expect(original.confirms, 0);
    expect(foreign.rechecks, 0);
    held.cancel();
    expect(original.cancels, 1);
    expect(foreign.cancels, 0);
  });
  test(
    'retirement cancels issued ACK immediately while async details is pending; no late review or double cancel',
    () async {
      final original = Issued()..pendingDetails = Completer<String>();
      final port = RustMonitoringConsentPort(prepareNative: (_) async => original);
      final owner = PanelSyncConsentController(port: port, scope: f.syncScope(), isOriginal: () => true);
      final pending = owner.prepare();
      await Future<void>.delayed(Duration.zero);
      expect(original.detailsCalls, 1);
      owner.retire();
      expect(original.cancels, 1);
      expect(owner.review, isNull);
      original.pendingDetails!.complete(jsonEncode(f.policy()));
      await pending;
      expect(owner.review, isNull);
      expect(original.confirms, 0);
      expect(original.cancels, 1);
      owner.dispose();
      expect(original.cancels, 1);
    },
  );
  test('sync cancel and async bridge errors preserve existing vault alarm without a fallback request', () async {
    const error = BridgeError(code: 'vault_key_mismatch', message: 'PRIVATE_NATIVE_PROSE', details: {});
    final original = Issued()..cancelError = error;
    final port = RustMonitoringConsentPort(prepareNative: (_) async => original);
    final held = await port.prepare('ORIGINAL');
    expect(
      () => held.cancel(),
      throwsA(isA<AppException>().having((e) => e.code, 'code', AppErrorCode.vaultKeyMismatch)),
    );
    expect(vaultKeyAlarm.value?.code, AppErrorCode.vaultKeyMismatch);
    vaultKeyAlarm.value = null;
    var requests = 0;
    final failing = RustMonitoringConsentPort(
      prepareNative: (_) async {
        requests++;
        throw error;
      },
    );
    await expectLater(
      failing.prepare('ORIGINAL'),
      throwsA(isA<AppException>().having((e) => e.code, 'code', AppErrorCode.vaultKeyMismatch)),
    );
    expect(requests, 1);
    expect(vaultKeyAlarm.value?.code, AppErrorCode.vaultKeyMismatch);
  });
}
