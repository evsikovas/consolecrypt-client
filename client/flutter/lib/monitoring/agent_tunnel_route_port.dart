import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_route_models.dart';

abstract interface class AgentTunnelRouteCapability {
  AgentTunnelRoutePort? get agentTunnelRoutePort;
}

abstract interface class AgentTunnelRoutePort {
  Future<AgentTunnelRouteHandle> prepare(RuntimeNoticeScope scope, AgentTunnelRouteIntent intent);
  Future<AgentTunnelAccessAudit> audit(RuntimeNoticeScope scope);
}

abstract interface class AgentTunnelRouteHandle {
  Future<AgentTunnelRouteView> details();
  Future<AgentTunnelRouteView> approveOnce();
  Future<AgentTunnelRouteView> stop();
  void cancel();
  Future<AgentTunnelRouteCleanup> joined();

  /// Releases only this Dart opaque proxy. It never asserts native/remote join.
  void dispose();
}
