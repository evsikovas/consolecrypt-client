/// Display projection of the actual closed Native ManagedExporterPolicy.
/// The full manifest digest is supplied and verified by Native, never minted here.
final class AgentExporterPolicy {
  const AgentExporterPolicy({
    required this.hostId,
    required this.agentId,
    required this.platform,
    required this.artifactVersion,
    required this.binaryBytes,
    required this.archiveSha256,
    required this.binarySha256,
    required this.noticeInventorySha256,
    required this.maxSeconds,
  });
  final String hostId, agentId, platform, artifactVersion;
  final String archiveSha256, binarySha256, noticeInventorySha256;
  final int binaryBytes, maxSeconds;
  String get address => '127.0.0.1:19100';
  List<String> get collectors => const ['cpu', 'meminfo'];
}

Never exporterWireDenied() => throw const FormatException('Exporter observation unavailable');
Map<String, dynamic> exporterObject(Object? value, Set<String> keys) {
  if (value is! Map<String, dynamic> || value.length != keys.length || !value.keys.every(keys.contains)) {
    exporterWireDenied();
  }
  return value;
}

String exporterUuid(Object? value) {
  if (value is! String ||
      !RegExp(r'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$').hasMatch(value) ||
      value == '00000000-0000-0000-0000-000000000000') {
    exporterWireDenied();
  }
  return value;
}

String exporterSha(Object? value) {
  if (value is! String || !RegExp(r'^[0-9a-f]{64}$').hasMatch(value) || value == '0' * 64) {
    exporterWireDenied();
  }
  return value;
}

int exporterInt(Object? value, int low, int high) {
  if (value is! int || value < low || value > high) {
    exporterWireDenied();
  }
  return value;
}

String exporterDecimal(Object? value) {
  if (value is! String ||
      !RegExp(r'^[1-9][0-9]{0,18}$').hasMatch(value) ||
      BigInt.parse(value) > BigInt.parse('9223372036854775807')) {
    exporterWireDenied();
  }
  return value;
}

AgentExporterPolicy parseAgentExporterPolicy(Object? raw, String hostId, {String? agentId}) {
  final p = exporterObject(raw, {
    'format',
    'hostId',
    'agentId',
    'artifact',
    'bindIp',
    'bindPort',
    'collectors',
    'maxSeconds',
  });
  final a = exporterObject(p['artifact'], {
    'name',
    'version',
    'platform',
    'archiveSha256',
    'binarySha256',
    'binaryBytes',
    'noticeInventorySha256',
  });
  if (p['format'] != 'cc-agent-managed-node-exporter/v1' ||
      exporterUuid(p['hostId']) != hostId ||
      agentId != null && exporterUuid(p['agentId']) != agentId ||
      a['name'] != 'node_exporter' ||
      a['version'] != 'v1.12.1' ||
      !const {'linux-x86_64', 'linux-aarch64'}.contains(a['platform']) ||
      p['bindIp'] != '127.0.0.1' ||
      p['bindPort'] is! int ||
      p['bindPort'] != 19100 ||
      p['collectors'] is! List ||
      (p['collectors'] as List).length != 2 ||
      (p['collectors'] as List)[0] != 'cpu' ||
      (p['collectors'] as List)[1] != 'meminfo') {
    exporterWireDenied();
  }
  return AgentExporterPolicy(
    hostId: hostId,
    agentId: exporterUuid(p['agentId']),
    platform: a['platform'] as String,
    artifactVersion: a['version'] as String,
    archiveSha256: exporterSha(a['archiveSha256']),
    binarySha256: exporterSha(a['binarySha256']),
    noticeInventorySha256: exporterSha(a['noticeInventorySha256']),
    binaryBytes: exporterInt(a['binaryBytes'], 64, 64 * 1024 * 1024),
    maxSeconds: exporterInt(p['maxSeconds'], 30, 300),
  );
}

final class AgentExporterEnrollmentCatalog {
  const AgentExporterEnrollmentCatalog(this.policy, this.manifestSha256);
  final AgentExporterPolicy policy;
  final String manifestSha256;
}

AgentExporterEnrollmentCatalog parseAgentExporterEnrollmentCatalog(Object? raw, String hostId) {
  final m = exporterObject(raw, {'policy', 'manifestSha256'});
  return AgentExporterEnrollmentCatalog(
    parseAgentExporterPolicy(m['policy'], hostId),
    exporterSha(m['manifestSha256']),
  );
}
