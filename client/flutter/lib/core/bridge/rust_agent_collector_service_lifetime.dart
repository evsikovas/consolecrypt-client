import 'dart:convert';

import 'package:consolecrypt/core/bridge/mapping.dart' show guard;
import 'package:consolecrypt/core/bridge/rust_agent_collector_service.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_credentials.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_port.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_revocation.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_status.dart';

/// Callback for the ACTUAL staged Rust Bridge method
/// agent_collector_service_lifetime_prepare(request_json: String).
/// No generated symbol/import/composition is claimed before real FRB output.
/// Existing read/catalog/status/revoke and SingleStart paths remain separate.
final class RustAgentCollectorServiceWithLifetime
    implements
        AgentCollectorServicePort,
        AgentCollectorServiceLifetimePort,
        AgentCollectorServiceCredentialsPort,
        AgentCollectorServiceStatusPort,
        AgentCollectorServiceRevocationPort {
  const RustAgentCollectorServiceWithLifetime({
    required this.original,
    required this.credentials,
    required this.status,
    required this.revocation,
    required this.prepareLifetimeOriginal,
  });
  final AgentCollectorServicePort original;
  final AgentCollectorServiceCredentialsPort credentials;
  final AgentCollectorServiceStatusPort status;
  final AgentCollectorServiceRevocationPort revocation;
  final Future<AgentCollectorServiceNativeHandle> Function(String requestJson) prepareLifetimeOriginal;

  @override
  Future<AgentCollectorServiceCatalogView> readCatalog(RuntimeNoticeScope scope, AgentAssociationRecord record) =>
      original.readCatalog(scope, record);

  @override
  Future<AgentCollectorServiceHandle> prepare(RuntimeNoticeScope scope, AgentCollectorServiceIntent intent) {
    if (intent.credentialUse != AgentCollectorServiceCredentialUse.singleStart) {
      throw const FormatException('Service-lifetime review requires its distinct prepare entry');
    }
    return original.prepare(scope, intent);
  }

  @override
  Future<AgentCollectorServiceHandle> prepareLifetime(RuntimeNoticeScope scope, AgentCollectorServiceIntent intent) async {
    if (intent.credentialUse != AgentCollectorServiceCredentialUse.serviceLifetime || intent.credentials.isEmpty) {
      throw const FormatException('Explicit service-lifetime credential references required');
    }
    final issued = await guard(() => prepareLifetimeOriginal(jsonEncode(intent.request(scope))));
    return RustAgentCollectorServiceIssuedHandle(issued, scope, intent);
  }

  @override
  Future<AgentCollectorServiceCredentialChoices> readCredentialChoices(RuntimeNoticeScope scope) =>
      credentials.readCredentialChoices(scope);

  @override
  Future<AgentCollectorServiceStatus> readSavedStatus(RuntimeNoticeScope scope, AgentAssociationRecord record) =>
      status.readSavedStatus(scope, record);

  @override
  Future<AgentCollectorServiceRevocationHandle> prepareServiceRevocation(AgentCollectorServiceStatus target) =>
      revocation.prepareServiceRevocation(target);
}
