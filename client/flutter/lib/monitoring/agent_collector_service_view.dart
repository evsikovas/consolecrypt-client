import 'dart:convert';

import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_credentials.dart';
import 'package:consolecrypt/monitoring/agent_metrics_manifest.dart';
import 'package:consolecrypt/monitoring/agent_rotation.dart' show AgentRotationNode;

/// Read-only display of the actual finite-service producer. A recorded
/// configuration never claims a running process, delivery, or a new grant.
enum AgentCollectorServicePhase { serviceReview, configuring, configurationRecorded, unknown, cancelled }

final class AgentCollectorServiceBootStamp {
  const AgentCollectorServiceBootStamp(this.bootId, this.createdBoottimeNs, this.deadlineBoottimeNs);
  final String bootId, createdBoottimeNs, deadlineBoottimeNs;
}

final class AgentCollectorServiceConfigurationAck {
  const AgentCollectorServiceConfigurationAck(this.actorRevision, this.recordedAtUtc);
  final String actorRevision;
  final DateTime recordedAtUtc;
}

/// Actual immutable metadata from lifetime_entry::publish_lifetime. This is
/// display evidence; it cannot create a Node permission or a BOOTTIME stamp.
final class AgentCollectorServiceLifetimeCredentials {
  const AgentCollectorServiceLifetimeCredentials(this.reviewId, this.purpose, this.retentionUntilUtc);
  final String reviewId, purpose;
  final DateTime retentionUntilUtc;
  bool same(AgentCollectorServiceLifetimeCredentials other) =>
      reviewId == other.reviewId && purpose == other.purpose && retentionUntilUtc == other.retentionUntilUtc;
}

final class AgentCollectorServiceView {
  const AgentCollectorServiceView({
    required this.scope,
    required this.record,
    required this.phase,
    required this.ownerKind,
    required this.ownerId,
    required this.ownerDeviceId,
    required this.ownerSigningKeySha256,
    required this.node,
    required this.serviceId,
    required this.serviceHash,
    required this.catalog,
    required this.ownerEpoch,
    required this.checkpoint,
    required this.actorRevision,
    required this.policyRevision,
    required this.revocationEpoch,
    required this.remainingMs,
    required this.issuedAtUtc,
    required this.expiresAtUtc,
    required this.effectiveAuthoritySeconds,
    required this.cleanupState,
    this.bootStamp,
    this.configurationAck,
    this.credentialSelections = const [],
    this.lifetimeCredentials,
  });
  final RuntimeNoticeScope scope;
  final AgentAssociationRecord record;
  final AgentCollectorServicePhase phase;
  final String ownerKind, ownerId, ownerDeviceId, ownerSigningKeySha256;
  final AgentRotationNode node;
  final String serviceId, serviceHash;
  final AgentMetricsCollectorCatalog catalog;
  final String ownerEpoch, checkpoint, actorRevision, policyRevision, revocationEpoch;
  final int remainingMs, effectiveAuthoritySeconds;
  final DateTime issuedAtUtc, expiresAtUtc;
  final String cleanupState;
  final AgentCollectorServiceBootStamp? bootStamp;
  final AgentCollectorServiceConfigurationAck? configurationAck;
  final List<AgentCollectorServiceCredentialSelection> credentialSelections;
  final AgentCollectorServiceLifetimeCredentials? lifetimeCredentials;
  bool get configurationRecorded => phase == AgentCollectorServicePhase.configurationRecorded;
  // Exact fixed facts of this producer, never observed process state.
  bool get runtimeObserved => false;
  bool get continuesWhenClientLocks => true;
  bool get automaticRenewal => false;
  bool get deliveryGuaranteed => false;
  bool sameOriginal(AgentCollectorServiceView other) =>
      ((lifetimeCredentials == null && other.lifetimeCredentials == null) ||
          (lifetimeCredentials != null && other.lifetimeCredentials != null &&
              lifetimeCredentials!.same(other.lifetimeCredentials!))) &&
      scope.same(other.scope) &&
      identical(record, other.record) &&
      node.same(other.node) &&
      sameCollectorCredentialSelections(credentialSelections, other.credentialSelections) &&
      ownerKind == other.ownerKind &&
      ownerId == other.ownerId &&
      ownerDeviceId == other.ownerDeviceId &&
      ownerSigningKeySha256 == other.ownerSigningKeySha256 &&
      serviceId == other.serviceId &&
      serviceHash == other.serviceHash &&
      catalog.collectorId == other.catalog.collectorId &&
      catalog.manifestSha256 == other.catalog.manifestSha256 &&
      ownerEpoch == other.ownerEpoch &&
      checkpoint == other.checkpoint &&
      policyRevision == other.policyRevision &&
      revocationEpoch == other.revocationEpoch &&
      issuedAtUtc == other.issuedAtUtc &&
      expiresAtUtc == other.expiresAtUtc &&
      effectiveAuthoritySeconds == other.effectiveAuthoritySeconds;
}

/// Start at the explicit preparation action. A later Native details reply may
/// shorten this original review window, never restart or extend it. This is an
/// advisory UI clock; the original Native monotonic deadline remains authority.
final class AgentCollectorServiceReviewClock {
  AgentCollectorServiceReviewClock({this.elapsed});
  final Duration Function()? elapsed;
  final Stopwatch _watch = Stopwatch()..start();
  int _deadlineMs = 60000, _lastElapsedMs = 0;
  bool _invalid = false, hasOriginalView = false;
  int _now() {
    final current = (elapsed?.call() ?? _watch.elapsed).inMilliseconds;
    if (current < 0 || current < _lastElapsedMs) {
      _invalid = true;
      return _lastElapsedMs;
    }
    _lastElapsedMs = current;
    return current;
  }

  void acceptNativeRemaining(int remainingMs) {
    if (remainingMs < 1 || remainingMs > 60000) {
      _invalid = true;
      return;
    }
    final proposed = _now() + remainingMs;
    if (proposed < _deadlineMs) _deadlineMs = proposed;
    hasOriginalView = true;
  }

  int get remainingMs {
    final current = _now();
    return _invalid ? 0 : (_deadlineMs - current).clamp(0, 60000);
  }

  int get remainingSeconds => (remainingMs + 999) ~/ 1000;
}

Never _bad() => throw const FormatException('Finite collector service metadata invalid');
Map<String, dynamic> _keys(Object? value, Set<String> keys) {
  if (value is! Map<String, dynamic> || value.length != keys.length || !value.keys.every(keys.contains)) _bad();
  return value;
}

String _text(Object? value, int cap) =>
    value is String && value.isNotEmpty && utf8.encode(value).length <= cap ? value : _bad();
String _id(Object? value) => checkedAssociationId(_text(value, 36));
String _hash(Object? value) => associationHash(_text(value, 64));
String _decimal(Object? value, {bool positive = false, bool signed = false}) =>
    associationNumber(_text(value, 20), positive: positive, signed: signed);
int _int(Object? value, int minimum, int maximum) =>
    value is int && value >= minimum && value <= maximum ? value : _bad();
void _fixed(Object? value, Object expected) {
  if (value.runtimeType != expected.runtimeType || value != expected) _bad();
}

DateTime _utc(Object? value) {
  final raw = _text(value, 32);
  if (RegExp(r'^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$').stringMatch(raw) != raw) _bad();
  final date = DateTime.tryParse(raw);
  if (date == null || !date.isUtc || date.toIso8601String().replaceFirst('.000Z', 'Z') != raw) _bad();
  return date;
}

AgentCollectorServiceView parseAgentCollectorServiceView(
  String raw,
  RuntimeNoticeScope scope,
  AgentAssociationRecord record,
) {
  if (raw.isEmpty ||
      utf8.encode(raw).length > 16384 ||
      record.phase != AgentAssociationPhase.associated ||
      record.purpose != AgentAssociationPurpose.metrics ||
      record.hostRevision == null ||
      record.certificateDigest == null ||
      record.nodeKeyEpoch == null ||
      record.nodeKeyFingerprint == null) {
    _bad();
  }
  final decoded = jsonDecode(raw);
  if (decoded is! Map<String, dynamic>) _bad();
  final lifetime = decoded['kind'] == 'finiteCollectorServiceLifetimeCredentials';
  final withCredentials = lifetime || decoded['kind'] == 'finiteCollectorServiceCredentials';
  final m = _keys(decoded, {
    'version',
    'profileId',
    'notificationScopeId',
    'kind',
    'phase',
    'associationId',
    'hostId',
    'hostRevision',
    'owner',
    'node',
    'scope',
    'serviceId',
    'serviceHash',
    'collectorId',
    'manifestSha256',
    'manifest',
    'ownerEpoch',
    'checkpoint',
    'actorRevision',
    'policyRevision',
    'revocationEpoch',
    'profile',
    'bufferMode',
    'remainingMs',
    'issuedAtUtc',
    'expiresAtUtc',
    'effectiveAuthoritySeconds',
    'maxAuthoritySeconds',
    'maxStarts',
    'maxConcurrent',
    'maxBufferBytes',
    'maxResidenceSeconds',
    'maxMemoryBytes',
    'continuesWhenClientLocks',
    'automaticRenewal',
    'osBootPolicy',
    'osRebootRequiresApproval',
    'bootStamp',
    'configurationAck',
    'runtimeState',
    'deliveryGuaranteed',
    'lossUnknown',
    'cleanupState',
    if (withCredentials) 'credentialSelections',
    if (lifetime) ...{
      'credentialPurpose',
      'reviewId',
      'requiresExplicitLifetimeConsent',
      'encryptedCredentialRetentionUntilUtc',
      'automaticCredentialRefresh',
    },
  });
  _fixed(m['version'], 1);
  if (m['profileId'] != scope.profileId ||
      m['notificationScopeId'] != scope.nonce ||
      _id(m['associationId']) != record.id ||
      _id(m['hostId']) != record.hostId ||
      _decimal(m['hostRevision'], positive: true, signed: true) != record.hostRevision) {
    _bad();
  }
  for (final e in <String, Object>{
    'kind': lifetime ? 'finiteCollectorServiceLifetimeCredentials' :
        withCredentials ? 'finiteCollectorServiceCredentials' : 'finiteCollectorService',
    'scope': 'metrics.collect',
    'profile': agentMetricsDiskProfile,
    'bufferMode': 'persistentNativeWal',
    'maxAuthoritySeconds': 86400,
    'maxStarts': 32,
    'maxConcurrent': 1,
    'maxBufferBytes': 536870912,
    'maxResidenceSeconds': 7200,
    'maxMemoryBytes': 536870912,
    'continuesWhenClientLocks': true,
    'automaticRenewal': false,
    'osBootPolicy': 'sameBootOnly',
    'osRebootRequiresApproval': true,
    'runtimeState': 'notObserved',
    'deliveryGuaranteed': false,
    'lossUnknown': true,
  }.entries) {
    _fixed(m[e.key], e.value);
  }
  final owner = _keys(m['owner'], {'kind', 'id', 'deviceId', 'signingKeySha256'}), ownerId = _id(owner['id']);
  if (!const {'localProfile', 'personalAccount'}.contains(owner['kind']) ||
      owner['kind'] == 'localProfile' && ownerId != scope.profileId) {
    _bad();
  }
  final node = AgentRotationNode.parse(m['node']);
  if (node.agentId != record.agentId ||
      node.keyEpoch != record.nodeKeyEpoch ||
      node.fingerprint != record.nodeKeyFingerprint) {
    _bad();
  }
  final phase = AgentCollectorServicePhase.values.where((p) => p.name == m['phase']).firstOrNull;
  if (phase == null) _bad();
  final cleanup = m['cleanupState'];
  if (!const {'notStarted', 'pending', 'closedObserved', 'unknown'}.contains(cleanup)) _bad();
  final catalog = parseAgentMetricsCollectorCatalog({
    'collectorId': m['collectorId'],
    'manifestSha256': m['manifestSha256'],
    'manifest': m['manifest'],
  }, record.hostId);
  if (!catalog.manifest.persistentDisk) _bad();
  final issued = _utc(m['issuedAtUtc']),
      expires = _utc(m['expiresAtUtc']),
      duration = _int(m['effectiveAuthoritySeconds'], 1, 86400);
  if (expires.difference(issued).inSeconds != duration ||
      record.certificateExpiresUtc != null && expires.isAfter(record.certificateExpiresUtc!)) {
    _bad();
  }
  AgentCollectorServiceLifetimeCredentials? lifetimeCredentials;
  if (lifetime) {
    _fixed(m['credentialPurpose'], 'metrics-collector-service-credential-authorization-v1');
    _fixed(m['requiresExplicitLifetimeConsent'], true);
    _fixed(m['automaticCredentialRefresh'], false);
    final reviewId = _id(m['reviewId']), retention = _utc(m['encryptedCredentialRetentionUntilUtc']);
    if (reviewId[14] != '4' || retention != expires) _bad();
    lifetimeCredentials = AgentCollectorServiceLifetimeCredentials(
      reviewId, m['credentialPurpose'] as String, retention,
    );
  }
  final serviceId = _id(m['serviceId']);
  // CollectorServiceDelegation requires a newly issued UUIDv4 service ID.
  if (serviceId[14] != '4') _bad();
  final actor = _decimal(m['actorRevision']);
  AgentCollectorServiceBootStamp? boot;
  AgentCollectorServiceConfigurationAck? ack;
  if (phase == AgentCollectorServicePhase.configurationRecorded) {
    final b = _keys(m['bootStamp'], {'bootId', 'createdBoottimeNs', 'deadlineBoottimeNs'}),
        a = _keys(m['configurationAck'], {'actorRevision', 'recordedAtUtc'});
    final created = _decimal(b['createdBoottimeNs'], positive: true),
        deadline = _decimal(b['deadlineBoottimeNs'], positive: true),
        difference = BigInt.parse(deadline) - BigInt.parse(created);
    if (difference <= BigInt.zero ||
        difference > BigInt.from(duration) * BigInt.from(1000000000) ||
        _decimal(a['actorRevision'], positive: true) != actor) {
      _bad();
    }
    boot = AgentCollectorServiceBootStamp(_id(b['bootId']), created, deadline);
    ack = AgentCollectorServiceConfigurationAck(actor, _utc(a['recordedAtUtc']));
    if (ack.recordedAtUtc.isBefore(issued)) _bad();
  } else if (m['bootStamp'] != null || m['configurationAck'] != null) {
    _bad();
  }
  return AgentCollectorServiceView(
    scope: scope,
    record: record,
    phase: phase,
    ownerKind: owner['kind'] as String,
    ownerId: ownerId,
    ownerDeviceId: _id(owner['deviceId']),
    ownerSigningKeySha256: _hash(owner['signingKeySha256']),
    node: node,
    serviceId: serviceId,
    serviceHash: _hash(m['serviceHash']),
    catalog: catalog,
    ownerEpoch: _decimal(m['ownerEpoch'], positive: true),
    checkpoint: _decimal(m['checkpoint']),
    actorRevision: actor,
    policyRevision: _decimal(m['policyRevision'], positive: true),
    revocationEpoch: _decimal(m['revocationEpoch'], positive: true),
    remainingMs: _int(m['remainingMs'], 1, 60000),
    issuedAtUtc: issued,
    expiresAtUtc: expires,
    effectiveAuthoritySeconds: duration,
    cleanupState: cleanup as String,
    lifetimeCredentials: lifetimeCredentials,
    bootStamp: boot,
    configurationAck: ack,
    credentialSelections: withCredentials
        ? parseCollectorCredentialSelections(m['credentialSelections'], catalog)
        : const [],
  );
}
