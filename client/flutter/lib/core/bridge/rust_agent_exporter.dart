import 'dart:convert';

import 'package:consolecrypt/core/bridge/mapping.dart' show guard, toAppException;
import 'package:consolecrypt/core/bridge/rust_agent_association.dart';
import 'package:consolecrypt/core/security/vault_key_alarm.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_association_port.dart';
import 'package:consolecrypt/monitoring/agent_exporter.dart';
import 'package:consolecrypt/src/rust/api/agent_node_association.dart' as existing;

/// Handwritten callbacks for the actual authored Bridge signatures. No guessed
/// generated exporter import or production factory activation is present.
abstract interface class AgentExporterNativeCalls {
  Future<String> catalog(String requestJson);
  Future<AgentExporterOriginal> prepare(String requestJson);
  Future<existing.AgentNodeAssociationHandle> registerExporters(String requestJson);
  Future<existing.AgentNodeRevocationHandle> revokeExporters(String requestJson);
}

abstract interface class AgentExporterOriginal {
  Future<String> details();
  Future<String> approveOnce();
  Future<String> stop();
  void cancel();
  Future<String> joined();
  void dispose();
}

Map<String, Object> _frame(RuntimeNoticeScope s) => {
  'version': 1,
  'profileId': s.profileId,
  'notificationScopeId': s.nonce,
};

final class RustAgentExporterPort implements AgentExporterPort, AgentExporterEnrollmentPort {
  const RustAgentExporterPort(this.calls);
  final AgentExporterNativeCalls calls;
  void _record(AgentAssociationRecord r) {
    if (!canRequestAgentExporter(r)) {
      throw const FormatException('Exporter association required');
    }
  }

  @override
  Future<AgentExporterCatalog> catalog(RuntimeNoticeScope s, AgentAssociationRecord r) async {
    _record(r);
    return parseAgentExporterCatalog(
      await guard(() => calls.catalog(jsonEncode({..._frame(s), 'associationId': r.id}))),
      s,
      r,
    );
  }

  @override
  Future<AgentExporterHandle> prepare(
    RuntimeNoticeScope s,
    AgentAssociationRecord r,
    AgentExporterCatalog c,
    int seconds,
  ) async {
    _record(r);
    if (c.policy.hostId != r.hostId ||
        c.policy.agentId != r.agentId ||
        seconds < 30 ||
        seconds > 300 ||
        seconds > c.policy.maxSeconds) {
      throw const FormatException('Exporter duration or catalog changed');
    }
    final original = await guard(
      () => calls.prepare(
        jsonEncode({
          ..._frame(s),
          'associationId': r.id,
          'manifestSha256': c.manifestSha256,
          'durationSeconds': seconds,
        }),
      ),
    );
    return _Original(original, s, r, c);
  }

  @override
  Future<AgentAssociationHandle> registerExporterProbe(RuntimeNoticeScope s, AgentAssociationHost h) async =>
      originalRustAgentAssociationHandle(
        await guard(
          () => calls.registerExporters(jsonEncode({..._frame(s), 'hostId': h.id, 'expectedHostRevision': h.revision})),
        ),
        s,
        h,
        AgentAssociationPurpose.exporters,
      );
  @override
  Future<AgentRevocationHandle> prepareExporterRevocation(RuntimeNoticeScope s, AgentAssociationRecord r) async {
    if (r.purpose != AgentAssociationPurpose.exporters) {
      throw const FormatException('Exporter association required');
    }
    return originalRustAgentRevocationHandle(
      await guard(() => calls.revokeExporters(jsonEncode({..._frame(s), 'associationId': r.id}))),
      s,
      r,
    );
  }
}

final class _Original implements AgentExporterHandle {
  _Original(this.raw, this.scope, this.record, this.catalog);
  final AgentExporterOriginal raw;
  final RuntimeNoticeScope scope;
  final AgentAssociationRecord record;
  final AgentExporterCatalog catalog;
  bool _spent = false, _disposed = false, _disposing = false;
  void _live() {
    if (_disposed || _disposing) {
      throw StateError('Exporter handle released');
    }
  }

  AgentExporterView? _last;
  AgentExporterView _parse(String s) {
    final value = parseAgentExporterView(s, scope, record, catalog);
    if (_last?.runtimeObserved == true && !value.runtimeObserved) {
      throw const FormatException('Exporter observation regressed');
    }
    _last = value;
    return value;
  }

  @override
  Future<AgentExporterView> details() async {
    _live();
    return _parse(await guard(raw.details));
  }

  @override
  Future<AgentExporterView> approveOnce() async {
    _live();
    if (_spent) {
      throw StateError('Exporter approval consumed');
    }
    _spent = true;
    return _parse(await guard(raw.approveOnce));
  }

  @override
  Future<AgentExporterView> stop() async {
    _live();
    return _parse(await guard(raw.stop));
  }

  @override
  void cancel() {
    _live();
    try {
      raw.cancel();
    } catch (e) {
      final mapped = toAppException(e);
      reportVaultKeyMismatch(mapped);
      throw mapped;
    }
  }

  @override
  Future<AgentExporterCleanup> joined() async {
    _live();
    final value = parseAgentExporterCleanup(await guard(raw.joined));
    if (_last?.runtimeObserved == true && !value.runtimeObserved) {
      throw const FormatException('Exporter observation regressed');
    }
    return value;
  }

  @override
  void dispose() {
    if (_disposed || _disposing) {
      return;
    }
    _disposing = true;
    try {
      raw.dispose();
      _disposed = true;
    } finally {
      _disposing = false;
    }
  }
}
