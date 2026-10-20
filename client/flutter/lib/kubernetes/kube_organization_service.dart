abstract interface class KubeOrganizationService {
  Future<void> saveKubeFavourites(String contextId, {required List<String> namespaces, required List<String> kinds});

  /// Labels belong to the shared cluster, including its other saved contexts.
  Future<void> saveKubeClusterLabels(String contextId, {required List<String> tags, String? color});
}
