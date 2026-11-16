import 'dart:convert';

import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:flutter/foundation.dart';

/// Read-only data about one saved association and canonical TCP destination.
/// A stored preference never grants replay, renewal or a different lease.
final class AgentTunnelResumeQuery {
  AgentTunnelResumeQuery(this.record, this.ip, this.port) {
    checkedAssociationId(record.id);
    if (!record.canRequestTunnel || !const {'127.0.0.1', '::1'}.contains(ip) || port < 1 || port > 65535) {
      throw const FormatException('Tunnel resume destination unavailable');
    }
  }
  final AgentAssociationRecord record;
  final String ip;
  final int port;
  bool same(AgentTunnelResumeQuery? other) =>
      other != null && identical(record, other.record) && ip == other.ip && port == other.port;
  Map<String, Object> request(RuntimeNoticeScope scope) => {
    'version': 1,
    'profileId': scope.profileId,
    'notificationScopeId': scope.nonce,
    'associationId': record.id,
    'destinationIp': ip,
    'destinationPort': port,
  };
}

final class AgentTunnelResumePreference {
  const AgentTunnelResumePreference(this.query, this.resumeOriginal);
  final AgentTunnelResumeQuery query;
  final bool resumeOriginal;
}

abstract interface class AgentTunnelResumePreferencePort {
  Future<AgentTunnelResumePreference> readResumePreference(RuntimeNoticeScope scope, AgentTunnelResumeQuery query);
}

AgentTunnelResumePreference parseAgentTunnelResumePreference(
  String raw,
  RuntimeNoticeScope scope,
  AgentTunnelResumeQuery query,
) {
  Never bad() => throw const FormatException('Tunnel resume preference unavailable');
  Map<String, dynamic> keys(Object? v, Set<String> fields) {
    if (v is! Map<String, dynamic> || v.length != fields.length || !v.keys.every(fields.contains)) bad();
    return v;
  }

  if (raw.isEmpty || utf8.encode(raw).length > 4096) bad();
  final m = keys(jsonDecode(raw), {
    'version',
    'profileId',
    'notificationScopeId',
    'associationId',
    'destination',
    'resumeOriginal',
  });
  if (m['version'] is! int ||
      m['version'] != 1 ||
      m['profileId'] != scope.profileId ||
      m['notificationScopeId'] != scope.nonce ||
      m['associationId'] != query.record.id ||
      m['resumeOriginal'] is! bool) {
    bad();
  }
  final d = keys(m['destination'], {'protocol', 'ip', 'port'});
  if (d['protocol'] != 'tcp' || d['ip'] != query.ip || d['port'] is! int || d['port'] != query.port) bad();
  return AgentTunnelResumePreference(query, m['resumeOriginal'] as bool);
}

/// One in-flight explicit read. Destination changes retire the returned data,
/// but never permit another read to overtake a still-pending original one.
final class AgentTunnelResumePreferenceController extends ChangeNotifier {
  AgentTunnelResumePreferenceController({required this.port, required this.scope, required this.original});
  final AgentTunnelResumePreferencePort? port;
  final RuntimeNoticeScope scope;
  final bool Function() original;
  AgentTunnelResumeQuery? selected;
  AgentTunnelResumePreference? value;
  Object? error;
  bool busy = false, _disposed = false, _retired = false;
  int _generation = 0;
  bool get current => !_disposed && !_retired && original();
  bool get canRead => current && port != null && selected != null && !busy;
  void _emit() {
    if (!_disposed) notifyListeners();
  }

  void select(AgentTunnelResumeQuery? query) {
    if (!current) {
      retire();
      return;
    }
    if (selected?.same(query) == true) return;
    _generation++;
    selected = query;
    value = null;
    error = null;
    _emit();
  }

  Future<void> read() async {
    if (!canRead) return;
    final query = selected!, token = _generation;
    busy = true;
    error = null;
    _emit();
    try {
      if (!current) return;
      final result = await port!.readResumePreference(scope, query);
      if (current && token == _generation && selected?.same(query) == true) {
        if (!result.query.same(query)) throw const FormatException('Tunnel resume preference changed');
        value = result;
      }
    } catch (e) {
      if (current && token == _generation) error = e;
    } finally {
      busy = false;
      _emit();
    }
  }

  void retire() {
    if (_retired) return;
    _retired = true;
    _generation++;
    value = null;
    error = null;
    _emit();
  }

  @override
  void dispose() {
    if (_disposed) return;
    _disposed = true;
    retire();
    super.dispose();
  }
}
