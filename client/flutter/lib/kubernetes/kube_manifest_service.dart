import 'package:consolecrypt/core/security/secret_text.dart';

import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';

abstract interface class KubeManifestService {
  Future<KubeJson> resourceSchema(String sessionId, KubeResourceKind kind);
  Future<KubeJson> compareManifest(String sessionId, KubeResourceRef resource, SecretText manifest);
  Future<KubeResourceDetail> createResource(
    String sessionId,
    KubeResourceRef resource,
    SecretText manifest, {
    bool dryRun = true,
  });

  /// An explicit reveal only. The owner must wipe on close, background, lock,
  /// or after 30 seconds; it must not put this value into resource caches.
  Future<SecretText> revealSecret(String sessionId, KubeResourceRef resource);
}
