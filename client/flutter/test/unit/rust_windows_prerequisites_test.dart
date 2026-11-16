import 'dart:convert';

import 'package:consolecrypt/automation/automation_windows_prerequisites.dart';
import 'package:consolecrypt/core/bridge/rust_automation.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/src/rust/api/error.dart';
import 'package:consolecrypt/src/rust/frb_generated.dart';
import 'package:flutter_test/flutter_test.dart';

import 'automation_windows_prerequisites_test.dart'
    show originalScope, windowsFact;

class Native extends Fake implements RustLibApi {
  final calls = <String>[];
  String? request, cancelled;
  BridgeError? error;
  @override
  Future<String> crateApiAutomationAutomationWindowsPrerequisitesJson({
    required String requestJson,
  }) async {
    calls.add('inspect');
    request = requestJson;
    if (error case final e?) throw e;
    return jsonEncode(windowsFact());
  }

  @override
  Future<void> crateApiAutomationAutomationRunnerCancelSetup({
    required String operationId,
  }) async {
    calls.add('cancel');
    cancelled = operationId;
  }
}

void main() {
  late Native api;
  late RustAutomationService service;
  setUp(() {
    api = Native();
    RustLib.initMock(api: api);
    service = RustAutomationService();
  });
  tearDown(RustLib.dispose);
  test('actual generated optional facade sends only original scope and operation then cancels exact existing setup slot', () async {
    expect(api.calls, isEmpty);
    const operation = '33333333-3333-4333-8333-333333333333';
    final result = await (service as AutomationWindowsPrerequisiteService)
        .inspectWindowsPrerequisites(
          operationId: operation,
          profileId: originalScope.profileId,
          notificationScopeId: originalScope.nonce,
        );
    expect(jsonDecode(api.request!), {
      'version': 1,
      'operationId': operation,
      'profileId': originalScope.profileId,
      'notificationScopeId': originalScope.nonce,
    });
    expect(
      WindowsPrerequisiteSnapshot.parse(result, originalScope).capturedAt.isUtc,
      isTrue,
    );
    await service.cancelRunnerSetup(operation);
    expect(api.cancelled, operation);
    expect(api.calls, ['inspect', 'cancel']);
  });
  test('vault key alarm propagates typed without fallback, retry or assumed feature state', () async {
    api.error = const BridgeError(
      code: 'vault_key_mismatch',
      message: 'synthetic fixed diagnostic',
      details: {},
    );
    await expectLater(
      service.inspectWindowsPrerequisites(
        operationId: '33333333-3333-4333-8333-333333333333',
        profileId: originalScope.profileId,
        notificationScopeId: originalScope.nonce,
      ),
      throwsA(
        isA<AppException>().having(
          (e) => e.code,
          'code',
          AppErrorCode.vaultKeyMismatch,
        ),
      ),
    );
    expect(api.calls, ['inspect']);
  });
}
