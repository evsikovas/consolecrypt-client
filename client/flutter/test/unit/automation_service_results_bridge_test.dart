import 'dart:convert';

import 'package:consolecrypt/automation/automation_portable_service.dart';
import 'package:consolecrypt/automation/automation_service_delegation_service.dart';
import 'package:consolecrypt/automation/automation_service_results.dart';
import 'package:consolecrypt/core/bridge/rust_automation.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/src/rust/api/error.dart';
import 'package:consolecrypt/src/rust/frb_generated.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/service_delegation_fixture.dart';
import '../support/service_results_fixture.dart';

final class _Api implements RustLibApi {
  final requests = <Map<String, dynamic>>[];
  BridgeError? failure;
  @override
  Future<String> crateApiAutomationServiceAutomationServiceResultsJson({required String requestJson}) async {
    requests.add(jsonDecode(requestJson) as Map<String, dynamic>);
    if (failure case final error?) throw error;
    return jsonEncode(recordedServiceResults());
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => throw UnsupportedError('Unexpected Native endpoint');
}

void main() {
  late _Api api;
  setUp(() {
    api = _Api();
    RustLib.initMock(api: api);
  });
  tearDown(RustLib.dispose);
  test(
    'real service facade adds result capability and preserves legacy optional interfaces with no constructor RPC',
    () async {
      final service = RustAutomationService();
      expect(service, isA<AutomationServiceResultsService>());
      expect(service, isA<AutomationServiceDelegationService>());
      expect(service, isA<AutomationPortableGatewayCapability>());
      expect(api.requests, isEmpty);
      final scope = serviceIntent().scope;
      final request = {...scope.frame, 'runnerId': serviceRunner, 'expectedRunnerRevision': 3};
      final raw = await service.serviceDelegationResults(request);
      expect(api.requests, [request]);
      final row = ServiceRecordedResults.parse(raw, scope, serviceRunner).jobs.single;
      expect(row.jobId, resultJob);
      expect(row.state, 'unknown');
      expect(row.partial, isTrue);
      expect(row.exitCode, -15);
      expect(row.timedOut, isNull);
    },
  );
  test('actual fixed results failure maps once without status reconnect retry or guessed success', () async {
    api.failure = const BridgeError(
      code: 'invalid_input',
      message: 'PRIVATE_NATIVE_DIAGNOSTIC',
      details: {'field': 'service_results', 'rule': 'service_results_unavailable_or_changed'},
    );
    final scope = serviceIntent().scope;
    await expectLater(
      RustAutomationService().serviceDelegationResults({
        ...scope.frame,
        'runnerId': serviceRunner,
        'expectedRunnerRevision': 3,
      }),
      throwsA(
        isA<AppException>()
            .having((e) => e.args['field'], 'field', 'service_results')
            .having((e) => e.args['rule'], 'rule', 'service_results_unavailable_or_changed'),
      ),
    );
    expect(api.requests, hasLength(1));
  });
}
