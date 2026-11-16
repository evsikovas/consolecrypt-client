import 'dart:convert';

import 'package:consolecrypt/monitoring/monitoring_models.dart';

final _uuid = RegExp(r'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$');
final _hash = RegExp(r'^[0-9a-f]{64}$');
Never readerInvalid() => throw const FormatException('reader_response_invalid');
bool readerId(Object? v) => v is String && _uuid.hasMatch(v) && v != '00000000-0000-0000-0000-000000000000';
Json readerMap(Object? v) => v is Map<String, dynamic> ? v : readerInvalid();
String readerText(Object? v, int cap) =>
    v is String && v.isNotEmpty && utf8.encode(v).length <= cap && !v.runes.any((r) => r < 32 || r >= 127 && r <= 159)
    ? v
    : readerInvalid();
int readerInteger(Object? v, {int min = 0, int max = 9007199254740991}) =>
    v is int && v >= min && v <= max ? v : readerInvalid();
String readerHash(Object? v) => v is String && _hash.hasMatch(v) ? v : readerInvalid();
void readerKeys(Json m, Set<String> keys) {
  if (m.length != keys.length || !m.keys.every(keys.contains)) readerInvalid();
}

/// Preserve exact unsigned integer lexemes used only by public review metadata.
/// JSON string contents/escapes are never inspected or changed. No authority or
/// numeric comparison is derived from these decimal display strings.
String readerExactIntegers(String raw) {
  final out = StringBuffer();
  var i = 0, inString = false, escaped = false;
  while (i < raw.length) {
    final c = raw[i];
    if (inString) {
      out.write(c);
      i++;
      if (escaped) {
        escaped = false;
      } else if (c == '\\') {
        escaped = true;
      } else if (c == '"') {
        inString = false;
      }
      continue;
    }
    if (c == '"') {
      inString = true;
      out.write(c);
      i++;
      continue;
    }
    if (c == '-' || c.codeUnitAt(0) >= 48 && c.codeUnitAt(0) <= 57) {
      final start = i;
      while (i < raw.length && '0123456789eE+-.'.contains(raw[i])) {
        i++;
      }
      final token = raw.substring(start, i);
      if (RegExp(r'^-?(0|[1-9][0-9]*)$').hasMatch(token) && BigInt.parse(token).abs() > BigInt.from(9007199254740991)) {
        out.write(jsonEncode(token));
      } else {
        out.write(token);
      }
    } else {
      out.write(c);
      i++;
    }
  }
  return out.toString();
}

Json readerDecode(String raw, {int maxBytes = 32768}) {
  if (utf8.encode(raw).length > maxBytes) readerInvalid();
  return readerMap(jsonDecode(readerExactIntegers(raw)));
}

enum AgentReaderAction { nodeOffer, nodeCert, issuerInit, association, read, revoke, reconcile }

final class AgentReaderSelection {
  AgentReaderSelection({
    required this.profileId,
    required this.notificationScopeId,
    required MonitoringSource source,
    required MonitoringBinding binding,
    required this.knownHostId,
  }) : sourceId = source.id,
       sourceRevision = source.revision,
       bindingId = binding.id,
       bindingRevision = binding.revision,
       hostId = binding.hostId {
    if (![profileId, sourceId, bindingId, hostId, knownHostId].every(readerId) ||
        sourceId != binding.sourceId ||
        sourceRevision != binding.sourceRevision) {
      readerInvalid();
    }
    readerInteger(sourceRevision, min: 1);
    readerInteger(bindingRevision, min: 1);
    readerText(notificationScopeId, 256);
  }
  final String profileId, notificationScopeId, sourceId, bindingId, hostId, knownHostId;
  final int sourceRevision, bindingRevision;
  Json get json => {
    'profileId': profileId,
    'hostId': hostId,
    'sourceId': sourceId,
    'sourceRevision': sourceRevision,
    'bindingId': bindingId,
    'bindingRevision': bindingRevision,
    'knownHostId': knownHostId,
  };
  void verifyNative(Json native) {
    readerKeys(native, {
      'nativeProfileId',
      'nativeHostId',
      'sourceId',
      'sourceRevision',
      'bindingId',
      'bindingRevision',
      'nativeBindingDigest',
      'deviceId',
      'notificationScopeId',
    });
    if (native['notificationScopeId'] != notificationScopeId ||
        native['nativeProfileId'] != profileId ||
        native['nativeHostId'] != hostId ||
        native['sourceId'] != sourceId ||
        native['sourceRevision'] != sourceRevision ||
        native['bindingId'] != bindingId ||
        native['bindingRevision'] != bindingRevision ||
        !readerId(native['deviceId'])) {
      readerInvalid();
    }
    readerHash(native['nativeBindingDigest']);
  }
}

final class AgentReaderLocator {
  AgentReaderLocator(this.peerDigest, this.associationDigest) {
    readerHash(peerDigest);
    readerHash(associationDigest);
  }
  final String peerDigest, associationDigest;
  Json get json => {'peerDigest': peerDigest, 'associationDigest': associationDigest};
}

final class AgentReaderRequest {
  AgentReaderRequest._(this.kind, this.action);
  factory AgentReaderRequest.issuer() => AgentReaderRequest._(AgentReaderAction.issuerInit, {'kind': 'issuerInit'});
  factory AgentReaderRequest.file(AgentReaderAction kind, Map<String, String> files, {int? expiresAt}) {
    final expected = switch (kind) {
      AgentReaderAction.nodeOffer => {'ownerPinFile', 'nodeHelloFile'},
      AgentReaderAction.nodeCert => {'ownerPinFile', 'signedOfferFile', 'signedClaimFile'},
      AgentReaderAction.association => {'selectionFile'},
      _ => readerInvalid(),
    };
    if (files.keys.toSet().difference(expected).isNotEmpty || files.length != expected.length) readerInvalid();
    for (final file in files.values) {
      readerText(file, 4096);
    }
    final action = <String, dynamic>{'kind': kind.name, ...files};
    if (kind == AgentReaderAction.nodeOffer) {
      action['offerExpiresAt'] = readerInteger(expiresAt, min: 1, max: 253402300799);
    }
    if (kind == AgentReaderAction.nodeCert) {
      action['certificateExpiresAt'] = readerInteger(expiresAt, min: 1, max: 253402300799);
    }
    return AgentReaderRequest._(kind, Map.unmodifiable(action));
  }
  factory AgentReaderRequest.association(
    AgentReaderAction kind,
    AgentReaderLocator locator, {
    bool originalDevice = false,
  }) {
    if (!{AgentReaderAction.read, AgentReaderAction.revoke, AgentReaderAction.reconcile}.contains(kind)) {
      readerInvalid();
    }
    return AgentReaderRequest._(
      kind,
      Map.unmodifiable({
        'kind': kind.name,
        'association': locator.json,
        if (kind == AgentReaderAction.revoke) 'subject': originalDevice ? 'originalDevice' : 'association',
      }),
    );
  }
  final AgentReaderAction kind;
  final Json action;
  String encode(AgentReaderSelection selection) =>
      jsonEncode({'notificationScopeId': selection.notificationScopeId, 'selection': selection.json, 'action': action});
}

final class AgentReaderFact {
  const AgentReaderFact(this.label, this.value);
  final String label, value;
}

/// Public typed metadata only. Sensitive/raw payloads never enter this view.
List<AgentReaderFact> readerPublicFacts(Object? value) {
  final result = <AgentReaderFact>[];
  void add(Object? v, String path, int depth) {
    if (depth > 12 || result.length >= 160) readerInvalid();
    if (v is Map<String, dynamic>) {
      if (v.length > 64) readerInvalid();
      for (final entry in v.entries) {
        final key = readerText(entry.key, 128);
        if (key == 'tokenHash' || key.endsWith('_sha256')) {
          readerHash(entry.value);
        } else if (key == 'endpoint' && entry.value is Map<String, dynamic>) {
          /* Closed authenticated metadata object, never a URL. */
        } else if (RegExp(
          r'(secret|private|password|seed|token|signature|command|endpoint|url)',
          caseSensitive: false,
        ).hasMatch(key)) {
          readerInvalid();
        }
        add(entry.value, path.isEmpty ? key : '$path · $key', depth + 1);
      }
    } else if (v is List) {
      if (v.length > 32) readerInvalid();
      for (var i = 0; i < v.length; i++) {
        add(v[i], '$path [${i + 1}]', depth + 1);
      }
    } else if (v == null || v is bool || v is int || v is String || v is double && v.isFinite) {
      final leaf = path.split(' · ').last;
      final text =
          v is int &&
              v >= 0 &&
              v <= 253402300799 &&
              {'expiresAt', 'issuedAt', 'notBefore', 'signingNotBefore', 'signingExpiresAt'}.contains(leaf)
          ? DateTime.fromMillisecondsSinceEpoch(v * 1000, isUtc: true).toIso8601String()
          : v == null
          ? '?'
          : v.toString();
      if (utf8.encode(text).length > 4096) readerInvalid();
      result.add(AgentReaderFact(path, text));
    } else {
      readerInvalid();
    }
  }

  add(value, '', 0);
  return List.unmodifiable(result);
}

void readerDisplayU64(Object? value, {bool positive = false}) {
  final text = value is int
      ? value.toString()
      : value is String
      ? value
      : '';
  if (!RegExp(r'^(0|[1-9][0-9]{0,19})$').hasMatch(text)) readerInvalid();
  final n = BigInt.parse(text);
  if (n > BigInt.parse('18446744073709551615') || positive && n == BigInt.zero) readerInvalid();
}

void readerNodeReview(Json review, AgentReaderAction action) {
  readerKeys(review, {'ownerPin', 'body', 'signingNotBefore', 'signingExpiresAt'});
  final pin = readerMap(review['ownerPin']);
  readerKeys(pin, {'context', 'key', 'epoch', 'checkpoint', 'expiresAt'});
  readerHash(pin['key']);
  readerDisplayU64(pin['epoch'], positive: true);
  readerDisplayU64(pin['checkpoint']);
  readerInteger(pin['expiresAt'], min: 1, max: 253402300799);
  final context = readerMap(pin['context']);
  readerKeys(context, {'tenant', 'deploymentId'});
  if (!readerId(context['deploymentId'])) readerInvalid();
  final tenant = readerMap(context['tenant']);
  readerKeys(tenant, {'kind', 'id'});
  if (!{'localProfile', 'personalAccount'}.contains(tenant['kind']) || !readerId(tenant['id'])) readerInvalid();
  final body = readerMap(review['body']);
  readerKeys(body, {'kind', 'value'});
  if (body['kind'] != (action == AgentReaderAction.nodeOffer ? 'offer' : 'certificate')) readerInvalid();
  final value = readerMap(body['value']), offer = action == AgentReaderAction.nodeOffer;
  readerKeys(
    value,
    offer
        ? {'header', 'invitationId', 'agentId', 'agentKey', 'tokenHash', 'scopes', 'issuedAt', 'expiresAt'}
        : {'header', 'agentId', 'keyEpoch', 'agentKey', 'scopes', 'capabilities', 'origin', 'notBefore', 'expiresAt'},
  );
  final header = readerMap(value['header']);
  readerKeys(header, {'version', 'context', 'ownerEpoch', 'checkpoint'});
  if (header['version'] != 1) readerInvalid();
  readerDisplayU64(header['ownerEpoch'], positive: true);
  readerDisplayU64(header['checkpoint']);
  if (!readerId(value['agentId']) || jsonEncode(value['scopes']) != '["telemetry.read"]') readerInvalid();
  readerHash(value['agentKey']);
  if (offer) {
    if (!readerId(value['invitationId'])) readerInvalid();
    readerHash(value['tokenHash']);
  } else {
    if (value['keyEpoch'] != 1) readerInvalid();
    final capabilities = readerMap(value['capabilities']);
    readerKeys(capabilities, {'version', 'scopes'});
    if (capabilities['version'] != 1 || jsonEncode(capabilities['scopes']) != '["telemetry.read"]') readerInvalid();
  }
  readerInteger(review['signingNotBefore'], min: 1, max: 253402300799);
  readerInteger(review['signingExpiresAt'], min: 1, max: 253402300799);
}

final class AgentReaderReview {
  AgentReaderReview._(this.id, this.remainingMilliseconds, this.kind, this.facts);
  factory AgentReaderReview.parse(String raw, AgentReaderSelection selection, AgentReaderAction requested) {
    final top = readerDecode(raw);
    readerKeys(top, {'reviewId', 'details', 'remainingMilliseconds', 'humanConfirmationRequired', 'deadlineRenewable'});
    if (!readerId(top['reviewId']) || top['humanConfirmationRequired'] != true || top['deadlineRenewable'] != false) {
      readerInvalid();
    }
    final details = readerMap(top['details']), kind = details['kind'];
    if (kind != (requested == AgentReaderAction.reconcile ? 'reconcileKnownHeadOnly' : requested.name)) readerInvalid();
    final fields = switch (requested) {
      AgentReaderAction.nodeOffer ||
      AgentReaderAction.nodeCert => {'kind', 'native', 'review', 'witness', 'actorApply'},
      AgentReaderAction.issuerInit => {'kind', 'native'},
      AgentReaderAction.association => {'kind', 'native', 'target', 'issuerPin', 'expectedHead', 'transportBinding'},
      AgentReaderAction.revoke => {
        'kind',
        'native',
        'target',
        'installedHead',
        'associationDigest',
        'subject',
        'association',
        'localCancellationFirst',
      },
      _ => {'kind', 'native', 'target', 'installedHead', 'associationDigest'},
    };
    readerKeys(details, fields);
    selection.verifyNative(readerMap(details['native']));
    if (requested == AgentReaderAction.nodeOffer || requested == AgentReaderAction.nodeCert) {
      if (details['witness'] != 'operatorImport' || details['actorApply'] != 'notObserved') readerInvalid();
      readerNodeReview(readerMap(details['review']), requested);
    }
    if (requested == AgentReaderAction.revoke &&
        (details['localCancellationFirst'] != true ||
            !{'association', 'originalDevice'}.contains(details['subject']))) {
      readerInvalid();
    }
    return AgentReaderReview._(
      top['reviewId'] as String,
      readerInteger(top['remainingMilliseconds'], min: 1, max: 60000),
      requested,
      readerPublicFacts(details),
    );
  }
  final String id;
  final int remainingMilliseconds;
  final AgentReaderAction kind;
  final List<AgentReaderFact> facts;
}

final class AgentReaderOutcome {
  AgentReaderOutcome._(
    this.kind, {
    this.publicPath,
    this.hasInvitation = false,
    this.locator,
    this.targetHash,
    this.expiresAt,
  });
  factory AgentReaderOutcome.parse(String raw, AgentReaderAction action) {
    final m = readerDecode(raw), kind = readerText(m['kind'], 40);
    if (kind == 'nodeOfferExported' || kind == 'nodeCertificateExported') {
      readerKeys(m, {
        'kind',
        'publicArtifactPath',
        'privateInvitationPath',
        'sha256',
        'bytes',
        'expiresAt',
        'actorApply',
        'cleanup',
      });
      if (kind !=
              (action == AgentReaderAction.nodeOffer
                  ? 'nodeOfferExported'
                  : action == AgentReaderAction.nodeCert
                  ? 'nodeCertificateExported'
                  : '') ||
          m['actorApply'] != 'notObserved' ||
          m['cleanup'] != 'cancelDropOrOriginalDeadline') {
        readerInvalid();
      }
      readerHash(m['sha256']);
      readerInteger(m['bytes'], min: 1, max: 16384);
      readerInteger(m['expiresAt'], min: 1, max: 253402300799);
      final invitation = kind == 'nodeOfferExported';
      if (invitation) {
        readerText(m['privateInvitationPath'], 4096);
      } else if (m['privateInvitationPath'] != null) {
        readerInvalid();
      }
      return AgentReaderOutcome._(
        kind,
        publicPath: readerText(m['publicArtifactPath'], 4096),
        hasInvitation: invitation,
        expiresAt: m['expiresAt'] as int,
      );
    }
    if (kind == 'issuerReady' && action == AgentReaderAction.issuerInit) {
      readerKeys(m, {'kind', 'issuerId', 'publicKey'});
      if (!readerId(m['issuerId'])) readerInvalid();
      readerHash(m['publicKey']);
      return AgentReaderOutcome._(kind);
    }
    if (kind == 'associationInstalled' && action == AgentReaderAction.association) {
      readerKeys(m, {'kind', 'peerDigest', 'associationDigest', 'ack'});
      if (m['ack'] != 'observedThisOperation') readerInvalid();
      return AgentReaderOutcome._(
        kind,
        locator: AgentReaderLocator(readerHash(m['peerDigest']), readerHash(m['associationDigest'])),
      );
    }
    if (kind == 'readReady' && action == AgentReaderAction.read) {
      readerKeys(m, {'kind', 'targetHash', 'expiresAt'});
      return AgentReaderOutcome._(
        kind,
        targetHash: readerHash(m['targetHash']),
        expiresAt: readerInteger(m['expiresAt'], min: 1, max: 253402300799),
      );
    }
    if (kind == 'readerRevocationApplied' && action == AgentReaderAction.revoke) {
      readerKeys(m, {'kind', 'ack'});
      if (m['ack'] != 'observedThisOperation') readerInvalid();
      return AgentReaderOutcome._(kind);
    }
    if (kind == 'knownHeadReconciled' && action == AgentReaderAction.reconcile) {
      readerKeys(m, {'kind', 'newApply'});
      if (m['newApply'] != false) readerInvalid();
      return AgentReaderOutcome._(kind);
    }
    return readerInvalid();
  }
  final String kind;
  final String? publicPath, targetHash;
  final bool hasInvitation;
  final AgentReaderLocator? locator;
  final int? expiresAt;
}

final class AgentReaderStatus {
  AgentReaderStatus._(
    this.facts,
    this.locator,
    this.issuerReady,
    this.endpointObserved,
    this.choices,
    this.choicesTruncated,
  );
  factory AgentReaderStatus.parse(String raw, AgentReaderSelection selection) {
    final m = readerDecode(raw);
    readerKeys(m, {
      'native',
      'issuer',
      'association',
      'endpoint',
      'runtime',
      'nodeActorApply',
      'remoteApplyAck',
      'associationChoices',
    });
    selection.verifyNative(readerMap(m['native']));
    if (m['nodeActorApply'] != 'unknown' || m['remoteApplyAck'] != 'notObservedByStatus') readerInvalid();
    final runtime = readerMap(m['runtime']);
    readerKeys(runtime, {'busySlots', 'excludesThisStatus', 'quarantinedSlots'});
    readerInteger(runtime['busySlots'], max: 8);
    readerInteger(runtime['quarantinedSlots'], max: 8);
    if (runtime['excludesThisStatus'] != true) readerInvalid();
    final issuer = readerMap(m['issuer']);
    readerKeys(issuer, {'state', 'issuerId', 'publicKey'});
    final ready = issuer['state'] == 'ready';
    if (ready) {
      if (!readerId(issuer['issuerId'])) readerInvalid();
      readerHash(issuer['publicKey']);
    } else if (issuer['state'] != 'unknown' || issuer['issuerId'] != null || issuer['publicKey'] != null) {
      readerInvalid();
    }
    final association = readerMap(m['association']);
    AgentReaderLocator? locator;
    if (association['state'] == 'unknown') {
      readerKeys(association, {'state'});
    } else {
      readerKeys(association, {
        'state',
        'peerDigest',
        'associationDigest',
        'latestSignedHead',
        'locallyRecordedInstalledHead',
        'target',
        'meaning',
      });
      if (association['state'] != 'verifiedLocal' ||
          association['meaning'] != 'verifiedLocalRecordNotCurrentRemoteAck') {
        readerInvalid();
      }
      locator = AgentReaderLocator(readerHash(association['peerDigest']), readerHash(association['associationDigest']));
    }
    final endpoint = readerMap(m['endpoint']);
    final observed = endpoint['state'] == 'observed';
    if (observed) {
      readerKeys(endpoint, {
        'state',
        'agentId',
        'nodeContext',
        'agentCertificateHash',
        'agentKeyEpoch',
        'targets',
        'sshKeyDigest',
        'meaning',
      });
      if (!readerId(endpoint['agentId']) || endpoint['meaning'] != 'authenticatedMetadataNotIssuerApproval') {
        readerInvalid();
      }
      readerHash(endpoint['agentCertificateHash']);
      readerHash(endpoint['sshKeyDigest']);
      readerInteger(endpoint['agentKeyEpoch'], min: 1);
    } else {
      readerKeys(endpoint, {'state'});
      if (endpoint['state'] != 'unavailable') readerInvalid();
    }
    final choicesJson = readerMap(m['associationChoices']);
    readerKeys(choicesJson, {'state', 'items', 'truncated', 'provenance'});
    if (!{'available', 'unavailable'}.contains(choicesJson['state']) ||
        choicesJson['truncated'] is! bool ||
        choicesJson['provenance'] != 'verifiedLocalAuthorityNotRemoteAck') {
      readerInvalid();
    }
    final rawChoices = choicesJson['items'];
    if (rawChoices is! List ||
        rawChoices.length > 16 ||
        choicesJson['state'] == 'unavailable' && rawChoices.isNotEmpty) {
      readerInvalid();
    }
    final choices = rawChoices.map((v) => AgentReaderAssociationChoice.parse(readerMap(v))).toList();
    if (choices.map((v) => v.locator.peerDigest + v.locator.associationDigest).toSet().length != choices.length) {
      readerInvalid();
    }
    return AgentReaderStatus._(
      readerPublicFacts(Map<String, dynamic>.of(m)..remove('associationChoices')),
      locator,
      ready,
      observed,
      List.unmodifiable(choices),
      choicesJson['truncated'] as bool,
    );
  }
  final List<AgentReaderFact> facts;
  final AgentReaderLocator? locator;
  final bool issuerReady, endpointObserved;
  final List<AgentReaderAssociationChoice> choices;
  final bool choicesTruncated;
}

const readerAgentMetrics = ['agent_uptime_seconds', 'agent_collector_running'];
const readerOsMetrics = [
  'node_memory_MemTotal_bytes',
  'node_memory_MemAvailable_bytes',
  'node_load1',
  'node_boot_time_seconds',
];

final class AgentReaderPoint {
  AgentReaderPoint(this.at, this.values, {required this.gap, required this.sourceTimes, this.collectionState});
  final DateTime at;
  final Map<String, double?> values;
  final bool gap;
  final Map<String, DateTime?> sourceTimes;

  /// Recorded state of these Agent gauges only, never a collection grant.
  final String? collectionState;
}

final class AgentReaderDataset {
  AgentReaderDataset._({
    required this.targetHash,
    required this.kind,
    this.observedAt,
    this.uptimeSeconds,
    this.agentVersion,
    this.agentId,
    this.hostId,
    this.collectionState,
    this.metrics = const [],
    this.points = const [],
    this.truncated = false,
    this.evicted = 0,
    this.storageState,
  });
  factory AgentReaderDataset.parse(String raw, AgentReaderOutcome original) {
    final envelope = readerDecode(raw, maxBytes: 270336);
    readerKeys(envelope, {'kind', 'targetHash', 'expiresAt', 'bodyJson'});
    if (envelope['kind'] != 'readerDataset' ||
        readerHash(envelope['targetHash']) != original.targetHash ||
        envelope['expiresAt'] != original.expiresAt) {
      readerInvalid();
    }
    final body = readerDecode(envelope['bodyJson'] as String, maxBytes: 262144);
    DateTime time(Object? v) {
      if (v is! num || !v.isFinite || v < 0 || v > 253402300799) readerInvalid();
      return DateTime.fromMillisecondsSinceEpoch((v * 1000).round(), isUtc: true);
    }

    double? number(Object? v) {
      if (v == null) return null;
      if (v is! num || !v.isFinite || v < 0) readerInvalid();
      return v.toDouble();
    }

    if (body.containsKey('schema_version') && body.containsKey('uptime_seconds')) {
      readerKeys(body, {
        'schema_version',
        'agent_id',
        'host_id',
        'host_binding',
        'version',
        'uptime_seconds',
        'timestamp',
        'collection',
        'capabilities',
        'audit',
        'history_available_while_online_only',
      });
      if (body['schema_version'] != 1 ||
          !readerId(body['agent_id']) ||
          !readerId(body['host_id']) ||
          body['host_binding'] != 'configured_not_identity_attested' ||
          body['history_available_while_online_only'] != true) {
        readerInvalid();
      }
      final version = readerText(body['version'], 64);
      if (!RegExp(r'^[A-Za-z0-9.+-]+$').hasMatch(version)) readerInvalid();
      final state = readerMap(body['collection'])['state'];
      if (!{'disabled', 'starting', 'running', 'degraded', 'stopped', 'blocked', 'unknown'}.contains(state)) {
        readerInvalid();
      }
      return AgentReaderDataset._(
        targetHash: original.targetHash!,
        kind: 'heartbeatV1',
        observedAt: time(body['timestamp']),
        uptimeSeconds: number(body['uptime_seconds']),
        agentVersion: version,
        agentId: body['agent_id'] as String,
        hostId: body['host_id'] as String,
        collectionState: state as String,
      );
    }
    final metrics = body['metrics'];
    if (metrics is! List || metrics.any((m) => m is! String)) readerInvalid();
    final os = jsonEncode(metrics) == jsonEncode(readerOsMetrics),
        local = jsonEncode(metrics) == jsonEncode(readerAgentMetrics);
    if (!os && !local) readerInvalid();
    readerKeys(body, {
      'metrics',
      'points',
      'truncated',
      'evicted_points',
      'stored_bytes',
      'budget_bytes',
      'effective_data_budget_bytes',
      'max_age_seconds',
      'actual_start',
      'actual_end',
      'storage_state',
      'available_while_agent_online_only',
      if (os) ...{
        'history_kind',
        'schema_version',
        'agent_id',
        'host_id',
        'resource_id',
        'restore_complete',
        'clock_state',
        'read_grants_collection',
        'production_append_available',
      },
    });
    if (os &&
        (body['schema_version'] != 1 ||
            body['history_kind'] != 'fixed_os_snapshot' ||
            body['restore_complete'] != true ||
            body['read_grants_collection'] != false ||
            body['production_append_available'] != false ||
            body['clock_state'] != 'ok')) {
      readerInvalid();
    }
    final rawPoints = body['points'];
    if (rawPoints is! List || rawPoints.length > (os ? 100 : 1000)) readerInvalid();
    final points = <AgentReaderPoint>[];
    for (final rawPoint in rawPoints) {
      final p = readerMap(rawPoint), at = time(p['timestamp']);
      if (points.isNotEmpty && at.isBefore(points.last.at)) readerInvalid();
      final hasCollectionState =
          !os && (p.containsKey('collection_state') || p.containsKey('collection_state_version'));
      readerKeys(p, {
        'timestamp',
        ...metrics.cast<String>(),
        if (hasCollectionState) ...{'collection_state', 'collection_state_version'},
        if (os) ...{'source_timestamps', 'timestamp_kinds', 'unknown_reasons', 'provenance', 'gap'},
      });
      final values = <String, double?>{}, times = <String, DateTime?>{};
      for (final metric in metrics.cast<String>()) {
        values[metric] = number(p[metric]);
        if (metric == 'agent_collector_running' &&
            p[metric] != null &&
            (p[metric] is! int || !{0, 1}.contains(p[metric]))) {
          readerInvalid();
        }
        times[metric] = os && readerMap(p['source_timestamps'])[metric] != null
            ? time(readerMap(p['source_timestamps'])[metric])
            : null;
      }
      String? collectionState;
      if (!os) {
        final running = p['agent_collector_running'];
        if (hasCollectionState) {
          readerInteger(p['collection_state_version'], min: 1, max: 1);
          collectionState = readerText(p['collection_state'], 32);
          final expected = switch (collectionState) {
            'running' => 1,
            'disabled' || 'stopped' || 'not_running' => 0,
            'starting' || 'degraded' || 'blocked' || 'unknown' => null,
            _ => readerInvalid(),
          };
          if (running != expected) readerInvalid();
        } else {
          // An old zero cannot distinguish explicit disable from a stopped
          // collector. Missing remains unknown; it must never become zero.
          collectionState = running == 1
              ? 'running'
              : running == 0
              ? 'not_running'
              : 'unknown';
        }
      }
      if (os && p['gap'] is! bool) readerInvalid();
      points.add(
        AgentReaderPoint(
          at,
          Map.unmodifiable(values),
          gap: os ? p['gap'] as bool : false,
          sourceTimes: Map.unmodifiable(times),
          collectionState: collectionState,
        ),
      );
    }
    if (body['truncated'] is! bool || body['available_while_agent_online_only'] != true) readerInvalid();
    final start = body['actual_start'], end = body['actual_end'];
    if (points.isEmpty
        ? start != null || end != null
        : start == null || end == null || time(start) != points.first.at || time(end) != points.last.at) {
      readerInvalid();
    }
    final storage = readerText(body['storage_state'], 64);
    return AgentReaderDataset._(
      targetHash: original.targetHash!,
      kind: os ? 'fixedOsHistoryV1' : 'localAgentHistoryV1',
      metrics: List.unmodifiable(metrics.cast<String>()),
      points: List.unmodifiable(points),
      truncated: body['truncated'] as bool,
      evicted: readerInteger(body['evicted_points']),
      storageState: storage,
    );
  }
  final String targetHash, kind;
  final DateTime? observedAt;
  final double? uptimeSeconds;
  final String? agentVersion, agentId, hostId, collectionState, storageState;
  final List<String> metrics;
  final List<AgentReaderPoint> points;
  final bool truncated;
  final int evicted;
}

final class AgentReaderAssociationChoice {
  AgentReaderAssociationChoice._(
    this.locator,
    this.agentId,
    this.resourceId,
    this.action,
    this.datasetKind,
    this.targetHash,
    this.expiresAt,
    this.deviceId,
  );
  factory AgentReaderAssociationChoice.parse(Json m) {
    readerKeys(m, {
      'peerDigest',
      'associationDigest',
      'agentId',
      'resourceId',
      'action',
      'datasetKind',
      'schemaVersion',
      'targetHash',
      'expiresAt',
      'deviceId',
      'scope',
    });
    if (![m['agentId'], m['resourceId'], m['deviceId']].every(readerId) ||
        !{'heartbeatRead', 'historyRead'}.contains(m['action']) ||
        !{'heartbeatV1', 'localAgentHistoryV1', 'fixedOsHistoryV1'}.contains(m['datasetKind']) ||
        m['schemaVersion'] != 1 ||
        m['scope'] != 'telemetry.read') {
      readerInvalid();
    }
    if ((m['action'] == 'heartbeatRead') != (m['datasetKind'] == 'heartbeatV1')) readerInvalid();
    return AgentReaderAssociationChoice._(
      AgentReaderLocator(readerHash(m['peerDigest']), readerHash(m['associationDigest'])),
      m['agentId'] as String,
      m['resourceId'] as String,
      m['action'] as String,
      m['datasetKind'] as String,
      readerHash(m['targetHash']),
      readerInteger(m['expiresAt'], min: 1, max: 253402300799),
      m['deviceId'] as String,
    );
  }
  final AgentReaderLocator locator;
  final String agentId, resourceId, action, datasetKind, targetHash, deviceId;
  final int expiresAt;
}
