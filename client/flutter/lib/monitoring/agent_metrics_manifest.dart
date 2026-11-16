import 'dart:convert';

enum AgentMetricsStorageProfile { volatileRam, persistentNativeWal }

/// Closed Native runtime metadata: fixed vmagent tuples or the distinct
/// finite disk runtime. Neither branch grants persistent execution.
/// This validates a display DTO; it does not inspect an ELF, verify installation,
/// recompute a manifest digest/collector UUID, or establish running collection.
enum AgentMetricsRuntimePlatform { linuxX86_64, linuxAarch64 }

enum AgentMetricsRuntimeKind { vmagent, prometheusHardmode }

final class AgentMetricsRuntime {
  const AgentMetricsRuntime._(this.runtimeKind, this.platform, this.binarySha256, this.binaryBytes);
  final AgentMetricsRuntimeKind runtimeKind;
  final AgentMetricsRuntimePlatform platform;
  final String binarySha256;
  final int binaryBytes;
  bool get persistentDisk => runtimeKind == AgentMetricsRuntimeKind.prometheusHardmode;
  String get kind => persistentDisk ? 'prometheusHardmode' : 'vmagent';
  String get upstreamTag => persistentDisk ? '3.16.0' : 'v1.156.0';
  String get platformName => switch (platform) {
    AgentMetricsRuntimePlatform.linuxX86_64 => 'linux-x86_64',
    AgentMetricsRuntimePlatform.linuxAarch64 => 'linux-aarch64',
  };
}

const agentMetricsDiskProfile = 'maintained-prometheus-disk-512mib-2h-v1';

/// Exact reviewed fixed-budget projection, never a caller-selected budget.
final class AgentMetricsDiskBudget {
  const AgentMetricsDiskBudget();
  int get maxChargedBytes => 536870912;
  int get maxResidenceSeconds => 7200;
  int get minBavailBytes => 134217728;
  int get operationalWalBytes => 67108864;
  int get checkpointBytes => 67108864;
  int get reservedBytes => 134217728;
  int get maxRootEntries => 256;
}

/// Closed read-only Native catalog projection. Never a receiver/target input.
final class AgentMetricsTarget {
  const AgentMetricsTarget({
    required this.id,
    required this.url,
    required this.kind,
    required this.sampleLimit,
    required this.seriesLimit,
    required this.responseBytes,
    required this.labelLimit,
  });
  final String id, url, kind;
  final int sampleLimit, seriesLimit, responseBytes, labelLimit;
}

final class AgentMetricsManifest {
  AgentMetricsManifest({
    required this.hostId,
    required this.site,
    required this.environment,
    required List<AgentMetricsTarget> targets,
    required this.scrapeIntervalSeconds,
    required this.receiverUrl,
    this.storageProfile = AgentMetricsStorageProfile.volatileRam,
    this.runtime,
  }) : targets = List.unmodifiable(targets) {
    final selectedRuntime = runtime;
    if (selectedRuntime != null && selectedRuntime.persistentDisk != persistentDisk) _bad();
  }
  final AgentMetricsRuntime? runtime;
  int get schemaVersion => runtime == null ? 1 : 2;
  final AgentMetricsStorageProfile storageProfile;
  bool get persistentDisk => storageProfile == AgentMetricsStorageProfile.persistentNativeWal;
  final String hostId, site, environment, receiverUrl;
  final List<AgentMetricsTarget> targets;
  final int scrapeIntervalSeconds;
  String get mode => 'remote_write';
  int get maxBufferBytes => persistentDisk ? 536870912 : 33554432;
  int get maxBufferAgeSeconds => persistentDisk ? 7200 : 300;
}

final class AgentMetricsCollectorCatalog {
  const AgentMetricsCollectorCatalog(this.collectorId, this.manifestSha256, this.manifest);
  final String collectorId, manifestSha256;
  final AgentMetricsManifest manifest;
}

Never _bad() => throw const FormatException('Metrics catalog invalid');
Map<String, dynamic> _keys(Object? v, Set<String> keys) {
  if (v is! Map<String, dynamic> || v.length != keys.length || !v.keys.every(keys.contains)) _bad();
  return v;
}

int _int(Object? v, int min, int max) => v is int && v >= min && v <= max ? v : _bad();
String _name(Object? v) => v is String && RegExp(r'^[A-Za-z][A-Za-z0-9_-]{0,31}$').stringMatch(v) == v ? v : _bad();
String _id(Object? v) =>
    v is String &&
        v != '00000000-0000-0000-0000-000000000000' &&
        RegExp(r'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$').stringMatch(v) == v
    ? v
    : _bad();
String _hash(Object? v) => v is String && RegExp(r'^[0-9a-f]{64}$').stringMatch(v) == v ? v : _bad();
String _endpoint(Object? v, {required bool receiver}) {
  if (v is! String || v.length > 512 || v.codeUnits.any((x) => x < 33 || x > 126)) _bad();
  if (receiver) {
    final m = RegExp(r'^http://127\.0\.0\.1:([1-9][0-9]{0,4})/api/v1/(write|push)$').firstMatch(v);
    if (m == null || int.parse(m[1]!) > 65535) _bad();
    return v;
  }
  final m = RegExp(
    r'^(http|https)://(127\.(?:0|[1-9][0-9]{0,2})\.(?:0|[1-9][0-9]{0,2})\.(?:0|[1-9][0-9]{0,2})|\[::1\]):([1-9][0-9]{0,4})/metrics$',
  ).firstMatch(v);
  if (m == null || int.parse(m[3]!) > 65535) _bad();
  if (m[2] != '[::1]' && m[2]!.split('.').any((x) => int.parse(x) > 255)) _bad();
  return v;
}

AgentMetricsRuntime _runtime(Object? value, {required bool disk}) {
  final r = _keys(value, {'kind', 'upstreamTag', 'platform', 'binarySha256', 'binaryBytes'});
  if (disk) {
    if (r['kind'] != 'prometheusHardmode' || r['upstreamTag'] != '3.16.0') _bad();
    final platform = switch (r['platform']) {
      'linux-x86_64' => AgentMetricsRuntimePlatform.linuxX86_64,
      'linux-aarch64' => AgentMetricsRuntimePlatform.linuxAarch64,
      _ => _bad(),
    };
    // Public metadata only: Native binds the FULL manifest; the Node verifies
    // its actual selected release/ELF. Do not substitute a local fixed hash.
    return AgentMetricsRuntime._(
      AgentMetricsRuntimeKind.prometheusHardmode,
      platform,
      _hash(r['binarySha256']),
      _int(r['binaryBytes'], 64, 201326592),
    );
  }
  if (r['kind'] != 'vmagent' || r['upstreamTag'] != 'v1.156.0') _bad();
  final (platform, digest, bytes) = switch (r['platform']) {
    'linux-x86_64' => (
      AgentMetricsRuntimePlatform.linuxX86_64,
      '2039bc05b6ad5c96b34b72801908896bf8a6a7bcab8ae32120cbb8e6217ef6f6',
      23209584,
    ),
    'linux-aarch64' => (
      AgentMetricsRuntimePlatform.linuxAarch64,
      '564346d8acd0979b62c2671ad586d63b243a925307bfa54d0d5f94bd50dda4f5',
      21299768,
    ),
    _ => _bad(),
  };
  if (_hash(r['binarySha256']) != digest) _bad();
  _int(r['binaryBytes'], bytes, bytes);
  return AgentMetricsRuntime._(AgentMetricsRuntimeKind.vmagent, platform, digest, bytes);
}

AgentMetricsManifest parseAgentMetricsManifest(Object? value, String expectedHost) {
  final version = value is Map<String, dynamic> ? value['schema_version'] : null;
  if (version is! int || (version != 1 && version != 2)) _bad();
  final v2 = version == 2;
  final disk = value is Map<String, dynamic> && (value.containsKey('profile') || value.containsKey('bufferMode'));
  final m = _keys(value, {
    'schema_version',
    'mode',
    'targets',
    'labels',
    'scrape_interval_seconds',
    'receiver',
    'buffer',
    if (disk) 'profile',
    if (disk) 'bufferMode',
    if (v2) 'runtime',
  });
  if (disk && (m['profile'] != agentMetricsDiskProfile || m['bufferMode'] != 'persistentNativeWal')) _bad();
  if (m['mode'] != 'remote_write') _bad();
  final runtime = v2 ? _runtime(m['runtime'], disk: disk) : null;
  final labels = _keys(m['labels'], {'cc_host_id', 'site', 'environment'}), host = _id(labels['cc_host_id']);
  if (host != expectedHost) _bad();
  final receiver = _keys(m['receiver'], {'url'}), buffer = _keys(m['buffer'], {'max_bytes', 'max_age_seconds'});
  final bytes = disk ? 536870912 : 33554432, age = disk ? 7200 : 300;
  _int(buffer['max_bytes'], bytes, bytes);
  _int(buffer['max_age_seconds'], age, age);
  final raw = m['targets'];
  if (raw is! List || raw.isEmpty || raw.length > 16) _bad();
  final ids = <String>{}, urls = <String>{}, targets = <AgentMetricsTarget>[];
  for (final value in raw) {
    final t = _keys(value, {'id', 'url', 'kind', 'sample_limit', 'series_limit', 'response_bytes', 'label_limit'}),
        id = _name(t['id']),
        url = _endpoint(t['url'], receiver: false);
    if (!ids.add(id) || !urls.add(url) || !const {'application', 'node_exporter', 'cadvisor'}.contains(t['kind'])) {
      _bad();
    }
    targets.add(
      AgentMetricsTarget(
        id: id,
        url: url,
        kind: t['kind'] as String,
        sampleLimit: _int(t['sample_limit'], 1, 50000),
        seriesLimit: _int(t['series_limit'], disk ? 256 : 1, 50000),
        responseBytes: _int(t['response_bytes'], 256, 8388608),
        labelLimit: _int(t['label_limit'], 4, 64),
      ),
    );
  }
  // Size only: the FULL v2 digest and digest-derived ID remain Native-owned.
  if (v2 && utf8.encode(jsonEncode(m)).length > 8192) _bad();
  return AgentMetricsManifest(
    runtime: runtime,
    storageProfile: disk ? AgentMetricsStorageProfile.persistentNativeWal : AgentMetricsStorageProfile.volatileRam,
    hostId: host,
    site: _name(labels['site']),
    environment: _name(labels['environment']),
    targets: targets,
    scrapeIntervalSeconds: _int(m['scrape_interval_seconds'], 1, 300),
    receiverUrl: _endpoint(receiver['url'], receiver: true),
  );
}

AgentMetricsCollectorCatalog parseAgentMetricsCollectorCatalog(Object? value, String expectedHost) {
  final m = _keys(value, {'collectorId', 'manifestSha256', 'manifest'});
  return AgentMetricsCollectorCatalog(
    _id(m['collectorId']),
    _hash(m['manifestSha256']),
    parseAgentMetricsManifest(m['manifest'], expectedHost),
  );
}
