import 'dart:convert';

import 'package:consolecrypt/core/bridge/mapping.dart' show guard;
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_credentials.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_port.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_revocation.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_status.dart';

/// Optional local metadata callback, supplied after actual Bridge generation.
/// The existing original configuration/status/revocation ports are preserved.
final class RustAgentCollectorServiceWithCredentials
    implements
        AgentCollectorServicePort,
        AgentCollectorServiceStatusPort,
        AgentCollectorServiceRevocationPort,
        AgentCollectorServiceCredentialsPort {
  const RustAgentCollectorServiceWithCredentials({
    required this.original,
    required this.status,
    required this.revocation,
    required this.readJson,
  });
  final AgentCollectorServicePort original;
  final AgentCollectorServiceStatusPort status;
  final AgentCollectorServiceRevocationPort revocation;
  final Future<String> Function(String requestJson) readJson;
  @override
  Future<AgentCollectorServiceCatalogView> readCatalog(RuntimeNoticeScope scope, AgentAssociationRecord record) =>
      original.readCatalog(scope, record);
  @override
  Future<AgentCollectorServiceHandle> prepare(RuntimeNoticeScope scope, AgentCollectorServiceIntent intent) =>
      original.prepare(scope, intent);
  @override
  Future<AgentCollectorServiceStatus> readSavedStatus(RuntimeNoticeScope scope, AgentAssociationRecord record) =>
      status.readSavedStatus(scope, record);
  @override
  Future<AgentCollectorServiceRevocationHandle> prepareServiceRevocation(AgentCollectorServiceStatus target) =>
      revocation.prepareServiceRevocation(target);
  @override
  Future<AgentCollectorServiceCredentialChoices> readCredentialChoices(RuntimeNoticeScope scope) async =>
      parseAgentCollectorServiceCredentialChoices(
        await guard(
          () => readJson(jsonEncode({'version': 1, 'profileId': scope.profileId, 'notificationScopeId': scope.nonce})),
        ),
        scope,
      );
}
