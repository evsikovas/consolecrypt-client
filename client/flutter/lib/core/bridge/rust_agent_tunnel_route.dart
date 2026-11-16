import 'dart:convert';

import 'package:consolecrypt/core/bridge/mapping.dart' show guard, toAppException;
import 'package:consolecrypt/core/security/vault_key_alarm.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_route_models.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_route_port.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_route_wire.dart';
import 'package:consolecrypt/src/rust/api/agent_tunnel_route.dart' as native;

/// Exact generated callback seam. Constructor performs no Native call.
abstract interface class AgentTunnelRouteNativeCalls {
  Future<native.AgentTunnelRouteHandle> prepare(String requestJson);
  Future<String> audit(String requestJson);
}

final class GeneratedAgentTunnelRouteCalls implements AgentTunnelRouteNativeCalls {
  const GeneratedAgentTunnelRouteCalls();
  @override
  Future<native.AgentTunnelRouteHandle> prepare(String requestJson) =>
      native.agentTunnelRoutePrepare(requestJson: requestJson);
  @override
  Future<String> audit(String requestJson) => native.agentTunnelAccessAudit(requestJson: requestJson);
}

void _sync(void Function() call) {
  try {
    call();
  } catch (error) {
    final mapped = toAppException(error);
    reportVaultKeyMismatch(mapped);
    throw mapped;
  }
}

final class RustAgentTunnelRoutePort implements AgentTunnelRoutePort {
  const RustAgentTunnelRoutePort({this.calls = const GeneratedAgentTunnelRouteCalls()});
  final AgentTunnelRouteNativeCalls calls;
  @override
  Future<AgentTunnelRouteHandle> prepare(RuntimeNoticeScope scope, AgentTunnelRouteIntent intent) async {
    final original = await guard(() => calls.prepare(jsonEncode(intent.request(scope))));
    return _OriginalRoute(original, scope, intent);
  }

  @override
  Future<AgentTunnelAccessAudit> audit(RuntimeNoticeScope scope) async => parseAgentTunnelAccessAudit(
    await guard(
      () => calls.audit(jsonEncode({'version': 1, 'profileId': scope.profileId, 'notificationScopeId': scope.nonce})),
    ),
    scope,
  );
}

/// Reuses the existing strict proxy for the same already-issued Native object.
/// Future optional adapters supply only that opaque object, never a DTO owner.
AgentTunnelRouteHandle originalRustTunnelRouteHandle(
  native.AgentTunnelRouteHandle original,
  RuntimeNoticeScope scope,
  AgentTunnelRouteIntent intent,
) => _OriginalRoute(original, scope, intent);

/// No global Core or generated free function is consulted after issuance.
final class _OriginalRoute implements AgentTunnelRouteHandle {
  _OriginalRoute(this.original, this.scope, this.intent);
  final native.AgentTunnelRouteHandle original;
  final RuntimeNoticeScope scope;
  final AgentTunnelRouteIntent intent;
  bool _disposed = false, _disposing = false;
  void _live() {
    if (_disposed || _disposing) throw StateError('Original tunnel proxy disposed or releasing');
  }

  @override
  Future<AgentTunnelRouteView> details() async {
    _live();
    return parseAgentTunnelRouteView(await guard(original.details), scope, intent);
  }

  @override
  Future<AgentTunnelRouteView> approveOnce() async {
    _live();
    return parseAgentTunnelRouteView(await guard(original.approveOnce), scope, intent);
  }

  @override
  Future<AgentTunnelRouteView> stop() async {
    _live();
    return parseAgentTunnelRouteView(await guard(original.stop), scope, intent);
  }

  @override
  void cancel() {
    _live();
    _sync(original.cancel);
  }

  @override
  Future<AgentTunnelRouteCleanup> joined() async {
    _live();
    return parseAgentTunnelRouteCleanup(await guard(original.joined));
  }

  @override
  void dispose() {
    if (_disposed || _disposing) return;
    _disposing = true;
    try {
      _sync(original.dispose);
      _disposed = true;
    } finally {
      _disposing = false;
    }
  }
}
