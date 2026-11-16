import 'dart:convert';

import 'package:consolecrypt/automation/automation_agent_installer.dart';
import 'package:consolecrypt/automation/automation_project_metadata.dart';
import 'package:consolecrypt/core/bridge/rust_automation.dart';
import 'package:consolecrypt/core/security/vault_key_alarm.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/src/rust/api/error.dart';
import 'package:consolecrypt/src/rust/frb_generated.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/agent_installer_fixture.dart' as f;

// Actual generated ABI signatures/production facade, synthetic response only.
// No Native/DLL, target installation, release trust or successful sync claim.
class _Api extends Fake implements RustLibApi {
  final calls = <String>[];
  final requests = <Map<String, dynamic>>[];
  BridgeError? failure;
  @override
  Future<String> crateApiAgentInstallerAutomationAgentInstallerContextJson({
    required String requestJson,
  }) async {
    calls.add('context');
    requests.add(jsonDecode(requestJson) as Map<String, dynamic>);
    if (failure case final error?) throw error;
    return jsonEncode(f.context());
  }

  @override
  Future<String> crateApiAgentInstallerAutomationAgentInstallerCreateJson({
    required String requestJson,
  }) async {
    calls.add('create');
    final r = jsonDecode(requestJson) as Map<String, dynamic>;
    requests.add(r);
    if (failure case final error?) throw error;
    return jsonEncode(f.created(r));
  }

  @override
  Future<String> crateApiAutomationAutomationProjectMetadataContext({
    required String id,
  }) async {
    calls.add('metadata-context');
    expect(id, f.projectId);
    throw const BridgeError(
      code: 'unsupported',
      message: 'private',
      details: {},
    );
  }

  @override
  Future<String> crateApiAutomationAutomationProjectMetadataCatalog() async {
    calls.add('metadata-catalog');
    throw const BridgeError(
      code: 'unsupported',
      message: 'private',
      details: {},
    );
  }

  @override
  Future<String> crateApiAutomationAutomationProjectMetadataPublish({
    required String requestJson,
  }) async {
    calls.add('metadata-publish');
    requests.add(jsonDecode(requestJson) as Map<String, dynamic>);
    throw const BridgeError(
      code: 'unsupported',
      message: 'private',
      details: {},
    );
  }
}

void main() {
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
  test('actual factory exposes lazy source and metadata capabilities; original source JSON encoded exactlyonce', () async {
    final service = RustAutomationService();
    expect(service, isA<AutomationAgentInstallerService>());
    expect(service, isA<AutomationProjectMetadataService>());
    expect(api.calls, isEmpty);
    final AutomationAgentInstallerService port = service;
    final frame = AgentInstallerFrame(
      profileId: f.profileId,
      notificationScopeId: f.nonce,
    );
    final context = AgentInstallerContext.parse(
      await port.agentInstallerContext(frame.toJson()),
      frame,
    );
    final r = context.request(
      name: 'Reviewed source',
      host: context.hosts.single,
      operation: AgentInstallerOperation.install,
      start: false,
      artifactPath: 'D:/public/real.zip',
      manifestPath: 'D:/public/real.json',
    );
    final created = AgentInstallerCreated.parse(
      await port.createAgentInstallerProject(r),
      context,
      r,
    );
    expect(api.calls, ['context', 'create']);
    expect(api.requests, [frame.toJson(), r]);
    expect(created.project.id, f.projectId);
    expect(api.calls.length, 2);
  });
  test('actual source delegates preserve original coded vault-key alarm and do notretry', () async {
    final AutomationAgentInstallerService port = RustAutomationService();
    api.failure = const BridgeError(
      code: 'vault_key_mismatch',
      message: 'private',
      details: {},
    );
    await expectLater(
      port.agentInstallerContext(
        AgentInstallerFrame(
          profileId: f.profileId,
          notificationScopeId: f.nonce,
        ).toJson(),
      ),
      throwsA(
        isA<AppException>().having(
          (e) => e.code,
          'code',
          AppErrorCode.vaultKeyMismatch,
        ),
      ),
    );
    expect(vaultKeyAlarm.value?.code, AppErrorCode.vaultKeyMismatch);
    expect(api.calls, ['context']);
  });
  test('actual metadata three delegates are explicit, guarded and never synthesize successfulpublication', () async {
    final AutomationProjectMetadataService port = RustAutomationService();
    expect(api.calls, isEmpty);
    await expectLater(
      port.projectMetadataContext(f.projectId),
      throwsA(isA<AppException>()),
    );
    await expectLater(
      port.projectMetadataCatalog(),
      throwsA(isA<AppException>()),
    );
    final original = {
      'version': 1,
      'profile_id': f.profileId,
      'notification_scope_id': f.nonce,
      'project_id': f.projectId,
    };
    await expectLater(
      port.publishProjectMetadata(original),
      throwsA(isA<AppException>()),
    );
    expect(api.calls, [
      'metadata-context',
      'metadata-catalog',
      'metadata-publish',
    ]);
    expect(api.requests, [original]);
  });
}
