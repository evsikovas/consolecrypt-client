import 'dart:convert';

import 'package:consolecrypt/automation/automation_inventory_export.dart';
import 'package:consolecrypt/automation/automation_runner_cleanup.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/bridge/rust_automation.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/src/rust/api/error.dart';
import 'package:consolecrypt/src/rust/frb_generated.dart';
import 'package:flutter_test/flutter_test.dart';

import '../helpers/inventory_export_fixture.dart';

InventoryExportRequest _request() => InventoryExportRequest(
  projectId: inventoryExportProjectId,
  expectedRevision: 7,
  hostIds: [inventoryExportHostId],
  groupNames: ['web'],
);
BridgeError _invalid(String field, String rule) => BridgeError(
  code: 'invalid_input',
  message: 'fixture diagnostic must not become export UI copy',
  details: {'field': field, 'rule': rule},
);

/// Actual generated top-level functions route to this mock. initMock loads no
/// DLL or Native AppCore and never opens a real profile/network/filesystem.
class _Native implements RustLibApi {
  final calls = <String>[];
  String? issuedRequest, configurationProject;
  String configuration = jsonEncode(inventoryExportConfiguration(selected: [inventoryExportHostId]));
  String snapshot = '\n${const JsonEncoder.withIndent('  ').convert(inventoryExportSnapshot())}\n';
  BridgeError? failure;
  @override
  Future<String> crateApiAutomationAutomationInventoryConfiguration({required String id}) async {
    calls.add('configuration');
    configurationProject = id;
    if (failure case final error?) throw error;
    return configuration;
  }

  @override
  Future<String> crateApiAutomationAutomationInventoryExportIssue({required String requestJson}) async {
    calls.add('issue');
    issuedRequest = requestJson;
    if (failure case final error?) throw error;
    return snapshot;
  }

  @override
  Future<String> crateApiAutomationAutomationRunnerSetup({
    required String operationId,
    required String targetJson,
    required bool install,
  }) async {
    calls.add('setup');
    if (failure case final error?) throw error;
    throw UnsupportedError('Fixture setup must fail with a safe code');
  }

  @override
  Future<String> crateApiAutomationAutomationRunnerCleanupScan({required String requestJson}) async {
    calls.add('cleanup');
    if (failure case final error?) throw error;
    throw UnsupportedError('Fixture cleanup must fail with a safe code');
  }

  @override
  dynamic noSuchMethod(Invocation invocation) =>
      throw UnsupportedError('Unexpected Native adapter call: ${invocation.memberName}');
}

void main() {
  late _Native api;
  late RustAutomationService service;
  setUp(() {
    api = _Native();
    RustLib.initMock(api: api);
    service = RustAutomationService();
  });
  tearDown(RustLib.dispose);

  test('actual Rust facade exposes optional export capability and calls only metadata plus Native issuer', () async {
    expect(service, isA<AutomationInventoryExportService>());
    final context = await service.inventoryExportContext(inventoryExportProjectId);
    expect(api.configurationProject, inventoryExportProjectId);
    expect(context.revision, 7);
    expect(context.permits(inventoryExportOtherId), isFalse); // Group members do not expand the fence.
    final issued = await service.issueInventoryExport(_request());
    expect(api.calls, ['configuration', 'issue']);
    expect(jsonDecode(api.issuedRequest!), _request().toJson());
    expect(issued.nativeJson, api.snapshot);
    expect(issued.bytes, utf8.encode(api.snapshot));
  });

  test('metadata opaque fields are never copied into the issuer request', () async {
    final value = inventoryExportConfiguration();
    value['opaque'] = {'credential_id': 'not-exportable', 'endpoint': 'not-a-caller-input'};
    api.configuration = jsonEncode(value);
    await service.inventoryExportContext(inventoryExportProjectId);
    await service.issueInventoryExport(_request());
    final sent = jsonDecode(api.issuedRequest!) as Map<String, dynamic>;
    expect(sent.keys.toSet(), {'version', 'projectId', 'expectedRevision', 'hostIds', 'groupNames'});
    expect(api.issuedRequest, isNot(contains('not-exportable')));
    expect(api.issuedRequest, isNot(contains('endpoint')));
  });

  test('export safe reasons survive real BridgeError to AppException mapping', () async {
    const reasons = {
      'inventory_export_invalid_request': InventoryExportReason.invalidRequest,
      'inventory_export_context_changed': InventoryExportReason.contextChanged,
      'inventory_export_host_not_selected': InventoryExportReason.hostNotSelected,
      'inventory_export_unknown_group': InventoryExportReason.unknownGroup,
      'inventory_export_invalid_groups': InventoryExportReason.invalidGroups,
      'inventory_export_unsupported_route': InventoryExportReason.unsupportedRoute,
      'inventory_export_unsupported_endpoint': InventoryExportReason.unsupportedEndpoint,
      'inventory_export_alias_conflict': InventoryExportReason.aliasConflict,
      'inventory_export_size': InventoryExportReason.size,
    };
    for (final entry in reasons.entries) {
      api.failure = _invalid('automation_inventory_export', entry.key);
      await expectLater(
        service.issueInventoryExport(_request()),
        throwsA(isA<InventoryExportFailure>().having((e) => e.reason, 'reason', entry.value)),
      );
    }
    api.failure = _invalid('automation_inventory_export', 'unknown_private_diagnostic');
    await expectLater(
      service.issueInventoryExport(_request()),
      throwsA(
        isA<InventoryExportFailure>()
            .having((e) => e.reason, 'reason', InventoryExportReason.failed)
            .having((e) => e.toString(), 'safe string', isNot(contains('unknown_private_diagnostic'))),
      ),
    );
  });

  test('profile and vault scope errors remain standard typed errors, never export retries', () async {
    for (final entry in {
      'vault_locked': AppErrorReason.vaultLocked,
      'no_active_profile': AppErrorReason.noActiveProfile,
    }.entries) {
      api.failure = BridgeError(code: entry.key, message: 'fixture scope revoked', details: const {});
      final before = api.calls.length;
      await expectLater(
        service.issueInventoryExport(_request()),
        throwsA(isA<AppException>().having((e) => e.reason, 'reason', entry.value)),
      );
      expect(api.calls.length, before + 1);
    }
  });

  test('mismatched origin and non-credential-free Native response are rejected without replacement data', () async {
    final wrongScope = inventoryExportSnapshot();
    (wrongScope['origin'] as Map<String, dynamic>)['projectRevision'] = 8;
    api.snapshot = jsonEncode(wrongScope);
    await expectLater(
      service.issueInventoryExport(_request()),
      throwsA(isA<InventoryExportFailure>().having((e) => e.reason, 'reason', InventoryExportReason.invalidResponse)),
    );
    final secret = inventoryExportSnapshot();
    final vars =
        (((secret['inventory'] as Map<String, dynamic>)['_meta'] as Map<String, dynamic>)['hostvars']
                    as Map<String, dynamic>)
                .values
                .first
            as Map<String, dynamic>;
    vars['credential_id'] = 'not-exportable';
    api.snapshot = jsonEncode(secret);
    await expectLater(
      service.issueInventoryExport(_request()),
      throwsA(isA<InventoryExportFailure>().having((e) => e.reason, 'reason', InventoryExportReason.invalidResponse)),
    );
    expect(api.calls, ['issue', 'issue']);
  });

  test('export rule mapper is field scoped and preserves both R10 bootstrap diagnostic families', () async {
    api.failure = _invalid('unrelated_field', 'inventory_export_size');
    await expectLater(service.issueInventoryExport(_request()), throwsA(isA<AppException>()));
    for (final rule in ['setup_bootstrap_failed', 'setup_command_too_large']) {
      api.failure = _invalid('automation_runner', rule);
      await expectLater(
        service.setupRunner({'kind': 'native'}, operationId: inventoryExportOtherId),
        throwsA(isA<AutomationRunnerSetupFailed>().having((e) => e.reason, 'reason', rule)),
      );
    }
    for (final reason in [RunnerCleanupReason.bootstrapFailed, RunnerCleanupReason.commandTooLarge]) {
      api.failure = _invalid('automation_runner_cleanup', reason.wire);
      await expectLater(
        service.scanRunnerCleanup({'kind': 'native'}, operationId: inventoryExportOtherId),
        throwsA(isA<AutomationRunnerCleanupFailed>().having((e) => e.reason, 'reason', reason)),
      );
    }
  });
}
