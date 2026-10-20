import 'dart:typed_data';

import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';

abstract interface class KubeAdvancedLogService {
  Future<KubeLogTargets> logTargets(String sessionId, KubeResourceRef resource);
  Future<String> logsSince(
    String sessionId,
    KubeResourceRef resource, {
    String? container,
    bool previous = false,
    bool timestamps = true,
    int tailLines = 1000,
    int? sinceSeconds,
  });
  Stream<String> followLogsSince(
    String sessionId,
    KubeResourceRef resource, {
    String? container,
    bool previous = false,
    bool timestamps = true,
    int tailLines = 1000,
    int? sinceSeconds,
  });
  Future<void> exportLogs(String sessionId, String path, Uint8List contents, {required bool mask});
}

final class KubeLogTargets {
  const KubeLogTargets({required this.targets, this.truncated = false});
  final List<KubeLogTarget> targets;
  final bool truncated;
  factory KubeLogTargets.fromJson(KubeJson json) => KubeLogTargets(
    truncated: json['truncated'] as bool? ?? false,
    targets: (json['targets'] as List)
        .cast<KubeJson>()
        .map(
          (target) =>
              KubeLogTarget(pod: target['pod'] as String, containers: (target['containers'] as List).cast<String>()),
        )
        .toList(),
  );
}

final class KubeLogTarget {
  const KubeLogTarget({required this.pod, required this.containers});
  final String pod;
  final List<String> containers;
}
