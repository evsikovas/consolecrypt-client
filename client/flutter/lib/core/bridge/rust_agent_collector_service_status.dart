import 'dart:convert';

import 'package:consolecrypt/core/bridge/mapping.dart' show guard;
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_port.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_status.dart';

/// Optional callback wrapper for actual Core management.rs status. The same
/// original preparation/opaque port is preserved; construction performs no RPC.
final class RustAgentCollectorServiceWithStatus implements AgentCollectorServicePort, AgentCollectorServiceStatusPort {
  const RustAgentCollectorServiceWithStatus({required this.original, required this.readJson});
  final AgentCollectorServicePort original;
  final Future<String> Function(String requestJson) readJson;
  @override
  Future<AgentCollectorServiceCatalogView> readCatalog(RuntimeNoticeScope scope, AgentAssociationRecord record) =>
      original.readCatalog(scope, record);
  @override
  Future<AgentCollectorServiceHandle> prepare(RuntimeNoticeScope scope, AgentCollectorServiceIntent intent) =>
      original.prepare(scope, intent);
  @override
  Future<AgentCollectorServiceStatus> readSavedStatus(RuntimeNoticeScope scope, AgentAssociationRecord record) async =>
      parseAgentCollectorServiceStatus(
        await guard(
          () => readJson(
            jsonEncode({
              'version': 1,
              'profileId': scope.profileId,
              'notificationScopeId': scope.nonce,
              'associationId': record.id,
            }),
          ),
        ),
        scope,
        record,
      );
}
