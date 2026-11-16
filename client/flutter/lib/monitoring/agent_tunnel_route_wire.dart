import 'dart:convert';

import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_route_models.dart';

Never _bad() => throw const FormatException('Tunnel observation invalid');
typedef _Json = Map<String, dynamic>;
_Json _object(Object? v) => v is _Json ? v : _bad();
_Json _keys(Object? v, Set<String> keys) {
  final m = _object(v);
  if (m.length != keys.length || !m.keys.every(keys.contains)) _bad();
  return m;
}

_Json _decode(String raw, int cap) {
  if (raw.isEmpty || utf8.encode(raw).length > cap) _bad();
  return _object(jsonDecode(raw));
}

String _text(Object? v, int cap) =>
    v is String &&
        v.isNotEmpty &&
        utf8.encode(v).length <= cap &&
        !v.runes.any((r) => r < 32 || r >= 127 && r <= 159 || r >= 0xd800 && r <= 0xdfff)
    ? v
    : _bad();
String _id(Object? v) => checkedAssociationId(_text(v, 36));
String _sha(Object? v) => associationHash(_text(v, 64));
String _decimal(Object? v, {bool positive = false, bool signed = false}) =>
    associationNumber(_text(v, 20), positive: positive, signed: signed);
int _int(Object? v, int min, int max) => v is int && v >= min && v <= max ? v : _bad();
bool _bool(Object? v) => v is bool ? v : _bad();
void _fixed(Object? v, Object expected) {
  if (v != expected || v.runtimeType != expected.runtimeType) _bad();
}

String _loopback(Object? v) => v == '127.0.0.1' || v == '::1' ? v as String : _bad();
void _frame(_Json m, RuntimeNoticeScope scope) {
  _fixed(m['version'], 1);
  _fixed(m['profileId'], scope.profileId);
  _fixed(m['notificationScopeId'], scope.nonce);
}

AgentTunnelRoutePhase _phase(Object? value) =>
    AgentTunnelRoutePhase.values.where((v) => v.name == value).firstOrNull ?? _bad();
AgentTunnelCleanupState _cleanup(Object? value) =>
    AgentTunnelCleanupState.values.where((v) => v.name == value).firstOrNull ?? _bad();
DateTime _utc(Object? v) {
  final raw = _text(v, 32);
  if (!RegExp(r'^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$').hasMatch(raw)) _bad();
  final d = DateTime.tryParse(raw);
  if (d == null || !d.isUtc || d.toIso8601String().replaceFirst('.000Z', 'Z') != raw) _bad();
  return d;
}

AgentTunnelRouteView parseAgentTunnelRouteView(String raw, RuntimeNoticeScope scope, AgentTunnelRouteIntent intent) {
  final decoded = _decode(raw, 16384);
  final m = _keys(decoded, {
    if (decoded.containsKey('routeId')) 'routeId',
    'version',
    'profileId',
    'notificationScopeId',
    'phase',
    'associationId',
    'hostId',
    'hostRevision',
    'owner',
    'node',
    'certificateDigest',
    'destination',
    'clientBind',
    'limits',
    'resumeOriginalSignedOptIn',
    'resumeTransportImplemented',
    'grantExpiresUtc',
    'remainingMs',
    'boundAddress',
    'actorAdmission',
    'remoteDrainObserved',
    'cleanupState',
    'sentBytes',
    'receivedBytes',
    'countersExact',
    'nativeAccountingOnly',
  });
  _frame(m, scope);
  _fixed(m['resumeOriginalSignedOptIn'], intent.resumeOriginal);
  final resumeTransportImplemented = _boolean(m['resumeTransportImplemented']);
  _fixed(m['remoteDrainObserved'], false);
  _fixed(m['countersExact'], false);
  _fixed(m['nativeAccountingOnly'], true);
  final association = _id(m['associationId']),
      host = _id(m['hostId']),
      revision = _decimal(m['hostRevision'], positive: true, signed: true),
      certificate = _sha(m['certificateDigest']);
  if (association != intent.record.id ||
      host != intent.record.hostId ||
      revision != intent.record.hostRevision ||
      certificate != intent.record.certificateDigest) {
    _bad();
  }
  final owner = _keys(m['owner'], {'kind', 'id', 'deviceId', 'signingKeySha256'}), ownerId = _id(owner['id']);
  if (!const {'localProfile', 'personalAccount'}.contains(owner['kind']) ||
      owner['kind'] == 'localProfile' && ownerId != scope.profileId) {
    _bad();
  }
  final node = _keys(m['node'], {'agentId', 'keyEpoch', 'publicKey', 'keyFingerprint'}),
      agent = _id(node['agentId']),
      keyEpoch = _decimal(node['keyEpoch'], positive: true),
      fingerprint = _sha(node['keyFingerprint']);
  if (agent != intent.record.agentId ||
      intent.record.nodeKeyFingerprint != null && fingerprint != intent.record.nodeKeyFingerprint ||
      intent.record.nodeKeyEpoch != null && keyEpoch != intent.record.nodeKeyEpoch) {
    _bad();
  }
  final destination = _keys(m['destination'], {'protocol', 'ip', 'port'}),
      bind = _keys(m['clientBind'], {'ip', 'port'});
  _fixed(destination['protocol'], 'tcp');
  final destinationIp = _loopback(destination['ip']),
      destinationPort = _int(destination['port'], 1, 65535),
      bindIp = _loopback(bind['ip']),
      bindPort = _int(bind['port'], 0, 65535);
  if (destinationIp != intent.destinationIp ||
      destinationPort != intent.destinationPort ||
      bindIp != intent.listenIp ||
      bindPort != intent.listenPort) {
    _bad();
  }
  final limits = _keys(m['limits'], {
    'maxTtlSeconds',
    'idleTimeoutSeconds',
    'maxChannels',
    'maxFrameBytes',
    'maxPendingFramesPerChannel',
    'maxPendingFramesTotal',
    'maxQueuedBytesPerChannel',
    'maxQueuedBytesTotal',
    'maxBytesPerChannel',
    'maxBytesTotal',
  });
  final ttl = _int(limits['maxTtlSeconds'], 1, intent.ttlSeconds),
      idle = _int(limits['idleTimeoutSeconds'], 1, intent.idleSeconds);
  if (idle > ttl) _bad();
  for (final item in <String, int>{
    'maxChannels': 1,
    'maxFrameBytes': 8192,
    'maxPendingFramesPerChannel': 1,
    'maxPendingFramesTotal': 2,
    'maxQueuedBytesPerChannel': 8192,
    'maxQueuedBytesTotal': 16384,
    'maxBytesPerChannel': 4194304,
    'maxBytesTotal': 4194304,
  }.entries) {
    _fixed(limits[item.key], item.value);
  }
  final phase = _phase(m['phase']), admission = _bool(m['actorAdmission']);
  if (phase == AgentTunnelRoutePhase.connected && !admission ||
      phase == AgentTunnelRoutePhase.routeReview && admission) {
    _bad();
  }
  String? address;
  if (m['boundAddress'] != null) {
    address = _text(m['boundAddress'], 64);
    final match = RegExp(r'^(127\.0\.0\.1|\[::1\]):([1-9][0-9]{0,4})$').firstMatch(address);
    if (match == null) _bad();
    final ip = match[1] == '[::1]' ? '::1' : match[1], port = int.parse(match[2]!);
    if (ip != intent.listenIp || port > 65535 || intent.listenPort != 0 && port != intent.listenPort) _bad();
  }
  if (const {
        AgentTunnelRoutePhase.listening,
        AgentTunnelRoutePhase.connecting,
        AgentTunnelRoutePhase.connected,
        AgentTunnelRoutePhase.stopRequested,
      }.contains(phase) &&
      address == null) {
    _bad();
  }
  return AgentTunnelRouteView(
    resumeOriginalSignedOptIn: intent.resumeOriginal,
    resumeTransportImplemented: resumeTransportImplemented,
    scope: scope,
    routeId: decoded.containsKey('routeId') ? _id(m['routeId']) : null,
    phase: phase,
    associationId: association,
    hostId: host,
    hostRevision: revision,
    owner: AgentTunnelOwner(owner['kind'] as String, ownerId, _id(owner['deviceId']), _sha(owner['signingKeySha256'])),
    node: AgentTunnelNode(agent, keyEpoch, _sha(node['publicKey']), fingerprint),
    certificateDigest: certificate,
    destination: AgentTunnelEndpoint(destinationIp, destinationPort),
    clientBind: AgentTunnelEndpoint(bindIp, bindPort),
    limits: AgentTunnelLimits(
      maxTtlSeconds: ttl,
      idleTimeoutSeconds: idle,
      maxChannels: 1,
      maxFrameBytes: 8192,
      maxPendingFramesPerChannel: 1,
      maxPendingFramesTotal: 2,
      maxQueuedBytesPerChannel: 8192,
      maxQueuedBytesTotal: 16384,
      maxBytesPerChannel: 4194304,
      maxBytesTotal: 4194304,
    ),
    grantExpiresUtc: _utc(m['grantExpiresUtc']),
    remainingMs: _int(m['remainingMs'], 1, 60000),
    boundAddress: address,
    actorAdmission: admission,
    cleanupState: _cleanup(m['cleanupState']),
    sentBytes: _decimal(m['sentBytes']),
    receivedBytes: _decimal(m['receivedBytes']),
  );
}

AgentTunnelRouteCleanup parseAgentTunnelRouteCleanup(String raw) {
  final m = _keys(_decode(raw, 4096), {
    'version',
    'phase',
    'remoteDrainObserved',
    'countersExact',
    'nativeAccountingOnly',
    'sentBytes',
    'receivedBytes',
    'actorAdmission',
    'cleanupState',
  });
  _fixed(m['version'], 1);
  _fixed(m['remoteDrainObserved'], false);
  _fixed(m['countersExact'], false);
  _fixed(m['nativeAccountingOnly'], true);
  if (!const {'closedObserved', 'unknown'}.contains(m['cleanupState'])) _bad();
  return AgentTunnelRouteCleanup(
    phase: _phase(m['phase']),
    state: _cleanup(m['cleanupState']),
    sentBytes: _decimal(m['sentBytes']),
    receivedBytes: _decimal(m['receivedBytes']),
    actorAdmission: _bool(m['actorAdmission']),
  );
}

// Native audit currently serializes integer JSON fields. Reject numbers beyond
// exact interoperable JSON integer range; never round a revision/sequence.
const _safe = 9007199254740991;
String _auditNumber(Object? v, {bool positive = false}) => _int(v, positive ? 1 : 0, _safe).toString();
DateTime _epoch(Object? v) => DateTime.fromMillisecondsSinceEpoch(_int(v, 1, 253402300799) * 1000, isUtc: true);
AgentTunnelAccessAudit parseAgentTunnelAccessAudit(String raw, RuntimeNoticeScope scope) {
  final m = _keys(_decode(raw, 196608), {
    'version',
    'profileId',
    'notificationScopeId',
    'boundedLocalObservations',
    'immutableLog',
    'remoteDeliveryGuarantee',
    'rows',
  });
  _frame(m, scope);
  _fixed(m['boundedLocalObservations'], true);
  _fixed(m['immutableLog'], false);
  _fixed(m['remoteDeliveryGuarantee'], false);
  final input = m['rows'];
  if (input is! List || input.length > 128) _bad();
  final rows = <AgentTunnelAccessRow>[];
  var previous = BigInt.zero;
  for (final value in input) {
    final row = _keys(value, {'sequence', 'profileId', 'deviceId', 'personalUserId', 'record'}),
        sequence = _auditNumber(row['sequence'], positive: true),
        profile = _id(row['profileId']),
        device = _id(row['deviceId']);
    if (profile != scope.profileId || BigInt.parse(sequence) <= previous) _bad();
    previous = BigInt.parse(sequence);
    final personal = row['personalUserId'] == null ? null : _id(row['personalUserId']);
    if (rows.isNotEmpty && (rows.first.deviceId != device || rows.first.personalUserId != personal)) _bad();
    final r = _keys(row['record'], {
      'operationId',
      'associationId',
      'hostId',
      'hostRevision',
      'grantId',
      'grantDigest',
      'policyDigest',
      'agentId',
      'keyEpoch',
      'protocol',
      'destinationIp',
      'destinationPort',
      'expiresAt',
      'observedAt',
      'event',
      'sent',
      'received',
      'countersExact',
      'nativeControlFinished',
    });
    _fixed(r['protocol'], 'tcp');
    final event = AgentTunnelAccessEvent.values.where((v) => v.name == r['event']).firstOrNull ?? _bad(),
        sent = _int(r['sent'], 0, 33554432),
        received = _int(r['received'], 0, 33554432),
        finished = _bool(r['nativeControlFinished']);
    if (sent + received > 33554432 ||
        const {AgentTunnelAccessEvent.openRequested, AgentTunnelAccessEvent.opened}.contains(event) &&
            (finished || sent != 0 || received != 0)) {
      _bad();
    }
    rows.add(
      AgentTunnelAccessRow(
        sequence: sequence,
        profileId: profile,
        deviceId: device,
        personalUserId: personal,
        record: AgentTunnelAccessRecord(
          operationId: _id(r['operationId']),
          associationId: _id(r['associationId']),
          hostId: _id(r['hostId']),
          hostRevision: _auditNumber(r['hostRevision'], positive: true),
          grantId: _id(r['grantId']),
          grantDigest: _sha(r['grantDigest']),
          policyDigest: _sha(r['policyDigest']),
          agentId: _id(r['agentId']),
          keyEpoch: _auditNumber(r['keyEpoch'], positive: true),
          destination: AgentTunnelEndpoint(_loopback(r['destinationIp']), _int(r['destinationPort'], 1, 65535)),
          expiresAt: _epoch(r['expiresAt']),
          observedAt: _epoch(r['observedAt']),
          event: event,
          sent: sent,
          received: received,
          countersExact: _bool(r['countersExact']),
          nativeControlFinished: finished,
        ),
      ),
    );
  }
  return AgentTunnelAccessAudit(scope, rows);
}

bool _boolean(Object? value) {
  if (value is! bool) throw const FormatException('Invalid route capability');
  return value;
}
