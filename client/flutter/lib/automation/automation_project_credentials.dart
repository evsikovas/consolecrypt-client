import 'dart:convert';

import 'package:consolecrypt/automation/automation_service.dart';

/// Optional Native metadata capability. No secret retrieval or connection API.
abstract interface class AutomationProjectCredentialsService {
  Future<AutomationJson> projectCredentialsContext(String projectId);
  Future<AutomationJson> saveProjectCredentials(AutomationJson request);
}

Never _invalid() => throw const FormatException('project_environment_credentials_changed_or_unavailable');
bool _id(Object? value) =>
    value is String &&
    value != '00000000-0000-0000-0000-000000000000' &&
    RegExp(r'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$').hasMatch(value);
bool _sha(Object? value) => value is String && RegExp(r'^[0-9a-f]{64}$').hasMatch(value);
bool _nonce(Object? value) =>
    value is String &&
    value.isNotEmpty &&
    utf8.encode(value).length <= 256 &&
    utf8.decode(utf8.encode(value)) == value &&
    !value.runes.any((c) => c < 32 || c >= 127 && c <= 159 || c >= 0xd800 && c <= 0xdfff);
bool _revision(Object? value) => value is int && value >= 1 && value <= 9007199254740991;
AutomationJson _object(Object? value, Set<String> keys) {
  if (value is! AutomationJson || value.length != keys.length || !value.keys.every(keys.contains)) _invalid();
  return value;
}

void _v1(Object? value) {
  if (value is! int || value != 1) _invalid();
}

bool _name(Object? value) =>
    value is String &&
    value.isNotEmpty &&
    value.trim() == value &&
    value.runes.length <= 120 &&
    !value.runes.any((c) => c < 32 || c >= 127 && c <= 159 || c >= 0xd800 && c <= 0xdfff);

final class ProjectCredentialsFrame {
  ProjectCredentialsFrame._(this.profileId, this.nonce, this.projectId, this.revision, this.policySha256);
  final String profileId, nonce, projectId, policySha256;
  final int revision;
  factory ProjectCredentialsFrame.parse(
    Object? value, {
    required String profileId,
    required String nonce,
    required String projectId,
    required int revision,
  }) {
    final row = _object(value, {
      'version',
      'profile_id',
      'notification_scope_id',
      'project_id',
      'project_revision',
      'policy_sha256',
    });
    _v1(row['version']);
    if (!_id(profileId) ||
        !_nonce(nonce) ||
        !_id(projectId) ||
        !_revision(revision) ||
        row['profile_id'] != profileId ||
        row['notification_scope_id'] != nonce ||
        row['project_id'] != projectId ||
        row['project_revision'] is! int ||
        row['project_revision'] != revision ||
        !_sha(row['policy_sha256'])) {
      _invalid();
    }
    return ProjectCredentialsFrame._(profileId, nonce, projectId, revision, row['policy_sha256'] as String);
  }
  AutomationJson toJson() => {
    'version': 1,
    'profile_id': profileId,
    'notification_scope_id': nonce,
    'project_id': projectId,
    'project_revision': revision,
    'policy_sha256': policySha256,
  };
}

final class DeploymentCredentialReference {
  DeploymentCredentialReference(this.hostId, this.credentialId) {
    if (!_id(hostId) || !_id(credentialId)) _invalid();
  }
  final String hostId, credentialId;
  factory DeploymentCredentialReference.parse(Object? value) {
    final row = _object(value, {'host_id', 'credential_id'});
    if (!_id(row['host_id']) || !_id(row['credential_id'])) _invalid();
    return DeploymentCredentialReference(row['host_id'] as String, row['credential_id'] as String);
  }
  AutomationJson toJson() => {'host_id': hostId, 'credential_id': credentialId};
}

final class DeploymentEnvironment {
  DeploymentEnvironment({
    required this.id,
    required this.name,
    required this.production,
    required List<DeploymentCredentialReference> references,
  }) : references = List.unmodifiable(references) {
    if (!_id(id) ||
        !_name(name) ||
        references.isEmpty ||
        references.length > 256 ||
        references.map((v) => v.hostId).toSet().length != references.length) {
      _invalid();
    }
  }
  final String id, name;
  final bool production;
  final List<DeploymentCredentialReference> references;
  factory DeploymentEnvironment.parse(Object? value) {
    final row = _object(value, {'id', 'name', 'production', 'overrides'}), refs = row['overrides'];
    if (!_id(row['id']) || !_name(row['name']) || row['production'] is! bool || refs is! List || refs.length > 256) {
      _invalid();
    }
    return DeploymentEnvironment(
      id: row['id'] as String,
      name: row['name'] as String,
      production: row['production'] as bool,
      references: refs.map(DeploymentCredentialReference.parse).toList(),
    );
  }
  AutomationJson toJson() => {
    'id': id,
    'name': name,
    'production': production,
    'overrides': [for (final ref in references) ref.toJson()],
  };
}

final class ProjectCredentialsPolicy {
  ProjectCredentialsPolicy(List<DeploymentEnvironment> environments) : environments = List.unmodifiable(environments) {
    if (environments.isEmpty ||
        environments.length > 16 ||
        environments.map((v) => v.id).toSet().length != environments.length ||
        utf8.encode(jsonEncode(toJson())).length > 65536) {
      _invalid();
    }
  }
  final List<DeploymentEnvironment> environments;
  static ProjectCredentialsPolicy? parse(Object? value) {
    if (value == null) return null;
    final row = _object(value, {'version', 'environments'}), values = row['environments'];
    _v1(row['version']);
    if (values is! List || values.isEmpty || values.length > 16) _invalid();
    return ProjectCredentialsPolicy(values.map(DeploymentEnvironment.parse).toList());
  }

  AutomationJson toJson() => {
    'version': 1,
    'environments': [for (final environment in environments) environment.toJson()],
  };
}

final class ProjectCredentialsContext {
  ProjectCredentialsContext._(this.frame, this.policy);
  final ProjectCredentialsFrame frame;
  final ProjectCredentialsPolicy? policy;
  factory ProjectCredentialsContext.parse(
    AutomationJson value, {
    required String profileId,
    required String nonce,
    required AutomationProject project,
  }) {
    if (utf8.encode(jsonEncode(value)).length > 67584) _invalid();
    _object(value, {'version', 'frame', 'policy', 'supported_credential_kinds'});
    _v1(value['version']);
    final kinds = value['supported_credential_kinds'];
    if (kinds is! List ||
        kinds.length != 3 ||
        kinds.toSet().length != 3 ||
        !kinds.toSet().containsAll({'password', 'ssh_private_key', 'ssh_certificate'}) ||
        project.revision == null) {
      _invalid();
    }
    return ProjectCredentialsContext._(
      ProjectCredentialsFrame.parse(
        value['frame'],
        profileId: profileId,
        nonce: nonce,
        projectId: project.id,
        revision: project.revision!,
      ),
      ProjectCredentialsPolicy.parse(value['policy']),
    );
  }
  AutomationJson selection(String environmentId) {
    if (policy?.environments.any((value) => value.id == environmentId) != true) _invalid();
    return {'version': 1, 'frame': frame.toJson(), 'environment_id': environmentId};
  }

  AutomationJson saveRequest(ProjectCredentialsPolicy? policy) => {
    'version': 1,
    'frame': frame.toJson(),
    'policy': policy?.toJson(),
  };
  ProjectCredentialsContext saved(AutomationJson result) {
    _object(result, {'version', 'project', 'frame', 'policy'});
    _v1(result['version']);
    final project = result['project'];
    if (project is! AutomationJson ||
        project['id'] != frame.projectId ||
        project['revision'] is! int ||
        project['revision'] != frame.revision + 1) {
      _invalid();
    }
    return ProjectCredentialsContext._(
      ProjectCredentialsFrame.parse(
        result['frame'],
        profileId: frame.profileId,
        nonce: frame.nonce,
        projectId: frame.projectId,
        revision: frame.revision + 1,
      ),
      ProjectCredentialsPolicy.parse(result['policy']),
    );
  }
}

/// Closed preview consistency; it never grants authority or lowers Native risk.
DeploymentEnvironment? checkedDeploymentReview(
  Object? raw,
  ProjectCredentialsContext? captured,
  String? selected,
  Object? effectiveProduction,
) {
  if (captured?.policy == null) {
    if (raw != null) _invalid();
    return null;
  }
  final env = captured!.policy!.environments.where((v) => v.id == selected).firstOrNull;
  final row = _object(raw, {'id', 'name', 'production', 'policy_sha256', 'credential_overrides'});
  if (env == null ||
      row['id'] != env.id ||
      row['name'] != env.name ||
      row['production'] is! bool ||
      effectiveProduction is! bool ||
      row['production'] != effectiveProduction ||
      env.production && !effectiveProduction ||
      row['policy_sha256'] != captured.frame.policySha256) {
    _invalid();
  }
  final refs = row['credential_overrides'];
  if (refs is! List || refs.length != env.references.length) _invalid();
  for (var i = 0; i < refs.length; i++) {
    final actual = DeploymentCredentialReference.parse(refs[i]), expected = env.references[i];
    if (actual.hostId != expected.hostId || actual.credentialId != expected.credentialId) _invalid();
  }
  return DeploymentEnvironment(id: env.id, name: env.name, production: effectiveProduction, references: env.references);
}
