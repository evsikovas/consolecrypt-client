import 'package:consolecrypt/kubernetes/kube_flux_cli_service.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';

abstract interface class KubeHelmCliService {
  Future<HelmRollbackReview> reviewHelmRollback(
    String sessionId,
    String namespace,
    String release,
    int revision, {
    String? manualPath,
  });
  Future<FluxCliOutput> approveHelmRollback(String ticket);
  Future<void> cancelHelmRollback(String ticket);
}

final class HelmRollbackReview {
  const HelmRollbackReview({
    required this.ticket,
    required this.path,
    required this.sha256,
    required this.namespace,
    required this.release,
    required this.revision,
    required this.currentRevision,
    required this.arguments,
    required this.expiresAt,
  });
  final String ticket, path, sha256, namespace, release;
  final int revision, currentRevision;
  final List<String> arguments;
  final DateTime expiresAt;
  factory HelmRollbackReview.fromJson(KubeJson json) => HelmRollbackReview(
    ticket: json['ticket'] as String,
    path: json['path'] as String,
    sha256: json['sha256'] as String,
    namespace: json['namespace'] as String,
    release: json['release'] as String,
    revision: json['revision'] as int,
    currentRevision: json['current_revision'] as int,
    arguments: (json['arguments'] as List).cast<String>(),
    expiresAt: DateTime.fromMillisecondsSinceEpoch(json['expires_at_ms'] as int, isUtc: true),
  );
}
