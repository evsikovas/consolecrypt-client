import 'dart:convert';

import 'package:consolecrypt/automation/automation_service.dart';
import 'package:crypto/crypto.dart' as digest;

enum AutomationExporterTemplate { nodeExporter, cadvisor }
enum AutomationCadvisorOperation { inspect, install, start, stop, remove }

/// Identity of the shipped source, not a file-name or project-name heuristic.
const automationCadvisorTemplateSha256 = '18fdbbadb11d9cc37cd7f851cda04b0cf6b80ce5f67a3fc8780c61ac3f9f6b06';

final class AutomationCadvisorInstallOptions {
  AutomationCadvisorInstallOptions({required this.imageSha256, required this.imageId,
    required this.owner, required this.port, required bool hostAccessReviewed,
    this.operation = AutomationCadvisorOperation.install, this.containerId}) {
    final hash = RegExp(r'^[0-9a-f]{64}$');
    if (!hostAccessReviewed || !hash.hasMatch(imageSha256) || imageSha256 == '0' * 64 ||
        !imageId.startsWith('sha256:') || !hash.hasMatch(imageId.substring(7)) || imageId.substring(7) == '0' * 64 ||
        !RegExp(r'^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$').hasMatch(owner) ||
        port < 1024 || port > 65535 ||
        operation != AutomationCadvisorOperation.install &&
          (containerId == null || !hash.hasMatch(containerId!) || containerId == '0' * 64) ||
        operation == AutomationCadvisorOperation.install && containerId != null) {
      throw const FormatException('cadvisor_install_options');
    }
  }
  final String imageSha256, imageId, owner;
  final int port;
  final AutomationCadvisorOperation operation;
  final String? containerId;
  /// Composer values only. Ordinary Native review/confirmation grants execution.
  AutomationJson get extraVars => {
    'cc_exporter_mode': operation.name, 'cc_cadvisor_version': '0.60.6',
    'cc_cadvisor_image_sha256': imageSha256, 'cc_cadvisor_image_id': imageId,
    'cc_cadvisor_owner': owner, 'cc_cadvisor_port': port,
    'cc_cadvisor_host_access': 'privileged-host-filesystems-and-kmsg',
    if (containerId != null) 'cc_cadvisor_container_id': containerId,
  };
}

/// Part of the ordinary signed job parameters. This binds the component
/// definition to exactly one selected saved Host; it is not an Agent permit,
/// installed-target catalog update or observation of a running container.
AutomationJson bindCadvisorComponentManifest(AutomationJson values, String hostId) {
  final keys = {'cc_exporter_mode', 'cc_cadvisor_version', 'cc_cadvisor_image_sha256',
    'cc_cadvisor_image_id', 'cc_cadvisor_owner', 'cc_cadvisor_port', 'cc_cadvisor_host_access'};
  if (values['cc_exporter_mode'] != 'install') keys.add('cc_cadvisor_container_id');
  if (values.length != keys.length || !values.keys.every(keys.contains) ||
      values['cc_cadvisor_version'] != '0.60.6' ||
      values['cc_cadvisor_host_access'] != 'privileged-host-filesystems-and-kmsg' ||
      values['cc_cadvisor_image_sha256'] is! String || values['cc_cadvisor_image_id'] is! String ||
      values['cc_cadvisor_owner'] is! String || values['cc_cadvisor_port'] is! int ||
      values['cc_cadvisor_container_id'] != null && values['cc_cadvisor_container_id'] is! String ||
      !RegExp(r'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$').hasMatch(hostId) ||
      hostId == '00000000-0000-0000-0000-000000000000') {
    throw const FormatException('cadvisor_component_manifest');
  }
  final mode = AutomationCadvisorOperation.values.where((v) => v.name == values['cc_exporter_mode']);
  if (mode.length != 1) throw const FormatException('cadvisor_component_manifest');
  final options = AutomationCadvisorInstallOptions(imageSha256: values['cc_cadvisor_image_sha256'] as String,
    imageId: values['cc_cadvisor_image_id'] as String, owner: values['cc_cadvisor_owner'] as String,
    port: values['cc_cadvisor_port'] as int, hostAccessReviewed: true, operation: mode.single,
    containerId: values['cc_cadvisor_container_id'] as String?);
  final definition = <String, dynamic>{
    'schemaVersion': 1, 'component': 'cadvisor', 'managedBy': 'reviewedAnsible', 'hostId': hostId,
    'owner': options.owner, 'collectorVersion': '0.60.6', 'imageManifestSha256': options.imageSha256,
    'imageId': options.imageId, 'bindIp': '127.0.0.1', 'port': options.port, 'metricsPath': '/metrics',
  };
  final sorted = definition.keys.toList()..sort();
  final hash = digest.sha256.convert(utf8.encode(jsonEncode({for (final key in sorted) key: definition[key]}))).toString();
  return {...options.extraVars, 'cc_cadvisor_component_manifest': definition, 'cc_cadvisor_manifest_sha256': hash};
}

/// A captured managed project revision containing only the exact shipped asset.
/// Recheck before Native preparation and confirmation; never infer installation.
final class AutomationCadvisorTemplateBinding {
  const AutomationCadvisorTemplateBinding._(this.project, this.revisionDigest);
  final AutomationProject project;
  final String revisionDigest;
  static Future<AutomationCadvisorTemplateBinding> capture(AutomationService service, String projectId,
      {required bool Function() isCurrent}) async {
    void check() { if (!isCurrent()) throw const FormatException('cadvisor_template_changed'); }
    check();
    final catalog = await service.projects(); check();
    final matches = catalog.projects.where((p) => p.id == projectId).toList();
    if (catalog.readOnly || matches.length != 1) throw const FormatException('cadvisor_template_changed');
    final project = matches.single;
    if (project.readOnly || project.sourceKind != 'managed' || project.revision == null || project.revision! < 1) {
      throw const FormatException('cadvisor_template_changed');
    }
    final tree = await service.files(projectId); check();
    if (!RegExp(r'^[0-9a-f]{64}$').hasMatch(tree.digest) || tree.entries.length != 1 ||
        tree.entries.single.directory || tree.entries.single.path != 'playbook.yml') {
      throw const FormatException('cadvisor_template_changed');
    }
    final file = await service.readFile(projectId, 'playbook.yml'); check();
    if (file.path != 'playbook.yml' || file.sha256 != automationCadvisorTemplateSha256 ||
        digest.sha256.convert(utf8.encode(file.text)).toString() != automationCadvisorTemplateSha256) {
      throw const FormatException('cadvisor_template_changed');
    }
    return AutomationCadvisorTemplateBinding._(project, tree.digest);
  }
  Future<void> recheck(AutomationService service, {required bool Function() isCurrent}) async {
    final next = await capture(service, project.id, isCurrent: isCurrent);
    if (next.project.revision != project.revision || next.revisionDigest != revisionDigest) {
      throw const FormatException('cadvisor_template_changed');
    }
  }
}

/// Shipped reviewed source only. No user path, execution, host or credentials.
Future<Map<String, String>> loadExporterTemplate(
  AutomationExporterTemplate template,
  Future<String> Function(String asset) load,
) async {
  final sources = template == AutomationExporterTemplate.nodeExporter
      ? {'node-exporter.service.j2': 'node-exporter.service.j2', 'playbook.yml': 'node-exporter.yml'}
      : {'playbook.yml': 'cadvisor.yml'};
  final result = <String, String>{};
  var total = 0;
  for (final e in sources.entries) {
    final text = await load('assets/automation/exporters/${e.value}');
    final bytes = utf8.encode(text).length;
    total += bytes;
    if (bytes == 0 || bytes > 65536 || total > 131072) throw const FormatException('exporter_template_budget');
    result[e.key] = text;
  }
  return Map.unmodifiable(result);
}

/// Populate only a just-created managed workspace, through existing file CAS.
/// Partial failure is kept visible; this helper never deletes or executes it.
Future<void> populateExporterTemplate(
  AutomationService service,
  AutomationProject project,
  Map<String, String> files, {
  required bool Function() isCurrent,
}) async {
  void check() {
    if (!isCurrent()) throw const FormatException('exporter_template_changed');
  }

  check();
  if (project.readOnly ||
      project.sourceKind != 'managed' ||
      project.revision != 1 ||
      files.keys.any((p) => p != 'playbook.yml' && p != 'node-exporter.service.j2') ||
      !files.containsKey('playbook.yml')) {
    throw const FormatException('exporter_template_project');
  }
  final tree = await service.files(project.id);
  check();
  if (tree.entries.length != 1 || tree.entries.single.path != 'playbook.yml' || tree.entries.single.directory) {
    throw const FormatException('exporter_template_project_changed');
  }
  final initial = await service.readFile(project.id, 'playbook.yml');
  check();
  for (final entry in files.entries) {
    check();
    await service.saveFile(
      project.id,
      path: entry.key,
      text: entry.value,
      expectedSha256: entry.key == 'playbook.yml' ? initial.sha256 : null,
    );
    check();
  }
}
