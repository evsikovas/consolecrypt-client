import 'package:consolecrypt/core/security/secret_text.dart';

typedef KubeJson = Map<String, dynamic>;

final class KubeConnection {
  const KubeConnection({
    required this.id,
    required this.name,
    required this.clusterName,
    required this.server,
    required this.namespace,
    required this.authentication,
    required this.production,
    this.allowExecInProd = false,
    this.allowPortForwardInProd = false,
    this.recordEvents = false,
    this.tags = const [],
    this.color,
    this.favouriteNamespaces = const [],
    this.favouriteKinds = const [],
  });
  final String id, name, clusterName, server, namespace, authentication;
  final bool production, allowExecInProd, allowPortForwardInProd, recordEvents;
  final List<String> tags, favouriteNamespaces, favouriteKinds;
  final String? color;

  factory KubeConnection.fromJson(KubeJson context, KubeJson? cluster) => KubeConnection(
    id: context['id'] as String,
    name: context['name'] as String,
    clusterName: cluster?['name'] as String? ?? '',
    server: cluster?['server'] as String? ?? '',
    namespace: context['namespace'] as String? ?? 'default',
    authentication: (context['auth'] as KubeJson?)?['kind'] as String? ?? 'none',
    production: context['production'] as bool? ?? true,
    allowExecInProd: context['allow_exec_in_prod'] as bool? ?? false,
    allowPortForwardInProd: context['allow_port_forward_in_prod'] as bool? ?? false,
    recordEvents: context['event_history_enabled'] as bool? ?? false,
    tags: List.unmodifiable((cluster?['tags'] as List? ?? const []).cast<String>()),
    color: cluster?['color'] as String?,
    favouriteNamespaces: List.unmodifiable((context['favourite_namespaces'] as List? ?? const []).cast<String>()),
    favouriteKinds: List.unmodifiable((context['favourite_kinds'] as List? ?? const []).cast<String>()),
  );
}

final class KubeImportEntry {
  const KubeImportEntry({
    required this.index,
    required this.name,
    this.server,
    this.namespace,
    this.userName,
    this.authentication,
    this.pendingFiles = const [],
    this.requiresRoute = false,
    this.insecureTls = false,
    this.error,
  });
  final int index;
  final String name;
  final String? server, namespace, userName, authentication, error;
  final List<String> pendingFiles;
  final bool requiresRoute, insecureTls;

  factory KubeImportEntry.fromJson(KubeJson json) => KubeImportEntry(
    index: json['index'] as int,
    name: json['name'] as String,
    server: json['server'] as String?,
    namespace: json['namespace'] as String?,
    userName: json['user_name'] as String?,
    authentication: json['authentication'] as String?,
    pendingFiles: List<String>.unmodifiable((json['pending_files'] as List).cast<String>()),
    requiresRoute: json['requires_route'] as bool,
    insecureTls: json['insecure_tls'] as bool,
    error: json['error'] as String?,
  );
}

final class KubeImportPreview {
  const KubeImportPreview({required this.ticket, required this.entries, required this.expiresAt, this.currentContext});
  final String ticket;
  final List<KubeImportEntry> entries;
  final DateTime expiresAt;
  final String? currentContext;

  factory KubeImportPreview.fromJson(KubeJson json) => KubeImportPreview(
    ticket: json['ticket'] as String,
    entries: List.unmodifiable((json['entries'] as List).map((e) => KubeImportEntry.fromJson(e as KubeJson))),
    expiresAt: DateTime.fromMillisecondsSinceEpoch(json['expires_at_ms'] as int, isUtc: true),
    currentContext: json['current_context'] as String?,
  );
}

final class KubeImportChoice {
  const KubeImportChoice({required this.index, this.production = true, this.approvedFiles = const [], this.sshHostId});
  final int index;
  final bool production;
  final List<String> approvedFiles;
  final String? sshHostId;
  KubeJson toJson() => {
    'index': index,
    'production': production,
    'approved_files': approvedFiles,
    'route': sshHostId == null ? {'kind': 'direct'} : {'kind': 'ssh', 'host_id': sshHostId},
  };
}

abstract interface class KubeService {
  Stream<void> changes();
  Future<List<KubeConnection>> inventory();
  Future<KubeImportPreview> preview(SecretText yaml, {String? sourceFile});
  Future<KubeImportPreview> previewFile(String path);
  Future<void> discard(String ticket);
  Future<void> commit(String ticket, List<KubeImportChoice> choices);
  Future<void> deleteContext(String id);
}

final class UnavailableKubeService implements KubeService {
  const UnavailableKubeService();
  @override
  Stream<void> changes() => const Stream.empty();
  @override
  Future<List<KubeConnection>> inventory() async => const [];
  Never _unavailable() => throw StateError('Kubernetes service unavailable'); // l10n-ignore: diagnostic
  @override
  Future<KubeImportPreview> preview(SecretText yaml, {String? sourceFile}) async => _unavailable();
  @override
  Future<KubeImportPreview> previewFile(String path) async => _unavailable();
  @override
  Future<void> discard(String ticket) async {}
  @override
  Future<void> commit(String ticket, List<KubeImportChoice> choices) async => _unavailable();
  @override
  Future<void> deleteContext(String id) async => _unavailable();
}
