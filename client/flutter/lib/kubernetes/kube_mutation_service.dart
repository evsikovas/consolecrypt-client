import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';

/// Mutations are validated again by the native ProdGuard immediately before
/// the request is sent. UI state never grants permission by itself.
abstract interface class KubeMutationService {
  Future<KubeWriteAccess> writeAccess(String sessionId);
  Future<KubeWriteAccess> requestWriteWindow(String sessionId, String typedContextName);
  Future<void> closeWriteWindow(String sessionId);
  Future<KubeResourceDetail> applyYaml(
    String sessionId,
    KubeResourceRef resource,
    SecretText manifest, {
    bool dryRun = true,
    bool force = false,
  });
  Future<void> deleteResource(
    String sessionId,
    KubeResourceRef resource, {
    required String uid,
    required String resourceVersion,
  });
}

final class KubeWriteAccess {
  const KubeWriteAccess({required this.production, this.canRequestWindow = false, this.expiresAt});
  final bool production, canRequestWindow;
  final DateTime? expiresAt;
  bool get canWrite => !production || (expiresAt?.isAfter(DateTime.now().toUtc()) ?? false);
  factory KubeWriteAccess.fromJson(KubeJson json) => KubeWriteAccess(
    production: json['production'] as bool? ?? true,
    canRequestWindow: json['can_request_window'] as bool? ?? false,
    expiresAt: json['expires_at_ms'] == null
        ? null
        : DateTime.fromMillisecondsSinceEpoch(json['expires_at_ms'] as int, isUtc: true),
  );
}
