import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';

abstract interface class KubeOperationsService {
  Future<KubePermissionReport> permissions(String sessionId, String namespace);
  Future<void> scale(String sessionId, KubeResourceRef resource, String resourceVersion, int replicas);
  Future<void> restart(String sessionId, KubeResourceRef resource, String resourceVersion);
  Future<void> cordon(String sessionId, String node, String resourceVersion, bool unschedulable);
  Future<List<KubeDrainCandidate>> drainPlan(String sessionId, String node);
  Future<void> evictPod(String sessionId, KubeResourceRef resource, String uid, String resourceVersion);
  Future<void> debugContainer(
    String sessionId,
    KubeResourceRef resource,
    String resourceVersion,
    KubeDebugContainer container,
  );
  Future<List<KubeResourceRow>> workloadPods(String sessionId, KubeResourceRef resource);
  Future<List<KubeResourceRow>> objectEvents(String sessionId, KubeResourceRef resource, String uid);
}

abstract interface class KubeTableService {
  Future<KubeResourceTable> table(
    String sessionId,
    KubeResourceKind kind, {
    String? namespace,
    String? labelSelector,
    String? continueToken,
    int limit = 200,
  });
}

abstract interface class KubeMetricsService {
  Future<List<KubeMetrics>> metrics(String sessionId, {required bool nodes, String? namespace});
}

final class KubePermissionReport {
  const KubePermissionReport({required this.incomplete, required this.canMutate, required this.rules});
  final bool incomplete, canMutate;
  final List<KubePermissionRule> rules;
  factory KubePermissionReport.fromJson(KubeJson json) => KubePermissionReport(
    incomplete: json['incomplete'] as bool? ?? true,
    canMutate: json['can_mutate'] as bool? ?? false,
    rules: (json['resource_rules'] as List? ?? const [])
        .whereType<KubeJson>()
        .map(KubePermissionRule.fromJson)
        .toList(),
  );
}

final class KubePermissionRule {
  const KubePermissionRule({
    required this.verbs,
    required this.apiGroups,
    required this.resources,
    required this.resourceNames,
  });
  final List<String> verbs, apiGroups, resources, resourceNames;
  factory KubePermissionRule.fromJson(KubeJson json) => KubePermissionRule(
    verbs: (json['verbs'] as List? ?? const []).whereType<String>().toList(),
    apiGroups: (json['api_groups'] as List? ?? const []).whereType<String>().toList(),
    resources: (json['resources'] as List? ?? const []).whereType<String>().toList(),
    resourceNames: (json['resource_names'] as List? ?? const []).whereType<String>().toList(),
  );
}

final class KubeDrainCandidate {
  const KubeDrainCandidate({required this.reference, required this.uid, required this.resourceVersion, this.blocker});
  final KubeResourceRef reference;
  final String uid, resourceVersion;
  final String? blocker;
  factory KubeDrainCandidate.fromJson(KubeJson json) => KubeDrainCandidate(
    reference: KubeResourceRef.fromJson(json['reference'] as KubeJson),
    uid: json['uid'] as String,
    resourceVersion: json['resource_version'] as String,
    blocker: json['blocker'] as String?,
  );
}

final class KubeDebugContainer {
  const KubeDebugContainer({
    required this.name,
    required this.image,
    this.targetContainer,
    this.command = const ['/bin/sh'],
  });
  final String name, image;
  final String? targetContainer;
  final List<String> command;
  KubeJson toJson() => {'name': name, 'image': image, 'target_container': targetContainer, 'command': command};
}

final class KubeTableColumn {
  const KubeTableColumn({
    required this.name,
    required this.type,
    required this.format,
    required this.description,
    required this.priority,
  });
  final String name, type, format, description;
  final int priority;
  factory KubeTableColumn.fromJson(KubeJson json) => KubeTableColumn(
    name: json['name'] as String? ?? '',
    type: json['type'] as String? ?? '',
    format: json['format'] as String? ?? '',
    description: json['description'] as String? ?? '',
    priority: json['priority'] as int? ?? 0,
  );
}

final class KubeResourceTable {
  const KubeResourceTable({required this.columns, required this.page});
  final List<KubeTableColumn> columns;
  final KubeResourcePage page;
  factory KubeResourceTable.fromJson(KubeJson json) => KubeResourceTable(
    columns: (json['columns'] as List? ?? const []).whereType<KubeJson>().map(KubeTableColumn.fromJson).toList(),
    page: KubeResourcePage.fromJson({
      'items': (json['rows'] as List? ?? const [])
          .whereType<KubeJson>()
          .map(
            (row) => {
              ...row['resource'] as KubeJson,
              'cells': (row['cells'] as List? ?? const []).map((cell) => cell?.toString() ?? '').toList(),
            },
          )
          .toList(),
      'resource_version': json['resource_version'],
      'continue_token': json['continue_token'],
    }),
  );
}

final class KubeMetrics {
  const KubeMetrics({
    required this.name,
    required this.cpuCores,
    required this.memoryBytes,
    this.namespace,
    this.timestamp,
    this.window,
    this.containers = const [],
    this.requests,
    this.limits,
    this.capacity,
    this.allocatable,
    this.resourcesAvailable = false,
  });
  final String name;
  final String? namespace, timestamp, window;
  final double cpuCores;
  final int memoryBytes;
  final List<KubeContainerMetrics> containers;
  final KubeResourceAmount? requests, limits, capacity, allocatable;
  final bool resourcesAvailable;
  factory KubeMetrics.fromJson(KubeJson json) => KubeMetrics(
    name: json['name'] as String,
    namespace: json['namespace'] as String?,
    timestamp: json['timestamp'] as String?,
    window: json['window'] as String?,
    requests: KubeResourceAmount.optional(json['requests']),
    limits: KubeResourceAmount.optional(json['limits']),
    capacity: KubeResourceAmount.optional(json['capacity']),
    allocatable: KubeResourceAmount.optional(json['allocatable']),
    resourcesAvailable: json['resources_available'] == true,
    cpuCores: (json['cpu_cores'] as num).toDouble(),
    memoryBytes: json['memory_bytes'] as int,
    containers: (json['containers'] as List? ?? const [])
        .whereType<KubeJson>()
        .map(KubeContainerMetrics.fromJson)
        .toList(),
  );
}

final class KubeContainerMetrics {
  const KubeContainerMetrics({
    required this.name,
    required this.cpuCores,
    required this.memoryBytes,
    this.requests,
    this.limits,
  });
  final KubeResourceAmount? requests, limits;
  final String name;
  final double cpuCores;
  final int memoryBytes;
  factory KubeContainerMetrics.fromJson(KubeJson json) => KubeContainerMetrics(
    name: json['name'] as String,
    requests: KubeResourceAmount.optional(json['requests']),
    limits: KubeResourceAmount.optional(json['limits']),
    cpuCores: (json['cpu_cores'] as num).toDouble(),
    memoryBytes: json['memory_bytes'] as int,
  );
}

final class KubeResourceAmount {
  const KubeResourceAmount({this.cpuCores, this.memoryBytes});
  final double? cpuCores;
  final int? memoryBytes;
  static KubeResourceAmount? optional(Object? json) => json is KubeJson
      ? KubeResourceAmount(cpuCores: (json['cpu_cores'] as num?)?.toDouble(), memoryBytes: json['memory_bytes'] as int?)
      : null;
}
