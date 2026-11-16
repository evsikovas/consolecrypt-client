import 'package:consolecrypt/automation/automation_agent_installer.dart';
import 'package:consolecrypt/automation/automation_service.dart';

enum MonitoringNodeOperation { install, restore }

Never _invalid() => throw const FormatException('monitoring_node_source');

/// The same bounded input domain as the fixed Native MonitoringNodePlan.
/// These are planning inputs, not observed hardware or measured capacity.
final class MonitoringNodePlan {
  MonitoringNodePlan({
    this.cpuCores = 2,
    this.memoryMib = 2048,
    this.diskGib = 50,
    this.retentionDays = 15,
    this.retentionGib = 40,
    this.expectedSamplesPerSecond = 2000,
    this.receiverEnabled = false,
    this.alertmanagerEnabled = false,
  }) {
    if (cpuCores < 2 ||
        cpuCores > 64 ||
        memoryMib < 2048 ||
        memoryMib > 262144 ||
        diskGib < 10 ||
        diskGib > 10000 ||
        retentionDays < 1 ||
        retentionDays > 365 ||
        retentionGib < 1 ||
        retentionGib > 9000 ||
        expectedSamplesPerSecond < 1 ||
        expectedSamplesPerSecond > 1000000 ||
        retentionGib * 10 > diskGib * 8 ||
        estimatedSampleBytes > retentionGib * 1024 * 1024 * 1024) {
      _invalid();
    }
  }
  final int cpuCores, memoryMib, diskGib, retentionDays, retentionGib, expectedSamplesPerSecond;
  final bool receiverEnabled, alertmanagerEnabled;
  int get estimatedSampleBytes => 2 * expectedSamplesPerSecond * retentionDays * 86400;
  AutomationJson toJson() => {
    'cpu_cores': cpuCores,
    'memory_mib': memoryMib,
    'disk_gib': diskGib,
    'retention_days': retentionDays,
    'retention_gib': retentionGib,
    'receiver_enabled': receiverEnabled,
    'alertmanager_enabled': alertmanagerEnabled,
    'expected_samples_per_second': expectedSamplesPerSecond,
  };
  AutomationJson request({
    required AgentInstallerContext context,
    required AgentInstallerHost host,
    required String name,
    required MonitoringNodeOperation operation,
  }) {
    if (!context.hosts.any((h) => identical(h, host)) ||
        name.isEmpty ||
        name.trim() != name ||
        name.runes.length > 120 ||
        name.runes.any((c) => c < 32 || c >= 127 && c <= 159)) {
      _invalid();
    }
    return {
      ...context.frame.toJson(),
      'name': name,
      'operation': operation.name,
      'hostId': host.id,
      'expectedHostRevision': host.revision,
      'plan': toJson(),
    };
  }
}

Map<String, Object?> _closed(Object? raw, Set<String> fields) {
  if (raw is! Map<String, Object?> ||
      raw.keys.toSet().difference(fields).isNotEmpty ||
      fields.difference(raw.keys.toSet()).isNotEmpty) {
    _invalid();
  }
  return raw;
}

final class MonitoringNodeSourceCreated {
  MonitoringNodeSourceCreated._(this.project, this.projectDigest);
  factory MonitoringNodeSourceCreated.parse(AutomationJson raw, AgentInstallerContext context, AutomationJson request) {
    final v = _closed(raw, {
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
    context.frame.matches(v);
    if (v['claimedRelease'] != null ||
        v['actualRuntimeVersion'] != null ||
        v['playbook'] != 'playbook.yml' ||
        v['hostId'] != request['hostId'] ||
        v['hostRevision'] != request['expectedHostRevision'] ||
        !{'install', 'restore'}.contains(request['operation'])) {
      _invalid();
    }
    final p = _closed(v['project'], {
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
    final id = p['id'], path = p['path'], digest = v['projectDigest'];
    if (id is! String ||
        !RegExp(r'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$').hasMatch(id) ||
        id == '00000000-0000-0000-0000-000000000000' ||
        p['name'] != request['name'] ||
        p['source_kind'] != 'managed' ||
        p['revision'] is! int ||
        p['revision'] != 1 ||
        p['read_only'] != false ||
        p['import_state'] != null ||
        p['import_error'] != null ||
        p['git_source'] != null ||
        path is! String ||
        path.isEmpty ||
        path.length > 4096 ||
        path.runes.any((c) => c < 32 || c >= 127 && c <= 159) ||
        digest is! String ||
        !RegExp(r'^[0-9a-f]{64}$').hasMatch(digest)) {
      _invalid();
    }
    return MonitoringNodeSourceCreated._(AutomationProject.fromJson(Map<String, dynamic>.from(p)), digest);
  }
  final AutomationProject project;
  final String projectDigest;
}
