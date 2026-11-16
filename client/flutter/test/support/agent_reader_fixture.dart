import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/file_dialog_service.dart';
import 'package:consolecrypt/monitoring/agent_reader_models.dart';
import 'package:consolecrypt/monitoring/agent_reader_port.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';

const profile = '11111111-1111-4111-8111-111111111111',
    hostId = '22222222-2222-4222-8222-222222222222',
    sourceId = '33333333-3333-4333-8333-333333333333',
    bindingId = '44444444-4444-4444-8444-444444444444',
    knownId = '55555555-5555-4555-8555-555555555555',
    device = '66666666-6666-4666-8666-666666666666';
const source = MonitoringSource(
  id: sourceId,
  name: 'Saved telemetry source',
  endpoint: 'http://127.0.0.1:9090',
  revision: 3,
);
final binding = MonitoringBinding(
  id: bindingId,
  name: 'Saved host binding',
  hostId: hostId,
  sourceId: sourceId,
  revision: 7,
  sourceRevision: 3,
  selector: {'instance': 'owned'},
  state: MonitoringBindingState.current,
);
final host = Host(
  id: const ObjectId(hostId),
  name: 'Saved SSH host',
  address: '127.0.0.1',
  createdAt: DateTime.utc(2026),
  updatedAt: DateTime.utc(2026),
);
final known = KnownHost(
  id: const ObjectId(knownId),
  hostPattern: '127.0.0.1',
  keyType: 'ssh-ed25519',
  publicKey: 'PUBLIC_KEY_METADATA',
  fingerprintSha256: 'SHA256:SYNTHETIC',
  source: KnownHostSource.manual,
  addedAt: DateTime.utc(2026),
  updatedAt: DateTime.utc(2026),
);
AgentReaderSelection selection({String nonce = 'original opaque unlock', String profileId = profile}) =>
    AgentReaderSelection(
      profileId: profileId,
      notificationScopeId: nonce,
      source: source,
      binding: binding,
      knownHostId: knownId,
    );
Json native(AgentReaderSelection s) => {
  'nativeProfileId': s.profileId,
  'nativeHostId': s.hostId,
  'sourceId': s.sourceId,
  'sourceRevision': s.sourceRevision,
  'bindingId': s.bindingId,
  'bindingRevision': s.bindingRevision,
  'nativeBindingDigest': 'a' * 64,
  'deviceId': device,
  'notificationScopeId': s.notificationScopeId,
};
Json review(AgentReaderSelection s, AgentReaderAction action) {
  final details = <String, dynamic>{
    'kind': action == AgentReaderAction.reconcile ? 'reconcileKnownHeadOnly' : action.name,
    'native': native(s),
  };
  switch (action) {
    case AgentReaderAction.nodeOffer:
    case AgentReaderAction.nodeCert:
      details.addAll({
        'review': {
          'ownerPin': {
            'context': {
              'tenant': {'kind': 'localProfile', 'id': knownId},
              'deploymentId': device,
            },
            'key': 'b' * 64,
            'epoch': 1,
            'checkpoint': 0,
            'expiresAt': 2000000000,
          },
          'body': {
            'kind': action == AgentReaderAction.nodeOffer ? 'offer' : 'certificate',
            'value': {
              'header': {
                'version': 1,
                'context': {
                  'tenant': {'kind': 'localProfile', 'id': knownId},
                  'deploymentId': device,
                },
                'ownerEpoch': 1,
                'checkpoint': 1,
              },
              'agentId': hostId,
              'agentKey': 'c' * 64,
              'scopes': ['telemetry.read'],
              'expiresAt': 2000000000,
              if (action == AgentReaderAction.nodeOffer) ...{
                'invitationId': device,
                'tokenHash': 'd' * 64,
                'issuedAt': 1900000000,
              },
              if (action == AgentReaderAction.nodeCert) ...{
                'keyEpoch': 1,
                'capabilities': {
                  'version': 1,
                  'scopes': ['telemetry.read'],
                },
                'origin': {
                  'kind': 'enrollment',
                  'invitationId': device,
                  'claimHash': 'e' * 64,
                  'keyFingerprint': 'f' * 64,
                },
                'notBefore': 1900000000,
              },
            },
          },
          'signingNotBefore': 1900000000,
          'signingExpiresAt': 2000000000,
        },
        'witness': 'operatorImport',
        'actorApply': 'notObserved',
      });
    case AgentReaderAction.association:
      details.addAll({
        'target': {'action': 'heartbeatRead'},
        'issuerPin': {'issuerId': device, 'issuerKey': 'b' * 64},
        'expectedHead': {'authorityRevision': 0},
        'transportBinding': {'sshServerKeyDigest': 'c' * 64},
      });
    case AgentReaderAction.issuerInit:
      break;
    default:
      details.addAll({
        'target': {'action': 'heartbeatRead'},
        'installedHead': {'authorityRevision': 2},
        'associationDigest': 'b' * 64,
      });
      if (action == AgentReaderAction.revoke) {
        details.addAll({
          'subject': 'association',
          'association': {'peerDigest': 'a' * 64, 'associationDigest': 'b' * 64},
          'localCancellationFirst': true,
        });
      }
  }
  return {
    'reviewId': device,
    'details': details,
    'remainingMilliseconds': 59000,
    'humanConfirmationRequired': true,
    'deadlineRenewable': false,
  };
}

Json statusJson(AgentReaderSelection s) => {
  'native': native(s),
  'issuer': {'state': 'unknown', 'issuerId': null, 'publicKey': null},
  'association': {'state': 'unknown'},
  'endpoint': {'state': 'unavailable'},
  'runtime': {'busySlots': 0, 'excludesThisStatus': true, 'quarantinedSlots': 0},
  'nodeActorApply': 'unknown',
  'remoteApplyAck': 'notObservedByStatus',
  'associationChoices': {
    'state': 'available',
    'items': <Json>[],
    'truncated': false,
    'provenance': 'verifiedLocalAuthorityNotRemoteAck',
  },
};
Json result(AgentReaderAction a) => switch (a) {
  AgentReaderAction.issuerInit => {'kind': 'issuerReady', 'issuerId': device, 'publicKey': 'b' * 64},
  AgentReaderAction.association => {
    'kind': 'associationInstalled',
    'peerDigest': 'a' * 64,
    'associationDigest': 'b' * 64,
    'ack': 'observedThisOperation',
  },
  AgentReaderAction.read => {'kind': 'readReady', 'targetHash': 'a' * 64, 'expiresAt': 2000000000},
  AgentReaderAction.revoke => {'kind': 'readerRevocationApplied', 'ack': 'observedThisOperation'},
  AgentReaderAction.reconcile => {'kind': 'knownHeadReconciled', 'newApply': false},
  _ => {
    'kind': a == AgentReaderAction.nodeOffer ? 'nodeOfferExported' : 'nodeCertificateExported',
    'publicArtifactPath': 'C:/OWNED_TEMP/signed.json',
    'privateInvitationPath': a == AgentReaderAction.nodeOffer ? 'C:/OWNED_TEMP/private-token.hex' : null,
    'sha256': 'a' * 64,
    'bytes': 200,
    'expiresAt': 2000000000,
    'actorApply': 'notObserved',
    'cleanup': 'cancelDropOrOriginalDeadline',
  },
};
Json heartbeat() => {
  'schema_version': 1,
  'agent_id': hostId,
  'host_id': knownId,
  'host_binding': 'configured_not_identity_attested',
  'version': '1.0.0',
  'uptime_seconds': 42,
  'timestamp': 1900000000,
  'collection': {'state': 'unknown'},
  'capabilities': {'protocol': 'cc-agent-local/1', 'telemetry.read': 1, 'local_history': 1},
  'audit': {'state': 'external_sink'},
  'history_available_while_online_only': true,
};
String dataset(Json body) => jsonEncode({
  'kind': 'readerDataset',
  'targetHash': 'a' * 64,
  'expiresAt': 2000000000,
  'bodyJson': jsonEncode(body),
});
Json history({int count = 2}) => {
  'metrics': readerAgentMetrics,
  'points': [
    for (var i = 0; i < count; i++)
      {
        'timestamp': 1900000000 + i * 15,
        'agent_uptime_seconds': i.toDouble(),
        'agent_collector_running': i == 0 ? null : 1,
      },
  ],
  'truncated': true,
  'evicted_points': 2,
  'stored_bytes': 200,
  'budget_bytes': 2048,
  'effective_data_budget_bytes': 1024,
  'max_age_seconds': 3600,
  'actual_start': count == 0 ? null : 1900000000,
  'actual_end': count == 0 ? null : 1900000000 + (count - 1) * 15,
  'storage_state': 'memory',
  'available_while_agent_online_only': true,
};

class Handle implements AgentReaderIssuedHandle {
  Handle(this.selection, this.action);
  final AgentReaderSelection selection;
  final AgentReaderAction action;
  int confirms = 0, cancels = 0, reads = 0;
  Completer<String>? waitingConfirm;
  String? customDetails;
  bool loseConfirm = false;
  @override
  String details() => customDetails ?? jsonEncode(review(selection, action));
  @override
  Future<String> confirm() async {
    confirms++;
    if (loseConfirm) throw StateError('NO_RAW_ERROR_IN_UI');
    return waitingConfirm?.future ?? jsonEncode(result(action));
  }

  @override
  Future<String> transferExport({required String publicDestination, String? invitationDestination}) async =>
      jsonEncode({
        'kind': 'nodeExportTransferred',
        'publicDestination': publicDestination,
        'invitationDestination': invitationDestination,
        'publicSha256': 'a' * 64,
        'publicBytes': 200,
        'invitationBytes': invitationDestination == null ? null : 64,
        'actorApply': 'notObserved',
        'copiesIrreversible': true,
      });
  @override
  void cancel() {
    cancels++;
  }

  @override
  Future<String> consumeRead() async {
    reads++;
    return dataset(heartbeat());
  }
}

class Port implements AgentReaderNativePort {
  Port(this.selection);
  final AgentReaderSelection selection;
  final requests = <Json>[], statuses = <Json>[], handles = <Handle>[];
  Completer<AgentReaderIssuedHandle>? pending;
  bool loseConfirm = false;
  Json? statusResponse;
  @override
  Future<AgentReaderIssuedHandle> prepareAction(String raw) async {
    final j = readerDecode(raw);
    requests.add(j);
    final a = AgentReaderAction.values.byName((j['action'] as Json)['kind'] as String);
    final h = Handle(selection, a)..loseConfirm = loseConfirm;
    handles.add(h);
    return pending?.future ?? h;
  }

  @override
  Future<String> status(String raw) async {
    statuses.add(readerDecode(raw));
    return jsonEncode(statusResponse ?? statusJson(selection));
  }
}

class Files implements FileDialogService {
  int opens = 0, saves = 0;
  Completer<String?>? pending, pendingSave;
  @override
  Future<String?> chooseOpenFile({List<String> extensions = const []}) async {
    opens++;
    return pending?.future ?? 'C:/OWNED_PUBLIC/selected.json';
  }

  @override
  Future<String?> chooseSaveFile({required String suggestedName, List<String> extensions = const []}) async {
    saves++;
    return pendingSave?.future ?? 'C:/OWNED_EXPORT/$suggestedName';
  }

  @override
  Future<bool> finishSaveFile(String path) async => true;
  @override
  Future<String?> chooseDirectory() async => null;
  @override
  Future<String?> chooseApplication({required String label, required String confirmButtonText}) async => null;
}

Json choice() => {
  'peerDigest': 'a' * 64,
  'associationDigest': 'b' * 64,
  'agentId': hostId,
  'resourceId': knownId,
  'action': 'heartbeatRead',
  'datasetKind': 'heartbeatV1',
  'schemaVersion': 1,
  'targetHash': 'a' * 64,
  'expiresAt': 2000000000,
  'deviceId': device,
  'scope': 'telemetry.read',
};
