import 'package:consolecrypt/kubernetes/kube_service.dart';

/// Handles are allocated by the native core and expire with the vault session.
/// No kubeconfig, certificate, token or external command crosses this interface.
abstract interface class KubeBrowserService {
  Future<KubeSession> connect(String contextId);
  Future<void> disconnect(String sessionId);
  Future<List<KubeResourceKind>> discover(String sessionId);
  Future<List<String>> namespaces(String sessionId);
  Future<KubeResourcePage> listResources(
    String sessionId,
    KubeResourceKind kind, {
    String? namespace,
    String? labelSelector,
    String? continueToken,
    int limit = 200,
  });
  Future<KubeResourceDetail> getResource(String sessionId, KubeResourceRef resource);
  Future<String> logs(
    String sessionId,
    KubeResourceRef resource, {
    String? container,
    bool previous = false,
    bool timestamps = true,
    int tailLines = 500,
  });
}

/// Streaming is optional until the backend provides a revocable stream handle.
abstract interface class KubeWatchService {
  Stream<KubeWatchEvent> watchResources(
    String sessionId,
    KubeResourceKind kind, {
    String? namespace,
    String? labelSelector,
    required String resourceVersion,
  });
}

abstract interface class KubeLogService {
  Stream<String> followLogs(
    String sessionId,
    KubeResourceRef resource, {
    String? container,
    bool previous = false,
    bool timestamps = true,
    int tailLines = 500,
  });
}

final class KubeSession {
  const KubeSession({
    required this.id,
    required this.contextId,
    required this.serverVersion,
    required this.production,
    this.allowExecInProd = false,
    this.allowPortForwardInProd = false,
  });
  final String id, contextId, serverVersion;
  final bool production;
  final bool allowExecInProd, allowPortForwardInProd;
  bool get canExec => !production || allowExecInProd;
  bool get canPortForward => !production || allowPortForwardInProd;
  factory KubeSession.fromJson(KubeJson json) => KubeSession(
    id: json['id'] as String,
    contextId: json['context_id'] as String,
    serverVersion: json['server_version'] as String? ?? '',
    production: json['production'] as bool? ?? true,
    allowExecInProd: json['allow_exec_in_prod'] as bool? ?? false,
    allowPortForwardInProd: json['allow_port_forward_in_prod'] as bool? ?? false,
  );
}

final class KubeResourceKind {
  const KubeResourceKind({
    required this.group,
    required this.version,
    required this.resource,
    required this.kind,
    required this.namespaced,
    this.verbs = const [],
  });
  final String group, version, resource, kind;
  final bool namespaced;
  final List<String> verbs;
  String get id => '$group/$version/$resource';
  String get apiVersion => group.isEmpty ? version : '$group/$version';
  bool get canList => verbs.contains('list');
  bool get canWatch => verbs.contains('watch');
  bool get canRead => verbs.contains('get');
  bool get isPod => group.isEmpty && resource == 'pods';
  bool get isSecret => group.isEmpty && resource == 'secrets';
  factory KubeResourceKind.fromJson(KubeJson json) => KubeResourceKind(
    group: json['group'] as String? ?? '',
    version: json['version'] as String,
    resource: json['resource'] as String,
    kind: json['kind'] as String,
    namespaced: json['namespaced'] as bool,
    verbs: List<String>.unmodifiable((json['verbs'] as List? ?? const []).cast<String>()),
  );
  KubeJson toJson() => {
    'group': group,
    'version': version,
    'resource': resource,
    'kind': kind,
    'namespaced': namespaced,
    'verbs': verbs,
  };
}

final class KubeResourceRef {
  const KubeResourceRef({required this.api, required this.name, this.namespace});
  final KubeResourceKind api;
  final String name;
  final String? namespace;
  String get id => '${api.id}/${namespace ?? ''}/$name';
  KubeJson toJson() => {'api': api.toJson(), 'namespace': namespace, 'name': name};
  factory KubeResourceRef.fromJson(KubeJson json) => KubeResourceRef(
    api: KubeResourceKind.fromJson(json['api'] as KubeJson),
    name: json['name'] as String,
    namespace: json['namespace'] as String?,
  );
}

final class KubeResourceRow {
  const KubeResourceRow({
    required this.name,
    required this.uid,
    required this.resourceVersion,
    this.namespace,
    this.createdAt,
    this.status = '',
    this.cells = const [],
    this.object = const {},
  });
  final String name, uid, resourceVersion, status;
  final String? namespace;
  final DateTime? createdAt;
  final List<String> cells;
  final KubeJson object;
  String get id => uid.isNotEmpty ? uid : '${namespace ?? ''}/$name';
  factory KubeResourceRow.fromJson(KubeJson json, {bool includeObject = false}) => KubeResourceRow(
    name: json['name'] as String,
    namespace: json['namespace'] as String?,
    uid: json['uid'] as String? ?? '',
    resourceVersion: json['resource_version'] as String? ?? '',
    status: json['status'] as String? ?? '',
    createdAt: DateTime.tryParse(json['created_at'] as String? ?? ''),
    cells: List<String>.unmodifiable((json['cells'] as List? ?? const []).map((e) => e?.toString() ?? '')),
    // Tables and watch caches need only summary fields. Retain full redacted
    // objects only for the separately bounded diagnostic response.
    object: includeObject ? Map<String, dynamic>.unmodifiable(json['object'] as KubeJson? ?? const {}) : const {},
  );
}

final class KubeResourcePage {
  const KubeResourcePage({required this.items, required this.resourceVersion, this.continueToken});
  final List<KubeResourceRow> items;
  final String resourceVersion;
  final String? continueToken;
  factory KubeResourcePage.fromJson(KubeJson json) => KubeResourcePage(
    items: List.unmodifiable((json['items'] as List).map((e) => KubeResourceRow.fromJson(e as KubeJson))),
    resourceVersion: json['resource_version'] as String? ?? '',
    continueToken: json['continue_token'] as String?,
  );
}

final class KubeResourceDetail {
  const KubeResourceDetail({required this.object, required this.yaml, this.containers = const []});

  /// The native core removes Secret values, managedFields and credential fields.
  final KubeJson object;
  final String yaml;
  final List<String> containers;
  factory KubeResourceDetail.fromJson(KubeJson json) => KubeResourceDetail(
    object: Map<String, dynamic>.unmodifiable(json['object'] as KubeJson),
    yaml: json['yaml'] as String,
    containers: List<String>.unmodifiable((json['containers'] as List? ?? const []).cast<String>()),
  );
}

enum KubeWatchEventType { snapshotBegin, snapshotPage, added, modified, deleted, bookmark, reconnecting, relist }

final class KubeWatchEvent {
  const KubeWatchEvent({required this.type, this.resource, this.page, this.resourceVersion = ''});
  final KubeWatchEventType type;
  final KubeResourceRow? resource;
  final KubeResourcePage? page;
  final String resourceVersion;
  factory KubeWatchEvent.fromJson(KubeJson json) => KubeWatchEvent(
    type: switch (json['type']) {
      'snapshot_begin' => KubeWatchEventType.snapshotBegin,
      'snapshot_page' => KubeWatchEventType.snapshotPage,
      'added' => KubeWatchEventType.added,
      'modified' => KubeWatchEventType.modified,
      'deleted' => KubeWatchEventType.deleted,
      'bookmark' => KubeWatchEventType.bookmark,
      'reconnecting' => KubeWatchEventType.reconnecting,
      _ => KubeWatchEventType.relist,
    },
    resource: json['resource'] == null ? null : KubeResourceRow.fromJson(json['resource'] as KubeJson),
    page: json['page'] == null ? null : KubeResourcePage.fromJson(json['page'] as KubeJson),
    resourceVersion: json['resource_version'] as String? ?? '',
  );
}
