import 'dart:convert';

import 'package:consolecrypt/core/bridge/mapping.dart' show guard, toAppException;
import 'package:consolecrypt/core/security/vault_key_alarm.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_outbound_bootstrap.dart';
import 'package:consolecrypt/src/rust/api/agent_node_association.dart' as native;

/// Typed original-handle seam shared by actual generated binding and component
/// tests. No later Core/profile lookup is performed on an issued review.
abstract interface class AgentOutboundBootstrapNativeHandle {
  Future<String> details();
  Future<String> approveOnce();
  void cancel();
  Future<String> joined();
  void dispose();
}

final class RustAgentOutboundBootstrapPort implements AgentOutboundBootstrapPort {
  const RustAgentOutboundBootstrapPort({required this.prepareOriginal});
  final Future<AgentOutboundBootstrapNativeHandle> Function(String requestJson) prepareOriginal;
  @override
  Future<AgentOutboundBootstrapHandle> prepare(RuntimeNoticeScope scope, AgentOutboundBootstrapIntent intent) async {
    final issued = await guard(() => prepareOriginal(jsonEncode(intent.request(scope))));
    return _Original(issued, scope, intent);
  }
}

/// Actual generated API, resolved lazily only on explicit Prepare. Construction
/// does not initialize FFI, connect a carrier, enroll a Node or approve anything.
RustAgentOutboundBootstrapPort createGeneratedAgentOutboundBootstrapPort({
  Future<native.AgentTunnelControlHandle> Function({required String requestJson}) prepare =
      native.agentTunnelControlBootstrapPrepare,
}) => RustAgentOutboundBootstrapPort(
  prepareOriginal: (requestJson) async => _GeneratedOriginal(await prepare(requestJson: requestJson)),
);

final class _GeneratedOriginal implements AgentOutboundBootstrapNativeHandle {
  const _GeneratedOriginal(this.original);
  final native.AgentTunnelControlHandle original;
  @override
  Future<String> details() => original.details();
  @override
  Future<String> approveOnce() => original.approveOnce();
  @override
  void cancel() => original.cancel();
  @override
  Future<String> joined() => original.joined();
  @override
  void dispose() => original.dispose();
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

final class _Original implements AgentOutboundBootstrapHandle {
  _Original(this.original, this.scope, this.intent);
  final AgentOutboundBootstrapNativeHandle original;
  final RuntimeNoticeScope scope;
  final AgentOutboundBootstrapIntent intent;
  bool _disposed = false;
  void _live() {
    if (_disposed) throw StateError('Original outbound proxy disposed');
  }

  @override
  Future<AgentOutboundBootstrapView> details() async {
    _live();
    return parseAgentOutboundBootstrapView(await guard(original.details), scope, intent);
  }

  @override
  Future<AgentOutboundBootstrapView> approveOnce() async {
    _live();
    return parseAgentOutboundBootstrapView(await guard(original.approveOnce), scope, intent);
  }

  @override
  void cancel() {
    _live();
    _sync(original.cancel);
  }

  @override
  Future<AgentOutboundBootstrapCleanup> joined() async {
    _live();
    return parseAgentOutboundBootstrapCleanup(await guard(original.joined));
  }

  @override
  void dispose() {
    if (_disposed) return;
    _disposed = true;
    _sync(original.dispose);
  }
}
