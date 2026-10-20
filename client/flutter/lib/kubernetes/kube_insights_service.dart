import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';

abstract interface class KubeInsightsService {
  Future<KubeInsights> insights(String sessionId, {String? namespace});
  Future<List<KubeHelmSummary>> helmHistory(String sessionId, {String? namespace, String? releaseName});
  Future<KubeHelmRelease> helmRelease(String sessionId, String namespace, String secretName);
  Future<void> fluxAction(String sessionId, KubeResourceRef resource, KubeFluxAction action, {String? reason});
  Future<void> argoAction(String sessionId, KubeResourceRef resource, KubeArgoAction action);
}

enum KubeFluxAction { reconcile, reconcileWithSource, suspend, resume, resetFailures, forceUpgrade }

enum KubeArgoAction { refresh, hardRefresh, sync }

final class KubeInsights {
  const KubeInsights({
    required this.issues,
    required this.gitops,
    required this.resources,
    this.partial = false,
    this.flow = const KubeGitOpsFlow(),
  });
  final List<KubeIssue> issues;
  final KubeGitOpsOverview gitops;
  final List<KubeInsightResource> resources;
  final bool partial;
  final KubeGitOpsFlow flow;
  factory KubeInsights.fromJson(KubeJson json) => KubeInsights(
    issues: List.unmodifiable((json['issues'] as List? ?? const []).map((e) => KubeIssue.fromJson(e as KubeJson))),
    gitops: KubeGitOpsOverview.fromJson(json['gitops'] as KubeJson),
    resources: List.unmodifiable(
      (json['resources'] as List? ?? const []).map((e) => KubeInsightResource.fromJson(e as KubeJson)),
    ),
    partial: json['partial'] as bool? ?? true,
    flow: KubeGitOpsFlow.fromJson(json['flow'] as KubeJson? ?? const {}),
  );
}

final class KubeIssue {
  const KubeIssue({
    required this.code,
    required this.severity,
    required this.kind,
    required this.name,
    this.namespace,
    this.subject,
    this.actions = const [],
    this.count = 1,
    this.exitCode,
    this.owner,
  });
  final String code, severity, kind, name;
  final String? namespace, subject;
  final List<String> actions;
  final int count;
  final int? exitCode;
  final KubeObjectIdentity? owner;
  factory KubeIssue.fromJson(KubeJson json) => KubeIssue(
    code: json['code'] as String,
    severity: json['severity'] as String,
    kind: json['kind'] as String,
    name: json['name'] as String,
    namespace: json['namespace'] as String?,
    subject: json['subject'] as String?,
    actions: List<String>.unmodifiable((json['actions'] as List? ?? const []).cast<String>()),
    count: json['count'] as int? ?? 1,
    exitCode: json['exit_code'] as int?,
    owner: json['owner'] is KubeJson ? KubeObjectIdentity.fromJson(json['owner'] as KubeJson) : null,
  );
}

final class KubeInsightResource {
  const KubeInsightResource({required this.api, required this.resource});
  final KubeResourceKind api;
  final KubeResourceRow resource;
  KubeJson get object => resource.object;
  KubeResourceRef get reference => KubeResourceRef(api: api, name: resource.name, namespace: resource.namespace);
  bool get isFlux => api.group.endsWith('.toolkit.fluxcd.io');
  bool get isArgo => api.group == 'argoproj.io' && api.kind == 'Application';
  bool get suspended => (object['spec'] as KubeJson?)?['suspend'] == true;
  factory KubeInsightResource.fromJson(KubeJson json) => KubeInsightResource(
    api: KubeResourceKind.fromJson(json['api'] as KubeJson),
    resource: KubeResourceRow.fromJson(json['resource'] as KubeJson, includeObject: true),
  );
}

final class KubeGitOpsOverview {
  const KubeGitOpsOverview({
    this.ready = 0,
    this.notReady = 0,
    this.suspended = 0,
    this.stalled = 0,
    this.reconciling = 0,
    this.dependencies = const [],
    this.cycles = const [],
  });
  final int ready, notReady, suspended, stalled, reconciling;
  final List<KubeDependency> dependencies;
  final List<KubeObjectIdentity> cycles;
  factory KubeGitOpsOverview.fromJson(KubeJson json) => KubeGitOpsOverview(
    ready: json['ready'] as int? ?? 0,
    notReady: json['not_ready'] as int? ?? 0,
    suspended: json['suspended'] as int? ?? 0,
    stalled: json['stalled'] as int? ?? 0,
    reconciling: json['reconciling'] as int? ?? 0,
    dependencies: List.unmodifiable(
      (json['dependencies'] as List? ?? const []).map((e) => KubeDependency.fromJson(e as KubeJson)),
    ),
    cycles: List.unmodifiable(
      (json['cycles'] as List? ?? const []).map((e) => KubeObjectIdentity.fromJson(e as KubeJson)),
    ),
  );
}

final class KubeObjectIdentity {
  const KubeObjectIdentity({required this.kind, required this.namespace, required this.name});
  final String kind, namespace, name;
  factory KubeObjectIdentity.fromJson(KubeJson json) => KubeObjectIdentity(
    kind: json['kind'] as String,
    namespace: json['namespace'] as String? ?? '',
    name: json['name'] as String,
  );
  String get label => '$kind · $namespace/$name';
}

final class KubeDependency {
  const KubeDependency({required this.from, required this.to, required this.ready, required this.missing});
  final KubeObjectIdentity from, to;
  final bool ready, missing;
  factory KubeDependency.fromJson(KubeJson json) => KubeDependency(
    from: KubeObjectIdentity.fromJson(json['from'] as KubeJson),
    to: KubeObjectIdentity.fromJson(json['to'] as KubeJson),
    ready: json['ready'] as bool,
    missing: json['missing'] as bool,
  );
}

final class KubeHelmSummary {
  const KubeHelmSummary({
    required this.name,
    required this.namespace,
    required this.revision,
    required this.status,
    required this.chart,
    required this.chartVersion,
    required this.appVersion,
    required this.secretName,
    this.updatedAt,
  });
  final String name, namespace, status, chart, chartVersion, appVersion, secretName;
  final int revision;
  final String? updatedAt;
  String get id => '$namespace/$secretName';
  factory KubeHelmSummary.fromJson(KubeJson json) => KubeHelmSummary(
    name: json['name'] as String,
    namespace: json['namespace'] as String,
    revision: json['revision'] as int,
    status: json['status'] as String,
    chart: json['chart'] as String,
    chartVersion: json['chart_version'] as String,
    appVersion: json['app_version'] as String? ?? '',
    secretName: json['secret_name'] as String,
    updatedAt: json['updated_at'] as String?,
  );
}

final class KubeHelmRelease {
  const KubeHelmRelease({
    required this.name,
    required this.namespace,
    required this.revision,
    required this.status,
    required this.chart,
    required this.chartVersion,
    required this.appVersion,
    required this.values,
    required this.chartValues,
    required this.manifest,
    this.updatedAt,
    this.computedValues,
    this.computedValuesPartial = true,
  });
  final String name, namespace, status, chart, chartVersion, appVersion, manifest;
  final int revision;
  final String? updatedAt;
  final Object? values, chartValues, computedValues;
  final bool computedValuesPartial;
  factory KubeHelmRelease.fromJson(KubeJson json) => KubeHelmRelease(
    name: json['name'] as String,
    namespace: json['namespace'] as String,
    revision: json['revision'] as int,
    status: json['status'] as String,
    chart: json['chart'] as String,
    chartVersion: json['chart_version'] as String,
    appVersion: json['app_version'] as String? ?? '',
    updatedAt: json['updated_at'] as String?,
    values: json['values'],
    chartValues: json['chart_values'],
    computedValues: json['computed_values'],
    computedValuesPartial: json['computed_values_partial'] as bool? ?? true,
    manifest: json['manifest'] as String,
  );
}

final class KubeGitOpsFlow {
  const KubeGitOpsFlow({this.nodes = const [], this.edges = const [], this.truncated = false});
  final List<KubeFlowNode> nodes;
  final List<KubeFlowEdge> edges;
  final bool truncated;
  factory KubeGitOpsFlow.fromJson(KubeJson json) => KubeGitOpsFlow(
    nodes: (json['nodes'] as List? ?? const []).cast<KubeJson>().map(KubeFlowNode.fromJson).toList(growable: false),
    edges: (json['edges'] as List? ?? const []).cast<KubeJson>().map(KubeFlowEdge.fromJson).toList(growable: false),
    truncated: json['truncated'] == true,
  );
}

final class KubeFlowNode {
  const KubeFlowNode({required this.object, this.ready});
  final KubeObjectIdentity object;
  final bool? ready;
  factory KubeFlowNode.fromJson(KubeJson json) =>
      KubeFlowNode(object: KubeObjectIdentity.fromJson(json['object'] as KubeJson), ready: json['ready'] as bool?);
}

final class KubeFlowEdge {
  const KubeFlowEdge({required this.from, required this.to, required this.relation});
  final KubeObjectIdentity from, to;
  final String relation;
  factory KubeFlowEdge.fromJson(KubeJson json) => KubeFlowEdge(
    from: KubeObjectIdentity.fromJson(json['from'] as KubeJson),
    to: KubeObjectIdentity.fromJson(json['to'] as KubeJson),
    relation: json['relation'] as String,
  );
}
