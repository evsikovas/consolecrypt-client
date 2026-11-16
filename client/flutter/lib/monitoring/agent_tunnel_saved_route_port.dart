import 'dart:convert';

import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_route_models.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_route_port.dart';

/// Optional exact-original lookup; no preparation, signing or automatic retry.
abstract interface class AgentTunnelSavedRoutePort {
  /// Owned by this service instance; metadata only, never an opaque handle.
  AgentTunnelSavedIntentCache get savedIntents;
  Future<AgentTunnelRouteHandle?> attachSaved(RuntimeNoticeScope scope, AgentTunnelRouteIntent intent);
}

abstract interface class AgentTunnelDetachableRouteHandle implements AgentTunnelRouteHandle {
  Future<AgentTunnelDetachReceipt> detachAutoOpen(AgentTunnelRouteView original);
}

/// A local UI ownership handoff, never a process/remote join or a new grant.
final class AgentTunnelDetachReceipt {
  const AgentTunnelDetachReceipt._(this.original, this.remainingMs);
  final AgentTunnelRouteView original;
  final int remainingMs;
  DateTime get grantExpiresUtc => original.grantExpiresUtc;
  String get boundAddress => original.boundAddress!;
  bool get originalOperationRetained => true;
  AgentTunnelCleanupState get cleanupState => AgentTunnelCleanupState.pending;
  bool get remoteDrainObserved => false;
}

/// Last observed explicit handoff, not a live-route or readiness assertion.
final class AgentTunnelRetainedRoute {
  const AgentTunnelRetainedRoute._(this.routeId, this.associationId, this.hostId, this.hostRevision,
    this.agentId, this.destinationIp, this.destinationPort, this.expiresUtc, this.metadataVersion);
  final String routeId, associationId, hostId, hostRevision, agentId, destinationIp;
  final int destinationPort, metadataVersion;
  final DateTime expiresUtc;
  String get destination => '${destinationIp == '::1' ? '[::1]' : destinationIp}:$destinationPort';
  bool matchesEndpoint(String endpoint) {
    final uri = Uri.tryParse(endpoint);
    return uri != null && const {'http', 'https'}.contains(uri.scheme) &&
      (uri.host == destinationIp || uri.host == '[$destinationIp]') && uri.port == destinationPort;
  }
}

/// At most 32 finite intent records in this service's memory. No handle, key,
/// signed grant, task, socket, persistence or process-restart restoration.
final class AgentTunnelSavedIntentCache {
  AgentTunnelSavedIntentCache({DateTime Function()? now}) : _now = now ?? DateTime.now;
  final DateTime Function() _now;
  final List<_IntentRecord> _records = [];
  int _nextMetadataVersion = 0;
  void _prune() => _records.removeWhere(
    (row) => row.clock.elapsedMilliseconds >= row.budgetMs || !_now().toUtc().isBefore(row.expiry),
  );
  List<AgentTunnelRetainedRoute> retainedRoutes(RuntimeNoticeScope scope) {
    _prune();
    return List.unmodifiable([
      for (final row in _records.reversed)
        if (row.scope.same(scope) && row.routeId != null)
          AgentTunnelRetainedRoute._(row.routeId!, row.association, row.host, row.revision, row.agent,
            row.destinationIp, row.destinationPort, row.expiry, row.metadataVersion),
    ]);
  }

  AgentTunnelRouteIntent? restore(RuntimeNoticeScope scope, AgentAssociationRecord current) {
    _prune();
    for (final row in _records.reversed) {
      if (row.matches(scope, current)) {
        return AgentTunnelRouteIntent(
          record: current,
          listenIp: row.listenIp,
          listenPort: row.listenPort,
          destinationIp: row.destinationIp,
          destinationPort: row.destinationPort,
          ttlSeconds: row.ttl,
          idleSeconds: row.idle,
          resumeOriginal: true,
        );
      }
    }
    return null;
  }

  int? metadataVersion(RuntimeNoticeScope scope, AgentTunnelRouteIntent intent) {
    _prune();
    for (final row in _records.reversed) {
      if (row.matches(scope, intent.record) && row.sameChoices(intent)) return row.metadataVersion;
    }
    return null;
  }

  /// Local metadata CAS only; never serialized or passed to Native custody.
  void forget(RuntimeNoticeScope scope, AgentTunnelRouteIntent intent, {required int? expectedVersion}) {
    if (expectedVersion == null) return;
    _records.removeWhere((row) => row.metadataVersion == expectedVersion && row.matches(scope, intent.record));
  }

  int? retainValidated(
    RuntimeNoticeScope scope,
    AgentTunnelRouteIntent intent,
    AgentTunnelDetachReceipt receipt,
    int originalRemainingMs,
  ) {
    final view = receipt.original;
    if (!intent.resumeOriginal ||
        !scope.same(view.scope) ||
        view.associationId != intent.record.id ||
        view.hostId != intent.record.hostId ||
        view.hostRevision != intent.record.hostRevision ||
        view.certificateDigest != intent.record.certificateDigest ||
        view.node.agentId != intent.record.agentId ||
        view.destination.ip != intent.destinationIp ||
        view.destination.port != intent.destinationPort ||
        view.clientBind.ip != intent.listenIp ||
        view.clientBind.port != intent.listenPort) {
      return null;
    }
    final wall = view.grantExpiresUtc.difference(_now().toUtc()).inMilliseconds;
    final budget = [originalRemainingMs, receipt.remainingMs, wall].reduce((a, b) => a < b ? a : b);
    if (budget <= 0) return null;
    _prune();
    _records.removeWhere((row) => row.matches(scope, intent.record));
    if (_records.length >= 32) _records.removeAt(0);
    final version = ++_nextMetadataVersion;
    _records.add(_IntentRecord(scope, intent, view, budget, version));
    return version;
  }
}

final class _IntentRecord {
  _IntentRecord(
    this.scope,
    AgentTunnelRouteIntent intent,
    AgentTunnelRouteView view,
    this.budgetMs,
    this.metadataVersion,
  ) : routeId = view.routeId,
      association = view.associationId,
      host = view.hostId,
      revision = view.hostRevision,
      certificate = view.certificateDigest,
      agent = view.node.agentId,
      epoch = view.node.keyEpoch,
      fingerprint = view.node.keyFingerprint,
      expiry = view.grantExpiresUtc,
      listenIp = intent.listenIp,
      listenPort = intent.listenPort,
      destinationIp = intent.destinationIp,
      destinationPort = intent.destinationPort,
      ttl = intent.ttlSeconds,
      idle = intent.idleSeconds;
  final RuntimeNoticeScope scope;
  final String? routeId;
  final String association, host, revision, certificate, agent, epoch, fingerprint, listenIp, destinationIp;
  final int listenPort, destinationPort, ttl, idle, budgetMs, metadataVersion;
  final DateTime expiry;
  final Stopwatch clock = Stopwatch()..start();
  bool matches(RuntimeNoticeScope selected, AgentAssociationRecord current) =>
      scope.same(selected) &&
      current.canRequestTunnel &&
      current.id == association &&
      current.hostId == host &&
      current.hostRevision == revision &&
      current.certificateDigest == certificate &&
      current.agentId == agent &&
      current.nodeKeyEpoch == epoch &&
      current.nodeKeyFingerprint == fingerprint;
  bool sameChoices(AgentTunnelRouteIntent intent) =>
      intent.resumeOriginal &&
      listenIp == intent.listenIp &&
      listenPort == intent.listenPort &&
      destinationIp == intent.destinationIp &&
      destinationPort == intent.destinationPort &&
      ttl == intent.ttlSeconds &&
      idle == intent.idleSeconds;
}

Never _invalid() => throw const FormatException('Original tunnel handoff unavailable');

AgentTunnelDetachReceipt parseAgentTunnelDetachReceipt(String raw, AgentTunnelRouteView original) {
  if (raw.isEmpty ||
      utf8.encode(raw).length > 8192 ||
      !original.resumeOriginalSignedOptIn ||
      !original.resumeTransportImplemented ||
      original.boundAddress == null ||
      !const {
        AgentTunnelRoutePhase.listening,
        AgentTunnelRoutePhase.connecting,
        AgentTunnelRoutePhase.connected,
      }.contains(original.phase)) {
    _invalid();
  }
  final value = jsonDecode(raw);
  const keys = {
    'version',
    'profileId',
    'notificationScopeId',
    'phase',
    'associationId',
    'destination',
    'boundAddress',
    'grantExpiresUtc',
    'remainingMs',
    'originalOperationRetained',
    'cleanupState',
    'remoteDrainObserved',
    'countersExact',
    'nativeAccountingOnly',
  };
  if (value is! Map<String, dynamic> || value.length != keys.length || !value.keys.every(keys.contains)) {
    _invalid();
  }
  void fixed(String key, Object expected) {
    final actual = value[key];
    if (actual != expected || actual.runtimeType != expected.runtimeType) {
      _invalid();
    }
  }

  fixed('version', 1);
  fixed('profileId', original.scope.profileId);
  fixed('notificationScopeId', original.scope.nonce);
  fixed('phase', 'detached');
  fixed('associationId', original.associationId);
  fixed('boundAddress', original.boundAddress!);
  fixed('originalOperationRetained', true);
  fixed('cleanupState', 'pending');
  fixed('remoteDrainObserved', false);
  fixed('countersExact', false);
  fixed('nativeAccountingOnly', true);
  final destination = value['destination'];
  if (destination is! Map<String, dynamic> ||
      destination.length != 3 ||
      !destination.keys.every(const {'protocol', 'ip', 'port'}.contains) ||
      destination['protocol'] != 'tcp' ||
      destination['ip'] != original.destination.ip ||
      destination['port'] is! int ||
      destination['port'] != original.destination.port) {
    _invalid();
  }
  final expiry = value['grantExpiresUtc'];
  if (expiry is! String || expiry.length > 32 || !RegExp(r'^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$').hasMatch(expiry)) {
    _invalid();
  }
  final parsed = DateTime.tryParse(expiry);
  if (parsed == null ||
      !parsed.isUtc ||
      parsed != original.grantExpiresUtc ||
      parsed.toIso8601String().replaceFirst('.000Z', 'Z') != expiry) {
    _invalid();
  }
  final remaining = value['remainingMs'];
  if (remaining is! int || remaining < 1 || remaining > 60000 || remaining > original.remainingMs) {
    _invalid();
  }
  return AgentTunnelDetachReceipt._(original, remaining);
}
