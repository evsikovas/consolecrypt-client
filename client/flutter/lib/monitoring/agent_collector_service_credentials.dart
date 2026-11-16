import 'dart:convert';

import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_metrics_manifest.dart';

/// Selection chooses a distinct prepare entry. It is never sent as a flag
/// on confirm and cannot upgrade an already issued SingleStart review.
enum AgentCollectorServiceCredentialUse { singleStart, serviceLifetime }

abstract interface class AgentCollectorServiceCredentialsPort {
  Future<AgentCollectorServiceCredentialChoices> readCredentialChoices(RuntimeNoticeScope scope);
}

final class AgentCollectorServiceCredentialChoice {
  const AgentCollectorServiceCredentialChoice(this.id, this.revision, this.name);
  final String id, revision, name;
}

final class AgentCollectorServiceCredentialChoices {
  AgentCollectorServiceCredentialChoices(this.scope, List<AgentCollectorServiceCredentialChoice> choices)
    : choices = List.unmodifiable(choices);
  final RuntimeNoticeScope scope;
  final List<AgentCollectorServiceCredentialChoice> choices;
}

final class AgentCollectorServiceCredentialSelection {
  AgentCollectorServiceCredentialSelection({
    required this.credentialId,
    required this.credentialRevision,
    this.targetId,
  }) {
    checkedAssociationId(credentialId);
    associationNumber(credentialRevision, positive: true, signed: true);
    if (targetId != null && RegExp(r'^[A-Za-z][A-Za-z0-9_-]{0,31}$').stringMatch(targetId!) != targetId) _bad();
  }
  final String credentialId, credentialRevision;

  /// Null is the actual fixed receiver. A scrape ID must exist in the catalog.
  final String? targetId;
  Map<String, Object> get target => targetId == null ? {'kind': 'receiver'} : {'kind': 'scrape', 'targetId': targetId!};
  Map<String, Object> request() => {
    'target': target,
    'credentialId': credentialId,
    'expectedCredentialRevision': credentialRevision,
  };
  bool same(AgentCollectorServiceCredentialSelection other) =>
      targetId == other.targetId &&
      credentialId == other.credentialId &&
      credentialRevision == other.credentialRevision;
}

Never _bad() => throw const FormatException('Original collector credential references unavailable');
Map<String, dynamic> _keys(Object? v, Set<String> keys) {
  if (v is! Map<String, dynamic> || v.length != keys.length || !v.keys.every(keys.contains)) _bad();
  return v;
}

String _id(Object? value) => value is String ? checkedAssociationId(value) : _bad();
String _revision(Object? value) =>
    value is String && value.length <= 20 ? associationNumber(value, positive: true, signed: true) : _bad();
AgentCollectorServiceCredentialChoices parseAgentCollectorServiceCredentialChoices(
  String raw,
  RuntimeNoticeScope scope,
) {
  if (raw.isEmpty || utf8.encode(raw).length > 16384) _bad();
  final m = _keys(jsonDecode(raw), {'version', 'profileId', 'notificationScopeId', 'kind', 'credentials'}),
      rows = m['credentials'];
  if (m['version'] is! int ||
      m['version'] != 1 ||
      m['profileId'] != scope.profileId ||
      m['notificationScopeId'] != scope.nonce ||
      m['kind'] != 'finiteCollectorServiceCredentialChoices' ||
      rows is! List ||
      rows.length > 64) {
    _bad();
  }
  final seen = <String>{}, choices = <AgentCollectorServiceCredentialChoice>[];
  for (final raw in rows) {
    final r = _keys(raw, {'credentialId', 'credentialRevision', 'name'}), id = _id(r['credentialId']), name = r['name'];
    if (!seen.add(id) ||
        name is! String ||
        name.isEmpty ||
        utf8.encode(name).length > 256 ||
        name.runes.any((c) => c <= 31 || c >= 127 && c <= 159 || c >= 0xd800 && c <= 0xdfff)) {
      _bad();
    }
    choices.add(AgentCollectorServiceCredentialChoice(id, _revision(r['credentialRevision']), name));
  }
  return AgentCollectorServiceCredentialChoices(scope, choices);
}

List<AgentCollectorServiceCredentialSelection> checkedCollectorCredentialSelections(
  List<AgentCollectorServiceCredentialSelection> selections,
  AgentMetricsCollectorCatalog catalog,
) {
  if (selections.length > 2) _bad();
  final targets = <String?>{};
  for (final ref in selections) {
    if (!targets.add(ref.targetId) ||
        ref.targetId != null && !catalog.manifest.targets.any((t) => t.id == ref.targetId)) {
      _bad();
    }
  }
  // Native permits at most one scrape target plus the one fixed receiver.
  if (selections.where((r) => r.targetId != null).length > 1) _bad();
  return List.unmodifiable(selections);
}

List<AgentCollectorServiceCredentialSelection> parseCollectorCredentialSelections(
  Object? raw,
  AgentMetricsCollectorCatalog catalog,
) {
  if (raw is! List || raw.isEmpty || raw.length > 2) _bad();
  final refs = <AgentCollectorServiceCredentialSelection>[];
  for (final value in raw) {
    final r = _keys(value, {'target', 'credentialId', 'credentialRevision'}), target = r['target'];
    if (target is! Map<String, dynamic>) _bad();
    String? targetId;
    if (target['kind'] == 'receiver') {
      _keys(target, {'kind'});
    } else if (target['kind'] == 'scrape') {
      _keys(target, {'kind', 'targetId'});
      if (target['targetId'] is! String) _bad();
      targetId = target['targetId'] as String;
    } else {
      _bad();
    }
    refs.add(
      AgentCollectorServiceCredentialSelection(
        credentialId: _id(r['credentialId']),
        credentialRevision: _revision(r['credentialRevision']),
        targetId: targetId,
      ),
    );
  }
  return checkedCollectorCredentialSelections(refs, catalog);
}

bool sameCollectorCredentialSelections(
  List<AgentCollectorServiceCredentialSelection> a,
  List<AgentCollectorServiceCredentialSelection> b,
) => a.length == b.length && List.generate(a.length, (i) => a[i].same(b[i])).every((v) => v);
