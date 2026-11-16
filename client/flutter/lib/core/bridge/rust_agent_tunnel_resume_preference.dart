import 'dart:convert';

import 'package:consolecrypt/core/bridge/mapping.dart' show guard;
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_resume_preference.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_route_models.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_route_port.dart';

/// Thin optional wrapper. Production composition requires the real generated
/// read-only callback after Root's FRB generation; no fabricated default exists.
final class RustAgentTunnelRouteWithResumePreference implements AgentTunnelRoutePort, AgentTunnelResumePreferencePort {
  const RustAgentTunnelRouteWithResumePreference({required this.original, required this.readJson});
  final AgentTunnelRoutePort original;
  final Future<String> Function(String requestJson) readJson;
  @override
  Future<AgentTunnelRouteHandle> prepare(RuntimeNoticeScope scope, AgentTunnelRouteIntent intent) =>
      original.prepare(scope, intent);
  @override
  Future<AgentTunnelAccessAudit> audit(RuntimeNoticeScope scope) => original.audit(scope);
  @override
  Future<AgentTunnelResumePreference> readResumePreference(
    RuntimeNoticeScope scope,
    AgentTunnelResumeQuery query,
  ) async =>
      parseAgentTunnelResumePreference(await guard(() => readJson(jsonEncode(query.request(scope)))), scope, query);
}
