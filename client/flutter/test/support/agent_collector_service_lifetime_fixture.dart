import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/bridge/rust_agent_collector_service.dart';
import 'package:consolecrypt/core/bridge/rust_agent_collector_service_lifetime.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_credentials.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_port.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_revocation.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_status.dart';

import 'agent_collector_service_credentials_fixture.dart' as c;
import 'agent_collector_service_flow_fixture.dart' as f;

/// SYNTHETIC shape of publish_lifetime d4f69499; UX/parser callback proof only.
/// No genuine secret, signed Node response, service, generated API or FFI.
const purpose = 'metrics-collector-service-credential-authorization-v1';
Map<String, dynamic> lifetimeWire({String phase = 'serviceReview', int remaining = 60000}) => {
  ...c.selectedWire(phase: phase),
  'kind': 'finiteCollectorServiceLifetimeCredentials',
  'credentialPurpose': purpose,
  'reviewId': 'a0000000-0000-4000-8000-00000000000a',
  'requiresExplicitLifetimeConsent': true,
  'encryptedCredentialRetentionUntilUtc': '2030-01-02T00:00:00Z',
  'automaticCredentialRefresh': false,
  'remainingMs': remaining,
};

List<AgentCollectorServiceCredentialSelection> refs() => [
  AgentCollectorServiceCredentialSelection(
    credentialId: c.credentialId, credentialRevision: '7', targetId: f.catalog.manifest.targets.first.id,
  ),
  AgentCollectorServiceCredentialSelection(credentialId: c.credentialId, credentialRevision: '7'),
];

AgentCollectorServiceIntent lifetimeIntent() => AgentCollectorServiceIntent(
  f.record, f.catalog, credentials: refs(), credentialUse: AgentCollectorServiceCredentialUse.serviceLifetime,
);

final class LifetimeNative implements AgentCollectorServiceNativeHandle {
  final calls = <String>[];
  String phase = 'serviceReview';
  int remaining = 60000;
  Completer<String>? pendingDetails, pendingConfirm;
  @override
  Future<String> details() async {
    calls.add('details');
    return pendingDetails?.future ?? jsonEncode(lifetimeWire(remaining: remaining));
  }
  @override
  Future<String> confirmConfiguration() async {
    calls.add('confirmConfiguration');
    if (pendingConfirm != null) return pendingConfirm!.future;
    phase = 'configurationRecorded';
    return jsonEncode(lifetimeWire(phase: phase, remaining: remaining));
  }
  @override
  void cancel() {
    calls.add('cancel');
    if (phase == 'serviceReview') phase = 'cancelled';
  }
  @override
  Future<String> joined() async {
    calls.add('joined');
    return f.joinedReply(phase: phase, mayRemain: phase == 'configurationRecorded');
  }
  @override
  void dispose() => calls.add('dispose');
}

final class LifetimeCalls implements AgentCollectorServiceCredentialsPort,
    AgentCollectorServiceStatusPort, AgentCollectorServiceRevocationPort {
  LifetimeCalls() {
    base = RustAgentCollectorServicePort(
      prepareOriginal: (raw) async {
        singleRequests.add(jsonDecode(raw) as Map<String, dynamic>);
        return single;
      },
      readCatalogJson: (_) async => f.catalogReply(),
    );
    port = RustAgentCollectorServiceWithLifetime(
      original: base, credentials: this, status: this, revocation: this,
      prepareLifetimeOriginal: (raw) async {
        lifetimeRequests.add(jsonDecode(raw) as Map<String, dynamic>);
        return pendingPrepare?.future ?? lifetime;
      },
    );
  }
  final single = c.CredentialNative(), lifetime = LifetimeNative();
  final singleRequests = <Map<String, dynamic>>[], lifetimeRequests = <Map<String, dynamic>>[];
  Completer<AgentCollectorServiceNativeHandle>? pendingPrepare;
  late final RustAgentCollectorServicePort base;
  late final RustAgentCollectorServiceWithLifetime port;
  @override
  Future<AgentCollectorServiceCredentialChoices> readCredentialChoices(RuntimeNoticeScope scope) async =>
      parseAgentCollectorServiceCredentialChoices(jsonEncode(c.choicesWire()), scope);
  @override
  Future<AgentCollectorServiceStatus> readSavedStatus(RuntimeNoticeScope scope, AgentAssociationRecord record) =>
      throw UnsupportedError('Not requested by this UX fixture');
  @override
  Future<AgentCollectorServiceRevocationHandle> prepareServiceRevocation(AgentCollectorServiceStatus target) =>
      throw UnsupportedError('Not requested by this UX fixture');
}
