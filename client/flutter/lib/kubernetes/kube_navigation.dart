import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// In-memory navigation only: requests cannot cross a lock or profile change.
final class KubeNavigationRequest {
  const KubeNavigationRequest({
    required this.scope,
    required this.contextId,
    this.namespace,
    this.resource,
    this.openAi = false,
    this.gitOpsAction,
  });
  final KubeScope scope;
  final String contextId;
  final String? namespace;
  final KubeResourceRef? resource;
  final bool openAi;

  /// Opens a GitOps view or a reviewed action; never grants write permission.
  final String? gitOpsAction;
}

final kubeNavigationProvider = NotifierProvider<KubeNavigation, KubeNavigationRequest?>(KubeNavigation.new);

final class KubeNavigation extends Notifier<KubeNavigationRequest?> {
  @override
  KubeNavigationRequest? build() {
    ref.watch(kubeScopeProvider);
    return null;
  }

  void open(KubeNavigationRequest request) {
    if (!ref.mounted) return;
    final scope = ref.read(kubeScopeProvider);
    if (!scope.unlocked || !identical(scope, request.scope)) return;
    state = request;
  }

  void consume(KubeNavigationRequest request) {
    if (!ref.mounted) return;
    if (identical(state, request)) state = null;
  }
}

/// Searchable metadata from the visible browser. The palette never opens a
/// cluster connection or fetches resource bodies just to display suggestions.
final class KubeNavigationSnapshot {
  const KubeNavigationSnapshot({
    required this.scope,
    required this.owner,
    required this.contextId,
    required this.namespace,
    this.namespaces = const [],
    this.resources = const [],
    this.hasFlux = false,
  });
  final KubeScope scope;
  final String owner, contextId, namespace;
  final List<String> namespaces;
  final List<KubeResourceRef> resources;
  final bool hasFlux;
}

final kubeActiveNavigationProvider = NotifierProvider<KubeActiveNavigation, KubeNavigationSnapshot?>(
  KubeActiveNavigation.new,
);

final class KubeActiveNavigation extends Notifier<KubeNavigationSnapshot?> {
  @override
  KubeNavigationSnapshot? build() {
    ref.watch(kubeScopeProvider);
    return null;
  }

  void publish(KubeNavigationSnapshot snapshot) {
    if (!ref.mounted) return;
    final scope = ref.read(kubeScopeProvider);
    if (!scope.unlocked || !identical(scope, snapshot.scope)) return;
    state = KubeNavigationSnapshot(
      scope: scope,
      owner: snapshot.owner,
      contextId: snapshot.contextId,
      namespace: snapshot.namespace,
      namespaces: List.unmodifiable(snapshot.namespaces.take(500)),
      resources: List.unmodifiable(snapshot.resources.take(500)),
      hasFlux: snapshot.hasFlux,
    );
  }

  void clear(String owner) {
    if (!ref.mounted) return;
    if (state?.owner == owner) state = null;
  }
}
