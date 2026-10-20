import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';

abstract interface class KubeForwardTargetsService {
  Future<KubeForwardTargets> forwardTargets(String sessionId, KubeResourceRef resource);
}

final class KubeForwardTargets {
  const KubeForwardTargets({required this.items, this.partial = false});
  final List<KubeForwardTarget> items;
  final bool partial;
  factory KubeForwardTargets.fromJson(KubeJson json) => KubeForwardTargets(
    items: List.unmodifiable((json['items'] as List).map((value) => KubeForwardTarget.fromJson(value as KubeJson))),
    partial: json['partial'] as bool? ?? false,
  );
}

final class KubeForwardTarget {
  const KubeForwardTarget({required this.pod, required this.uid, required this.ready, required this.ports});
  final KubeResourceRef pod;
  final String uid;
  final bool ready;
  final List<KubeForwardPort> ports;
  factory KubeForwardTarget.fromJson(KubeJson json) => KubeForwardTarget(
    pod: KubeResourceRef.fromJson(json['pod'] as KubeJson),
    uid: json['uid'] as String,
    ready: json['ready'] as bool,
    ports: List.unmodifiable((json['ports'] as List).map((value) => KubeForwardPort.fromJson(value as KubeJson))),
  );
}

final class KubeForwardPort {
  const KubeForwardPort({required this.port, this.name});
  final String? name;
  final int port;
  factory KubeForwardPort.fromJson(KubeJson json) =>
      KubeForwardPort(port: json['port'] as int, name: json['name'] as String?);
}
