import 'dart:convert';

import 'package:consolecrypt/automation/automation_service.dart';

/// Optional fixed source factory. This never installs, executes, signs a
/// release, enrolls an Agent or changes any target permission.
abstract interface class AutomationAgentInstallerService {
  Future<AutomationJson> agentInstallerContext(AutomationJson frame);
  Future<AutomationJson> createAgentInstallerProject(AutomationJson request);
}

enum AgentInstallerOperation { install, update, rollback, remove, status }

Never _bad() => throw const FormatException('agent_installer_metadata');
Map<String, Object?> _map(Object? value, Set<String> fields) {
  if (value is! Map<String, Object?> ||
      value.keys.toSet().difference(fields).isNotEmpty ||
      fields.difference(value.keys.toSet()).isNotEmpty) {
    _bad();
  }
  return value;
}

String _id(Object? value) {
  if (value is! String ||
      !RegExp(r'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$')
          .hasMatch(value) ||
      value == '00000000-0000-0000-0000-000000000000') {
    _bad();
  }
  return value;
}

String _nonce(Object? value) {
  if (value is! String ||
      value.isEmpty ||
      utf8.encode(value).length > 256 ||
      utf8.decode(utf8.encode(value)) != value ||
      value.runes.any(
        (c) => c < 32 || c >= 127 && c <= 159 || c >= 0xd800 && c <= 0xdfff,
      )) {
    _bad();
  }
  return value;
}

String _revision(Object? value) {
  if (value is! String ||
      !RegExp(r'^[1-9][0-9]{0,18}$').hasMatch(value) ||
      BigInt.parse(value) > BigInt.parse('9223372036854775807')) {
    _bad();
  }
  return value;
}

String _text(Object? value, int max) {
  if (value is! String ||
      value.isEmpty ||
      value.runes.length > max ||
      value.runes.any((c) => c < 32 || c >= 127 && c <= 159)) {
    _bad();
  }
  return value;
}

String _hash(Object? value) {
  if (value is! String || !RegExp(r'^[0-9a-f]{64}$').hasMatch(value)) _bad();
  return value;
}

void _v1(Object? value) {
  if (value is! int || value != 1) _bad();
}

final class AgentInstallerFrame {
  AgentInstallerFrame({
    required String profileId,
    required String notificationScopeId,
  }) : profileId = _id(profileId),
       notificationScopeId = _nonce(notificationScopeId);
  final String profileId, notificationScopeId;
  AutomationJson toJson() => {
    'version': 1,
    'profileId': profileId,
    'notificationScopeId': notificationScopeId,
  };
  void matches(Map<String, Object?> value) {
    _v1(value['version']);
    if (value['profileId'] != profileId ||
        value['notificationScopeId'] != notificationScopeId) {
      _bad();
    }
  }
}

final class AgentInstallerHost {
  AgentInstallerHost._(this.id, this.revision, this.name);
  final String id, revision, name;
}

final class AgentInstallerContext {
  AgentInstallerContext._(this.frame, this.hosts);
  factory AgentInstallerContext.parse(
    AutomationJson raw,
    AgentInstallerFrame frame,
  ) {
    final value = _map(raw, {
      'version',
      'profileId',
      'notificationScopeId',
      'hosts',
    });
    frame.matches(value);
    final hosts = value['hosts'];
    if (hosts is! List || hosts.length > 64) _bad();
    final ids = <String>{}, result = <AgentInstallerHost>[];
    for (final item in hosts) {
      final host = _map(item, {'hostId', 'hostRevision', 'name'}),
          id = _id(host['hostId']);
      if (!ids.add(id)) _bad();
      result.add(
        AgentInstallerHost._(
          id,
          _revision(host['hostRevision']),
          _text(host['name'], 256),
        ),
      );
    }
    return AgentInstallerContext._(frame, List.unmodifiable(result));
  }
  final AgentInstallerFrame frame;
  final List<AgentInstallerHost> hosts;
  AutomationJson request({
    required String name,
    required AgentInstallerHost host,
    required AgentInstallerOperation operation,
    required bool start,
    String? artifactPath,
    String? manifestPath,
  }) {
    if (!hosts.any((h) => identical(h, host)) || name.trim() != name) _bad();
    _text(name, 120);
    final release =
        operation == AgentInstallerOperation.install ||
        operation == AgentInstallerOperation.update;
    if (release != (artifactPath != null && manifestPath != null) ||
        !release && (artifactPath != null || manifestPath != null) ||
        start &&
            operation != AgentInstallerOperation.install &&
            operation != AgentInstallerOperation.update &&
            operation != AgentInstallerOperation.rollback) {
      _bad();
    }
    if (release) {
      _text(artifactPath, 4096);
      _text(manifestPath, 4096);
    }
    return {
      ...frame.toJson(),
      'name': name,
      'operation': operation.name,
      'start': start,
      'hostId': host.id,
      'expectedHostRevision': host.revision,
      'artifactPath': artifactPath,
      'manifestPath': manifestPath,
    };
  }
}

final class AgentInstallerCreated {
  AgentInstallerCreated._(
    this.project,
    this.projectDigest,
    this.artifactSha256,
    this.manifestSha256,
    this.artifactBytes,
  );
  factory AgentInstallerCreated.parse(
    AutomationJson raw,
    AgentInstallerContext context,
    AutomationJson request,
  ) {
    final value = _map(raw, {
      'version',
      'profileId',
      'notificationScopeId',
      'project',
      'projectDigest',
      'claimedRelease',
      'actualRuntimeVersion',
      'hostId',
      'hostRevision',
      'playbook',
    });
    context.frame.matches(value);
    if (value['actualRuntimeVersion'] != null ||
        value['playbook'] != 'playbook.yml' ||
        value['hostId'] != request['hostId'] ||
        value['hostRevision'] != request['expectedHostRevision']) {
      _bad();
    }
    final p = _map(value['project'], {
      'id',
      'name',
      'source_kind',
      'path',
      'revision',
      'read_only',
      'import_state',
      'import_error',
      'git_source',
    });
    _id(p['id']);
    if (p['name'] != request['name'] ||
        p['source_kind'] != 'managed' ||
        p['revision'] is! int ||
        p['revision'] != 1 ||
        p['read_only'] != false ||
        p['import_state'] != null ||
        p['import_error'] != null ||
        p['git_source'] != null ||
        p['path'] is! String) {
      _bad();
    }
    String? artifact, manifest;
    int? bytes;
    final release =
        request['operation'] == 'install' || request['operation'] == 'update';
    if (release) {
      final claimed = _map(value['claimedRelease'], {
        'artifactSha256',
        'artifactBytes',
        'manifestSha256',
        'signatureVerified',
      });
      artifact = _hash(claimed['artifactSha256']);
      manifest = _hash(claimed['manifestSha256']);
      final count = claimed['artifactBytes'];
      if (claimed['signatureVerified'] != false ||
          count is! int ||
          count < 1 ||
          count > 32 * 1024 * 1024) {
        _bad();
      }
      bytes = count;
    } else if (value['claimedRelease'] != null) {
      _bad();
    }
    return AgentInstallerCreated._(
      AutomationProject.fromJson(Map<String, dynamic>.from(p)),
      _hash(value['projectDigest']),
      artifact,
      manifest,
      bytes,
    );
  }
  final AutomationProject project;
  final String projectDigest;
  final String? artifactSha256, manifestSha256;
  final int? artifactBytes;
}
