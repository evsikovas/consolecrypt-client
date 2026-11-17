import 'dart:convert';

import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_setup_models.dart';

Never _invalid() => throw const AgentSetupFailure(AgentSetupIssue.unavailable);

Map<String, dynamic> _object(Object? value, Set<String> keys) {
  if (value is! Map<String, dynamic> || value.length != keys.length || !value.keys.every(keys.contains)) {
    _invalid();
  }
  return value;
}

Map<String, dynamic> _decode(String raw) {
  if (raw.isEmpty || utf8.encode(raw).length > 2 * 1024 * 1024) {
    _invalid();
  }
  final value = jsonDecode(raw);
  if (value is! Map<String, dynamic>) {
    _invalid();
  }
  return value;
}

void _frame(Map<String, dynamic> value, RuntimeNoticeScope scope, String kind) {
  if (value['version'] != 1 ||
      value['version'] is! int ||
      value['kind'] != kind ||
      value['profileId'] != scope.profileId ||
      value['notificationScopeId'] != scope.nonce) {
    _invalid();
  }
}

AgentSetupHost _host(Object? value) {
  final row = _object(value, {'id', 'revision', 'name', 'address'});
  if (row['id'] is! String || row['revision'] is! String || row['name'] is! String || row['address'] is! String) {
    _invalid();
  }
  final id = checkedAssociationId(row['id'] as String);
  final revision = associationNumber(row['revision'] as String, positive: true, signed: true);
  final name = row['name'] as String, address = row['address'] as String;
  if (name.isEmpty || name.length > 1024 || address.isEmpty || address.length > 4096) {
    _invalid();
  }
  return AgentSetupHost(id: id, revision: revision, name: name, address: address);
}

AgentSetupContext parseAgentSetupContext(String raw, RuntimeNoticeScope scope) {
  final row = _object(_decode(raw), {
    'version',
    'kind',
    'profileId',
    'notificationScopeId',
    'hosts',
    'profiles',
    'releases',
    'destinations',
    'releaseState',
    'destinationState',
  });
  _frame(row, scope, 'agentSetupContext');
  final hosts = row['hosts'], profiles = row['profiles'];
  // Current Native preflight cannot admit a distribution or receiver yet.
  // Unknown future authority must not become an executable plan by accident.
  if (hosts is! List ||
      hosts.length > 10000 ||
      profiles is! List ||
      profiles.length != 1 ||
      row['releases'] is! List ||
      (row['releases'] as List).isNotEmpty ||
      row['destinations'] is! List ||
      (row['destinations'] as List).isNotEmpty ||
      row['releaseState'] != 'unavailable' ||
      row['destinationState'] != 'unconfigured') {
    _invalid();
  }
  final profile = _object(profiles.single, {'id', 'requiresCollection', 'pairingPurpose', 'available'});
  if (profile['id'] != 'linux_basic' ||
      profile['requiresCollection'] != true ||
      profile['pairingPurpose'] != 'metrics' ||
      profile['available'] != true) {
    _invalid();
  }
  final parsed = hosts.map(_host).toList();
  if (parsed.map((h) => h.id).toSet().length != parsed.length) {
    _invalid();
  }
  return AgentSetupContext(
    scope: scope,
    hosts: parsed,
    profiles: const [
      AgentSetupProfile(
        id: 'linux_basic',
        name: 'Linux',
        description: '',
        requiresCollection: true,
        pairingPurpose: AgentAssociationPurpose.metrics,
      ),
    ],
    releases: const [],
    destinations: const [],
  );
}

AgentSetupPlan parseAgentSetupPlan(String raw, RuntimeNoticeScope scope, AgentSetupSelection selection) {
  final row = _object(_decode(raw), {
    'version',
    'kind',
    'profileId',
    'notificationScopeId',
    'id',
    'host',
    'monitoringProfileId',
    'destinationRef',
    'release',
    'canProceed',
    'requiresInstall',
    'requiresPairing',
    'requiresCollection',
    'certificateLifetimeSeconds',
    'actions',
    'blocker',
    'blockers',
    'observations',
    'remoteMutationAttempted',
  });
  _frame(row, scope, 'agentSetupPlan');
  final host = _host(row['host']);
  if (host.id != selection.host.id ||
      host.revision != selection.host.revision ||
      host.name != selection.host.name ||
      host.address != selection.host.address ||
      row['monitoringProfileId'] != selection.profile.id ||
      selection.profile.id != 'linux_basic' ||
      selection.destination != null ||
      row['destinationRef'] != null ||
      row['release'] != null ||
      row['canProceed'] != false ||
      row['requiresInstall'] != false ||
      row['requiresPairing'] != false ||
      row['requiresCollection'] != true ||
      row['certificateLifetimeSeconds'] != 0 ||
      row['certificateLifetimeSeconds'] is! int ||
      row['remoteMutationAttempted'] != false ||
      row['actions'] is! List ||
      (row['actions'] as List).isNotEmpty ||
      row['blocker'] != 'releaseUnavailable') {
    _invalid();
  }
  final blockers = row['blockers'];
  if (blockers is! List ||
      blockers.length != 2 ||
      blockers[0] != 'releaseUnavailable' ||
      blockers[1] != 'destinationUnavailable') {
    _invalid();
  }
  final observations = _object(row['observations'], {'software', 'node', 'collectorRuntime', 'graphData'});
  if (!observations.values.every((v) => v == 'notObserved')) {
    _invalid();
  }
  if (row['id'] is! String) {
    _invalid();
  }
  return AgentSetupPlan(
    id: checkedAssociationId(row['id'] as String),
    selection: selection,
    canProceed: false,
    requiresInstall: false,
    requiresPairing: false,
    requiresCollection: true,
    actions: const [],
    blocker: AgentSetupIssue.releaseUnavailable,
  );
}

AgentSetupCleanup parseAgentSetupCleanup(String raw) {
  final row = _object(_decode(raw), {'version', 'kind', 'closedObserved', 'remoteMutationAttempted'});
  if (row['version'] != 1 ||
      row['version'] is! int ||
      row['kind'] != 'agentSetupCleanup' ||
      row['closedObserved'] != true ||
      row['remoteMutationAttempted'] != false) {
    _invalid();
  }
  return const AgentSetupCleanup(closedObserved: true);
}
