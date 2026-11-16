import 'package:consolecrypt/core/bridge/rust_agent_tunnel_resume_preference.dart';
import 'package:consolecrypt/core/bridge/rust_agent_tunnel_route.dart';
import 'package:consolecrypt/core/bridge/rust_agent_tunnel_saved_route.dart';
import 'package:consolecrypt/src/rust/api/agent_tunnel_route.dart' as native;

/// Actual private FRB208 output, ABI1014148186; not yet installed in Main.
final class GeneratedAgentTunnelSavedRouteCalls implements AgentTunnelSavedRouteNativeCalls {
  const GeneratedAgentTunnelSavedRouteCalls();
  @override
  Future<native.AgentTunnelRouteHandle> prepare(String requestJson) =>
      native.agentTunnelRoutePrepare(requestJson: requestJson);
  @override
  Future<native.AgentTunnelRouteHandle?> attachSaved(String requestJson) =>
      native.agentTunnelRouteAttachSaved(requestJson: requestJson);
  @override
  Future<String> detachAutoOpen(native.AgentTunnelRouteHandle original) => original.detachAutoOpen();
  @override
  Future<String> audit(String requestJson) => native.agentTunnelAccessAudit(requestJson: requestJson);
}

/// New service-local metadata starts empty. No lookup, signing or listener is
/// created by construction; all issued mutations keep the original raw object.
RustAgentTunnelRouteWithSavedOriginal createGeneratedSavedTunnelRoutePort() {
  const calls = GeneratedAgentTunnelSavedRouteCalls();
  final preferences = RustAgentTunnelRouteWithResumePreference(
    original: const RustAgentTunnelRoutePort(calls: calls),
    readJson: (requestJson) => native.agentTunnelResumePreference(requestJson: requestJson),
  );
  return RustAgentTunnelRouteWithSavedOriginal(calls: calls, preferences: preferences);
}
