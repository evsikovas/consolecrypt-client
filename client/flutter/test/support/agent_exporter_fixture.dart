// SYNTHETIC actual exporter Native source shape. No live grant/Agent/FFI proof.
import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/bridge/rust_agent_exporter.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/src/rust/api/agent_node_association.dart' as existing;

const profile = '10000000-0000-4000-8000-000000000001',
    host = '20000000-0000-4000-8000-000000000002',
    agent = '30000000-0000-4000-8000-000000000003',
    association = '40000000-0000-4000-8000-000000000004',
    device = '50000000-0000-4000-8000-000000000005';
final scope = RuntimeNoticeScope.fromProfile({
  'id': profile,
  'active': true,
  'vault_state': 'unlocked',
  'notification_scope_id': 'exporter-original',
})!;
final now = DateTime.utc(2030);
AgentAssociationRecord record({AgentAssociationPurpose purpose = AgentAssociationPurpose.exporters}) =>
    AgentAssociationRecord(
      associationId: association,
      hostId: host,
      agentId: agent,
      phase: AgentAssociationPhase.associated,
      hostRevision: '9007199254740993',
      nodeKeyFingerprint: 'a' * 64,
      certificateDigest: 'b' * 64,
      nodeKeyEpoch: '1',
      rotationState: 'notRequested',
      revocationState: 'notRequested',
      purpose: purpose,
    );
Map<String, dynamic> policy() => {
  'format': 'cc-agent-managed-node-exporter/v1',
  'hostId': host,
  'agentId': agent,
  'artifact': {
    'name': 'node_exporter',
    'version': 'v1.12.1',
    'platform': 'linux-x86_64',
    'archiveSha256': 'c' * 64,
    'binarySha256': 'd' * 64,
    'binaryBytes': 4096,
    'noticeInventorySha256': 'e' * 64,
  },
  'bindIp': '127.0.0.1',
  'bindPort': 19100,
  'collectors': ['cpu', 'meminfo'],
  'maxSeconds': 300,
};
Map<String, dynamic> catalogWire() => {
  'version': 1,
  'profileId': profile,
  'notificationScopeId': scope.nonce,
  'phase': 'exporterCatalog',
  'associationId': association,
  'hostId': host,
  'hostRevision': '9007199254740993',
  'node': {'agentId': agent, 'keyEpoch': '1', 'publicKey': 'f' * 64, 'keyFingerprint': 'a' * 64},
  'scope': 'exporters.manage',
  'purpose': 'exporters',
  'manifestSha256': '1' * 64,
  'policy': policy(),
  'runtimeObserved': false,
  'actorAdmission': false,
  'installationPerformed': false,
  'automaticRenewal': false,
  'cleanupState': 'closedObserved',
};
Map<String, dynamic> view({String phase = 'exporterReview', int remaining = 60000}) {
  final admitted = phase == 'running' || phase == 'stopped';
  return {
    ...catalogWire(),
    'phase': phase,
    'owner': {'kind': 'localProfile', 'id': profile, 'deviceId': device, 'signingKeySha256': '2' * 64},
    'reviewRemainingMs': admitted ? null : remaining,
    'originalExpiresUtc': '2030-01-01T00:02:00Z',
    'leaseHash': admitted ? '3' * 64 : null,
    'artifactVersion': 'v1.12.1',
    'runningVersion': null,
    'runtimeObserved': admitted,
    'actorAdmission': admitted,
    'childPrivilege': 'nonroot-no-capabilities',
    'cleanupState': 'pending',
    'remoteDrainObserved': false,
  };
}

String joinedWire({String cleanup = 'closedObserved', bool admitted = false}) => jsonEncode({
  'version': 1,
  'phase': 'cancelled',
  'actorAdmission': admitted,
  'runtimeObserved': admitted,
  'remoteDrainObserved': false,
  'cleanupState': cleanup,
});

class Raw implements AgentExporterOriginal {
  final List<String> calls = [];
  Map<String, dynamic> reply = view();
  String cleanup = 'closedObserved';
  Completer<String>? approval;
  @override
  Future<String> details() async {
    calls.add('details');
    return jsonEncode(reply);
  }

  @override
  Future<String> approveOnce() async {
    calls.add('approve');
    reply = view(phase: 'running');
    if (approval != null) {
      return await approval!.future;
    }
    return jsonEncode(reply);
  }

  @override
  Future<String> stop() async {
    calls.add('stop');
    return jsonEncode(reply);
  }

  @override
  void cancel() {
    calls.add('cancel');
  }

  @override
  Future<String> joined() async {
    calls.add('joined');
    return joinedReply();
  }

  String joinedReply() => joinedWire(cleanup: cleanup, admitted: reply['actorAdmission'] == true);
  @override
  void dispose() {
    calls.add('dispose');
  }
}

class Calls implements AgentExporterNativeCalls {
  final Map<String, List<Map<String, dynamic>>> requests = {};
  final Raw raw = Raw();
  Completer<AgentExporterOriginal>? pending;
  Exception? error;
  late final RustAgentExporterPort port = RustAgentExporterPort(this);
  void capture(String key, String json) =>
      requests.putIfAbsent(key, () => []).add(jsonDecode(json) as Map<String, dynamic>);
  @override
  Future<String> catalog(String request) async {
    capture('catalog', request);
    if (error != null) {
      throw error!;
    }
    return jsonEncode(catalogReply());
  }

  Map<String, dynamic> catalogReply() => catalogWire();
  @override
  Future<AgentExporterOriginal> prepare(String request) async {
    capture('prepare', request);
    if (pending != null) {
      return await pending!.future;
    }
    return raw;
  }

  @override
  Future<existing.AgentNodeAssociationHandle> registerExporters(String request) => throw UnimplementedError();
  @override
  Future<existing.AgentNodeRevocationHandle> revokeExporters(String request) => throw UnimplementedError();
}
