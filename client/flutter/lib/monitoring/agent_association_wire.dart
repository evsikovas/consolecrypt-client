import 'dart:convert';

import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_exporter_policy.dart';
import 'package:consolecrypt/monitoring/agent_metrics_manifest.dart';

Never _bad() => throw const FormatException('Agent association response invalid');
typedef _Json = Map<String, dynamic>;
_Json _object(Object? v) => v is _Json ? v : _bad();
_Json _keys(Object? v, Set<String> keys) {
  final m = _object(v);
  if (m.length != keys.length || !m.keys.every(keys.contains)) _bad();
  return m;
}

_Json _decode(String raw, int cap) {
  if (utf8.encode(raw).length > cap) _bad();
  return _object(jsonDecode(raw));
}

int _int(Object? v, int min, int max) => v is int && v >= min && v <= max ? v : _bad();
String _text(Object? v, int cap) =>
    v is String &&
        v.isNotEmpty &&
        utf8.encode(v).length <= cap &&
        !v.runes.any((r) => r < 32 || r >= 127 && r <= 159 || r >= 0xd800 && r <= 0xdfff)
    ? v
    : _bad();
String _id(Object? v) => checkedAssociationId(_text(v, 36));
String _sha(Object? v) => associationHash(_text(v, 64));
String _number(Object? v, {bool positive = false, bool signed = false}) =>
    associationNumber(_text(v, 20), positive: positive, signed: signed);
String _choice(Object? v, Set<String> values) => v is String && values.contains(v) ? v : _bad();
const _cleanup = {'notStarted', 'pending', 'closedObserved', 'unknown'};
const _phases = {
  'idle',
  'probing',
  'offerReview',
  'claiming',
  'claimReview',
  'applying',
  'associated',
  'cancelled',
  'expired',
  'unknown',
};
void _frame(_Json m, RuntimeNoticeScope scope) {
  if (m['version'] is! int ||
      m['version'] != 1 ||
      m['profileId'] != scope.profileId ||
      m['notificationScopeId'] != scope.nonce) {
    _bad();
  }
}

DateTime _utc(Object? value) {
  final raw = _text(value, 64),
      m = RegExp(r'^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.\d{1,9})?Z$').firstMatch(raw),
      d = DateTime.tryParse(raw);
  if (m == null ||
      d == null ||
      !d.isUtc ||
      d.year != int.parse(m[1]!) ||
      d.month != int.parse(m[2]!) ||
      d.day != int.parse(m[3]!) ||
      d.hour != int.parse(m[4]!) ||
      d.minute != int.parse(m[5]!) ||
      d.second != int.parse(m[6]!)) {
    _bad();
  }
  return d;
}

_Json _owner(Object? value) {
  final m = _keys(value, {'kind', 'id', 'deviceId', 'signingKeySha256'});
  _choice(m['kind'], {'localProfile', 'personalAccount'});
  _id(m['id']);
  _id(m['deviceId']);
  _sha(m['signingKeySha256']);
  return m;
}

_Json _node(Object? value) {
  final m = _keys(value, {'agentId', 'keyEpoch', 'publicKey', 'keyFingerprint'});
  _id(m['agentId']);
  _number(m['keyEpoch'], positive: true);
  _sha(m['publicKey']);
  _sha(m['keyFingerprint']);
  return m;
}

_Json _context(Object? value) {
  final m = _keys(value, {'kind', 'id', 'deploymentId'});
  _choice(m['kind'], {'localProfile', 'personalAccount'});
  _id(m['id']);
  _id(m['deploymentId']);
  return m;
}

_Json _identity(Object? value) {
  final m = _keys(value, {'identityId', 'certificateSha256'});
  _id(m['identityId']);
  _sha(m['certificateSha256']);
  return m;
}

AgentAssociationStatus parseAgentAssociationStatus(String raw, RuntimeNoticeScope scope) {
  final m = _keys(_decode(raw, 65536), {
    'version',
    'profileId',
    'notificationScopeId',
    'hosts',
    'clientIdentity',
    'associations',
  });
  _frame(m, scope);
  if (m['hosts'] is! List ||
      (m['hosts'] as List).length > 64 ||
      m['associations'] is! List ||
      (m['associations'] as List).length > 32) {
    _bad();
  }
  final hosts = [for (final rawHost in m['hosts'] as List) _host(rawHost)];
  final records = [for (final rawRecord in m['associations'] as List) _record(rawRecord)];
  final identity = _keys(m['clientIdentity'], {'state', 'identityId', 'certificateSha256'}),
      state = _choice(identity['state'], {'missing', 'pending', 'storedMetadata', 'unavailable'});
  final present = identity['identityId'] != null;
  if (present != (identity['certificateSha256'] != null) ||
      state == 'missing' && present ||
      (state == 'storedMetadata' || state == 'pending') && !present) {
    _bad();
  }
  return AgentAssociationStatus(
    scope: scope,
    clientIdentityState: AgentClientIdentityState.values.byName(state),
    hosts: hosts,
    records: records,
    clientIdentityId: present ? _id(identity['identityId']) : null,
    clientCertificateSha256: present ? _sha(identity['certificateSha256']) : null,
  );
}

AgentAssociationHost _host(Object? v) {
  final m = _keys(v, {'hostId', 'hostRevision', 'name'});
  return AgentAssociationHost(
    id: _id(m['hostId']),
    revision: _number(m['hostRevision'], positive: true, signed: true),
    name: _text(m['name'], 256),
  );
}

AgentAssociationPurpose? _purpose(_Json m) {
  if (!m.containsKey('purpose') && !m.containsKey('scope') && !m.containsKey('scopes')) return null;
  final purpose = _choice(m['purpose'], {'tunnel', 'metrics', 'exporters'});
  final scope = switch (purpose) {
    'metrics' => 'metrics.collect',
    'exporters' => 'exporters.manage',
    _ => 'tunnel.open',
  };
  if (m['scope'] != scope ||
      m['scopes'] is! List ||
      (m['scopes'] as List).length != 1 ||
      (m['scopes'] as List).single != scope) {
    _bad();
  }
  return AgentAssociationPurpose.values.byName(purpose);
}

AgentAssociationRecord _record(Object? v) {
  final object = _object(v);
  final hasPurpose = object.keys.any(const {'purpose', 'scope', 'scopes'}.contains);
  final hasRevoke = object.containsKey('revocationState') || object.containsKey('remoteRevocationObserved');
  final hasRotation = object.containsKey('keyEpoch') || object.containsKey('rotationState');
  final m = _keys(v, {
        'associationId',
        'hostId',
        'hostRevision',
        'agentId',
        'nodeKeyFingerprint',
        'certificateDigest',
        'certificateExpiresUtc',
        'state',
        if (hasRevoke) ...{'revocationState', 'remoteRevocationObserved'},
        if (hasRotation) ...{'keyEpoch', 'rotationState'},
        if (hasPurpose) ...{'purpose', 'scope', 'scopes'},
      }),
      state = _choice(m['state'], {'pending', 'unknown', 'associated', 'blocked', if (hasRevoke) 'revoked'});
  final purpose = _purpose(m);
  String? revocation;
  bool? observed;
  if (hasRevoke) {
    revocation = _choice(m['revocationState'], {'notRequested', 'unknown', 'observedRevoked'});
    if (m['remoteRevocationObserved'] is! bool) _bad();
    observed = m['remoteRevocationObserved'] as bool;
    if (observed != (revocation == 'observedRevoked') || observed != (state == 'revoked')) {
      _bad();
    }
  }
  final present = m['certificateDigest'] != null;
  final keyEpoch = hasRotation ? _number(m['keyEpoch'], positive: true) : null;
  final rotation = hasRotation ? _choice(m['rotationState'], {'notRequested', 'unknown', 'selectedObserved'}) : null;
  if (hasRotation &&
      (!hasRevoke ||
          !const {'1', '2'}.contains(keyEpoch) ||
          (rotation == 'selectedObserved') != (keyEpoch == '2') ||
          rotation == 'unknown' && state != 'blocked')) {
    _bad();
  }
  if ((purpose == AgentAssociationPurpose.metrics || purpose == AgentAssociationPurpose.exporters) &&
      (keyEpoch != '1' || rotation != 'notRequested')) {
    _bad();
  }
  if (present != (m['certificateExpiresUtc'] != null) || (state == 'associated' || state == 'revoked') && !present) {
    _bad();
  }
  return AgentAssociationRecord(
    associationId: _id(m['associationId']),
    hostId: _id(m['hostId']),
    agentId: _id(m['agentId']),
    phase: AgentAssociationPhase.values.byName(state),
    hostRevision: _number(m['hostRevision'], positive: true, signed: true),
    nodeKeyFingerprint: _sha(m['nodeKeyFingerprint']),
    certificateDigest: present ? _sha(m['certificateDigest']) : null,
    certificateExpiresUtc: present ? _utc(m['certificateExpiresUtc']) : null,
    revocationState: revocation,
    remoteRevocationObserved: observed,
    nodeKeyEpoch: keyEpoch,
    rotationState: rotation,
    purpose: purpose,
  );
}

_Json _operation(
  String raw,
  RuntimeNoticeScope scope,
  AgentAssociationHost host,
  AgentAssociationPurpose expectedPurpose,
) {
  final object = _decode(raw, 16384);
  final current = object.keys.any(
    const {'purpose', 'scope', 'scopes', 'scopeSource', 'advertisedScopes', 'collector'}.contains,
  );
  final m = _keys(object, {
    'version',
    'profileId',
    'notificationScopeId',
    'phase',
    'hostId',
    'hostRevision',
    'remainingMs',
    'node',
    'nodeContext',
    'owner',
    'ownerEpoch',
    'checkpoint',
    'actorRevision',
    'clientIdentity',
    'associationId',
    'claimDigest',
    'certificateDigest',
    'maxCertificateLifetimeSeconds',
    'certificateExpiresUtc',
    'actorAdmission',
    'cleanupState',
    if (current) ...{'purpose', 'scope', 'scopes', 'scopeSource', 'advertisedScopes', 'collector'},
    if (expectedPurpose == AgentAssociationPurpose.exporters) 'exporter',
  });
  final purpose = _purpose(m);
  if (current && purpose != expectedPurpose || !current && expectedPurpose != AgentAssociationPurpose.tunnel) _bad();
  if (current) {
    if (purpose == AgentAssociationPurpose.tunnel) {
      if (m['scopeSource'] != 'explicitFactoryRequest' || m['advertisedScopes'] != null || m['collector'] != null) {
        _bad();
      }
    } else if (m['scopeSource'] == 'actualAdvertisedIntersection') {
      if (m['advertisedScopes'] is! List ||
          (m['advertisedScopes'] as List).length != 1 ||
          (m['advertisedScopes'] as List).single !=
              (purpose == AgentAssociationPurpose.exporters ? 'exporters.manage' : 'metrics.collect')) {
        _bad();
      }
      if (purpose == AgentAssociationPurpose.exporters) {
        if (m['collector'] != null) _bad();
        parseAgentExporterEnrollmentCatalog(m['exporter'], host.id);
      } else {
        parseAgentMetricsCollectorCatalog(m['collector'], host.id);
      }
    } else if (m['scopeSource'] == 'explicitFactoryRequest') {
      if (m['advertisedScopes'] != null ||
          m['collector'] != null ||
          expectedPurpose == AgentAssociationPurpose.exporters && m['exporter'] != null ||
          const {'offerReview', 'claimReview', 'associated'}.contains(m['phase'])) {
        _bad();
      }
    } else {
      _bad();
    }
  }
  _frame(m, scope);
  final phase = _choice(m['phase'], _phases);
  if (_id(m['hostId']) != host.id ||
      _number(m['hostRevision'], positive: true, signed: true) != host.revision ||
      m['actorAdmission'] != false) {
    _bad();
  }
  _int(m['remainingMs'], 0, 60000);
  final max = _int(m['maxCertificateLifetimeSeconds'], 0, 2592000);
  if (phase != 'claimReview' && max != 0) _bad();
  _choice(m['cleanupState'], _cleanup);
  for (final key in ['node', 'nodeContext', 'owner', 'clientIdentity']) {
    if (m[key] != null) {
      switch (key) {
        case 'node':
          _node(m[key]);
        case 'nodeContext':
          _context(m[key]);
        case 'owner':
          _owner(m[key]);
        case 'clientIdentity':
          _identity(m[key]);
      }
    }
  }
  for (final key in ['ownerEpoch', 'checkpoint', 'actorRevision']) {
    if (m[key] != null) _number(m[key], positive: key == 'ownerEpoch');
  }
  if (m['associationId'] != null) _id(m['associationId']);
  for (final key in ['claimDigest', 'certificateDigest']) {
    if (m[key] != null) _sha(m[key]);
  }
  if (m['certificateExpiresUtc'] != null) _utc(m['certificateExpiresUtc']);
  return m;
}

void _sameOwnerContext(_Json owner, _Json context, RuntimeNoticeScope scope) {
  if (owner['kind'] != context['kind'] ||
      owner['kind'] == 'localProfile' && owner['id'] != scope.profileId ||
      owner['kind'] == 'personalAccount' && owner['id'] != context['id']) {
    _bad();
  }
}

AgentAssociationReview parseAgentAssociationReview(
  String raw,
  RuntimeNoticeScope scope,
  AgentAssociationHost host,
  AgentAssociationReviewStage stage, {
  AgentAssociationPurpose expectedPurpose = AgentAssociationPurpose.tunnel,
}) {
  final m = _operation(raw, scope, host, expectedPurpose);
  if (m['phase'] != (stage == AgentAssociationReviewStage.offer ? 'offerReview' : 'claimReview') ||
      _int(m['remainingMs'], 1, 60000) < 1 ||
      m['certificateDigest'] != null ||
      m['certificateExpiresUtc'] != null) {
    _bad();
  }
  final node = _node(m['node']),
      context = _context(m['nodeContext']),
      owner = _owner(m['owner']),
      identity = _identity(m['clientIdentity']);
  if (owner['kind'] == 'localProfile' && owner['id'] != scope.profileId) _bad();
  return AgentAssociationReview(
    scope: scope,
    stage: stage,
    purpose: expectedPurpose,
    metricsCollector: expectedPurpose == AgentAssociationPurpose.metrics
        ? parseAgentMetricsCollectorCatalog(m['collector'], host.id)
        : null,
    exporterCatalog: expectedPurpose == AgentAssociationPurpose.exporters
        ? parseAgentExporterEnrollmentCatalog(m['exporter'], host.id)
        : null,
    hostId: host.id,
    hostRevision: host.revision,
    agentId: _id(node['agentId']),
    nodeFingerprint: _sha(node['keyFingerprint']),
    nodePublicKey: _sha(node['publicKey']),
    nodeKeyEpoch: _number(node['keyEpoch'], positive: true),
    ownerKind: owner['kind'] as String,
    ownerId: _id(owner['id']),
    ownerDeviceId: _id(owner['deviceId']),
    ownerSigningKeySha256: _sha(owner['signingKeySha256']),
    nodeContextKind: context['kind'] as String,
    nodeContextId: _id(context['id']),
    nodeDeploymentId: _id(context['deploymentId']),
    ownerEpoch: _number(m['ownerEpoch'], positive: true),
    checkpoint: _number(m['checkpoint']),
    actorRevision: _number(m['actorRevision']),
    clientIdentityId: _id(identity['identityId']),
    certificateSha256: _sha(identity['certificateSha256']),
    remainingMs: m['remainingMs'] as int,
    associationId: m['associationId'] == null ? null : _id(m['associationId']),
    claimDigest: m['claimDigest'] == null ? null : _sha(m['claimDigest']),
    maxCertificateLifetimeSeconds: _int(
      m['maxCertificateLifetimeSeconds'],
      stage == AgentAssociationReviewStage.certificate ? 1 : 0,
      2592000,
    ),
  );
}

AgentAssociationResult parseAgentAssociationResult(
  String raw,
  RuntimeNoticeScope scope,
  AgentAssociationHost host, {
  AgentAssociationPurpose expectedPurpose = AgentAssociationPurpose.tunnel,
}) {
  final m = _operation(raw, scope, host, expectedPurpose);
  if (m['phase'] != 'associated') _bad();
  _node(m['node']);
  _sameOwnerContext(_owner(m['owner']), _context(m['nodeContext']), scope);
  _identity(m['clientIdentity']);
  _sha(m['claimDigest']);
  return AgentAssociationResult(
    scope: scope,
    associationId: _id(m['associationId']),
    purpose: expectedPurpose,
    metricsCollector: expectedPurpose == AgentAssociationPurpose.metrics
        ? parseAgentMetricsCollectorCatalog(m['collector'], host.id)
        : null,
    exporterCatalog: expectedPurpose == AgentAssociationPurpose.exporters
        ? parseAgentExporterEnrollmentCatalog(m['exporter'], host.id)
        : null,
    agentId: _id(_node(m['node'])['agentId']),
    certificateDigest: _sha(m['certificateDigest']),
    checkpoint: _number(m['checkpoint']),
    actorAdmission: false,
    certificateExpiresUtc: _utc(m['certificateExpiresUtc']),
  );
}

const _identityKeys = {
  'version',
  'profileId',
  'notificationScopeId',
  'phase',
  'owner',
  'algorithm',
  'validityDays',
  'remainingMs',
  'identityId',
  'certificateSha256',
  'cleanupState',
};
_Json _creation(String raw, RuntimeNoticeScope scope) {
  final m = _keys(_decode(raw, 8192), _identityKeys);
  _frame(m, scope);
  _choice(m['phase'], {'createReview', 'creating', 'stored', 'cancelled', 'expired', 'unknown'});
  _owner(m['owner']);
  if (m['algorithm'] != 'ecdsaP256Sha256' || m['validityDays'] is! int || m['validityDays'] != 30) {
    _bad();
  }
  _int(m['remainingMs'], 0, 60000);
  _choice(m['cleanupState'], _cleanup);
  return m;
}

AgentClientIdentityReview parseAgentClientIdentityReview(String raw, RuntimeNoticeScope scope) {
  final m = _creation(raw, scope);
  if (m['phase'] != 'createReview' || m['identityId'] != null || m['certificateSha256'] != null) {
    _bad();
  }
  final owner = _owner(m['owner']);
  if (owner['kind'] == 'localProfile' && owner['id'] != scope.profileId) _bad();
  return AgentClientIdentityReview(
    scope: scope,
    ownerKind: owner['kind'] as String,
    ownerId: _id(owner['id']),
    deviceId: _id(owner['deviceId']),
    signingKeySha256: _sha(owner['signingKeySha256']),
    remainingMs: _int(m['remainingMs'], 1, 60000),
  );
}

AgentClientIdentityResult parseAgentClientIdentityResult(String raw, RuntimeNoticeScope scope) {
  final m = _creation(raw, scope);
  if (m['phase'] != 'stored') _bad();
  return AgentClientIdentityResult(
    scope: scope,
    identityId: _id(m['identityId']),
    certificateSha256: _sha(m['certificateSha256']),
  );
}

AgentAssociationCleanup parseAgentAssociationCleanup(String raw) {
  final m = _keys(_decode(raw, 1024), {'version', 'phase', 'cleanupState'});
  if (m['version'] is! int || m['version'] != 1) _bad();
  return AgentAssociationCleanup(
    _choice(m['phase'], {..._phases, 'createReview', 'creating', 'stored'}),
    _choice(m['cleanupState'], _cleanup),
  );
}

const _revocationPhases = {'revocationReview', 'revoking', 'revoked', 'unknown', 'cancelled', 'expired'};
AgentRevocationDetails parseAgentRevocationDetails(
  String raw,
  RuntimeNoticeScope scope,
  AgentAssociationRecord original,
) {
  final object = _decode(raw, 16384);
  final domain = object.keys.any(const {'purpose', 'scope', 'scopes'}.contains);
  final m = _keys(object, {
    'version',
    'profileId',
    'notificationScopeId',
    'remainingMs',
    'phase',
    'associationId',
    'hostId',
    'hostRevision',
    'node',
    'owner',
    'nodeContext',
    'certificateDigest',
    'actorRevision',
    'checkpoint',
    'localBlocked',
    'remoteRevocationObserved',
    'actorAdmission',
    'cleanupState',
    if (domain) ...{'purpose', 'scope', 'scopes'},
  });
  if (domain && _purpose(m) != (original.purpose ?? AgentAssociationPurpose.tunnel) ||
      !domain && original.purpose != null && original.purpose != AgentAssociationPurpose.tunnel) {
    _bad();
  }
  _frame(m, scope);
  final phase = _choice(m['phase'], _revocationPhases),
      node = _node(m['node']),
      owner = _owner(m['owner']),
      context = _context(m['nodeContext']);
  final blocked = m['localBlocked'], observed = m['remoteRevocationObserved'];
  if (blocked != null && blocked is! bool ||
      observed is! bool ||
      m['actorAdmission'] != false ||
      _id(m['associationId']) != original.id ||
      _id(m['hostId']) != original.hostId ||
      _number(m['hostRevision'], positive: true, signed: true) != original.hostRevision ||
      node['agentId'] != original.agentId ||
      node['keyFingerprint'] != original.nodeKeyFingerprint ||
      original.nodeKeyEpoch != null && node['keyEpoch'] != original.nodeKeyEpoch ||
      _sha(m['certificateDigest']) != original.certificateDigest ||
      owner['kind'] == 'localProfile' && owner['id'] != scope.profileId ||
      (observed == true) != (phase == 'revoked') ||
      observed == true && blocked != true ||
      phase == 'revocationReview' && blocked != false) {
    _bad();
  }
  return AgentRevocationDetails(
    scope: scope,
    phase: phase,
    record: original,
    remainingMs: _int(m['remainingMs'], 1, 60000),
    nodePublicKey: node['publicKey'] as String,
    nodeKeyEpoch: node['keyEpoch'] as String,
    ownerKind: owner['kind'] as String,
    ownerId: owner['id'] as String,
    ownerDeviceId: owner['deviceId'] as String,
    ownerSigningKeySha256: owner['signingKeySha256'] as String,
    nodeContextKind: context['kind'] as String,
    nodeContextId: context['id'] as String,
    nodeDeploymentId: context['deploymentId'] as String,
    actorRevision: _number(m['actorRevision']),
    checkpoint: _number(m['checkpoint']),
    localBlocked: blocked as bool?,
    remoteRevocationObserved: observed,
    cleanupState: _choice(m['cleanupState'], _cleanup),
  );
}

AgentRevocationCleanup parseAgentRevocationCleanup(String raw) {
  final m = _keys(_decode(raw, 4096), {'version', 'phase', 'remoteRevocationObserved', 'cleanupState'});
  final phase = _choice(m['phase'], _revocationPhases), observed = m['remoteRevocationObserved'];
  if (m['version'] is! int || m['version'] != 1 || observed is! bool || (observed == true) != (phase == 'revoked')) {
    _bad();
  }
  return AgentRevocationCleanup(phase, _choice(m['cleanupState'], _cleanup), observed);
}
