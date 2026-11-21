import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/docker/docker_kube_plan.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_manifest_service.dart';

/// Create-only execution. A partial deployment keeps explicit receipts;
/// retries never replace existing resources or delete somebody else's work.
final class DockerKubeDeployment {
  DockerKubeDeployment({required this.service, required this.sessionId, required this.current, required this.kinds});
  final KubeManifestService service;
  final String sessionId;
  final bool Function() current;
  final List<KubeResourceKind> kinds;
  final created = <KubeResourceRef>[];
  List<DockerKubeDocument>? _validated;
  bool _cancelled = false;
  bool get validated => _validated != null && !_cancelled;
  void invalidate() => _validated = null;
  void cancel() {
    _cancelled = true;
    invalidate();
  }

  void _check() {
    if (_cancelled || !current()) throw StateError('Deployment scope expired');
  }

  Future<void> validate(List<DockerKubeDocument> documents) async {
    invalidate();
    if (created.isNotEmpty || documents.length != 2) throw StateError('Already started');
    // Native verifies the complete discovery record, including verbs. Never
    // send the parser's identity-only constants to the native resource API.
    final resolved = documents.map((document) {
      final identity = document.resource;
      final kind = kinds.singleWhere(
        (k) =>
            k.id == identity.api.id &&
            k.kind == identity.api.kind &&
            k.namespaced == identity.api.namespaced &&
            k.verbs.contains('create'),
      );
      return DockerKubeDocument(
        KubeResourceRef(api: kind, name: identity.name, namespace: identity.namespace),
        document.manifest,
      );
    }).toList();
    for (final document in resolved) {
      _check();
      await _create(document, dryRun: true);
      _check();
    }
    _validated = List.unmodifiable(resolved);
  }

  Future<void> deploy({required void Function() onProgress}) async {
    _check();
    final documents = _validated;
    invalidate(); // One explicit attempt per server validation.
    if (documents == null || created.isNotEmpty) throw StateError('Dry-run required');
    for (final document in documents) {
      _check();
      await _create(document, dryRun: false);
      created.add(document.resource);
      if (current()) onProgress();
      _check();
    }
  }

  Future<void> _create(DockerKubeDocument document, {required bool dryRun}) async {
    final manifest = SecretText(document.manifest);
    try {
      await service.createResource(sessionId, document.resource, manifest, dryRun: dryRun);
    } finally {
      manifest.wipe();
    }
  }
}
