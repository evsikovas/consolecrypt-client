import 'dart:convert';

import 'package:consolecrypt/core/bridge/mapping.dart' show guard;
import 'package:consolecrypt/core/bridge/rust_agent_tunnel_route.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_resume_preference.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_route_models.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_route_port.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_saved_route_port.dart';
import 'package:consolecrypt/src/rust/api/agent_tunnel_route.dart' as native;

/// Exact authored Rust signatures. No new generated names are referenced until
/// Root's real generation; the existing issued opaque type is unchanged.
abstract interface class AgentTunnelSavedRouteNativeCalls implements AgentTunnelRouteNativeCalls {
  Future<native.AgentTunnelRouteHandle?> attachSaved(String requestJson);
  Future<String> detachAutoOpen(native.AgentTunnelRouteHandle original);
}

final class RustAgentTunnelRouteWithSavedOriginal
    implements AgentTunnelRoutePort, AgentTunnelSavedRoutePort, AgentTunnelResumePreferencePort {
  RustAgentTunnelRouteWithSavedOriginal({required this.calls, required this.preferences});
  final AgentTunnelSavedRouteNativeCalls calls;
  final AgentTunnelResumePreferencePort preferences;
  @override final AgentTunnelSavedIntentCache savedIntents = AgentTunnelSavedIntentCache();
  AgentTunnelRouteHandle _issue(native.AgentTunnelRouteHandle raw, RuntimeNoticeScope scope, AgentTunnelRouteIntent intent) =>
      _Saved(originalRustTunnelRouteHandle(raw, scope, intent), () => calls.detachAutoOpen(raw));
  @override
  Future<AgentTunnelRouteHandle> prepare(RuntimeNoticeScope scope, AgentTunnelRouteIntent intent) async =>
      _issue(await guard(() => calls.prepare(jsonEncode(intent.request(scope)))), scope, intent);
  @override
  Future<AgentTunnelRouteHandle?> attachSaved(RuntimeNoticeScope scope, AgentTunnelRouteIntent intent) async {
    if (!intent.resumeOriginal) throw const FormatException('Explicit original tunnel opt-in required');
    final raw = await guard(() => calls.attachSaved(jsonEncode(intent.request(scope))));
    return raw == null ? null : _issue(raw, scope, intent);
  }
  @override
  Future<AgentTunnelAccessAudit> audit(RuntimeNoticeScope scope) => RustAgentTunnelRoutePort(calls: calls).audit(scope);
  @override
  Future<AgentTunnelResumePreference> readResumePreference(RuntimeNoticeScope scope, AgentTunnelResumeQuery query) =>
      preferences.readResumePreference(scope, query);
}

final class _Saved implements AgentTunnelDetachableRouteHandle {
  _Saved(this.original, this.detachOriginal);
  final AgentTunnelRouteHandle original;
  final Future<String> Function() detachOriginal;
  AgentTunnelRouteView? _observed;
  bool _disposed = false, _disposing = false, _detachSpent = false;
  void _live() { if (_disposed || _disposing) throw StateError('Original tunnel proxy disposed or releasing'); }
  Future<AgentTunnelRouteView> _read(Future<AgentTunnelRouteView> Function() call) async {
    _live();
    final value = await call(); _observed = value; return value;
  }
  @override Future<AgentTunnelRouteView> details() => _read(original.details);
  @override Future<AgentTunnelRouteView> approveOnce() => _read(original.approveOnce);
  @override Future<AgentTunnelRouteView> stop() => _read(original.stop);
  @override void cancel() { _live(); original.cancel(); }
  @override Future<AgentTunnelRouteCleanup> joined() { _live(); return original.joined(); }
  @override
  Future<AgentTunnelDetachReceipt> detachAutoOpen(AgentTunnelRouteView originalView) async {
    _live();
    if (_detachSpent || !identical(originalView, _observed)) {
      throw const FormatException('Original tunnel handoff review changed or spent');
    }
    _detachSpent = true;
    return parseAgentTunnelDetachReceipt(await guard(detachOriginal), originalView);
  }
  @override
  void dispose() {
    if (_disposed || _disposing) return;
    _disposing = true;
    try {
      original.dispose();
      _disposed = true; _observed = null;
    } finally {
      _disposing = false;
    }
  }
}
