import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';

abstract interface class KubeHelmComparisonService {
  Future<KubeHelmComparison> compareHelmLive(String sessionId, String namespace, String secretName);
}

final class KubeHelmComparison {
  const KubeHelmComparison({required this.revision, required this.items, required this.partial, required this.masked});
  final int revision;
  final List<KubeHelmObjectComparison> items;
  final bool partial, masked;
  factory KubeHelmComparison.fromJson(KubeJson json) => KubeHelmComparison(
    revision: json['revision'] as int,
    items: (json['items'] as List).whereType<KubeJson>().map(KubeHelmObjectComparison.fromJson).toList(),
    partial: json['partial'] as bool? ?? true,
    masked: json['masked_fields'] as bool? ?? true,
  );
}

final class KubeHelmObjectComparison {
  const KubeHelmObjectComparison({required this.reference, required this.state, required this.diff});
  final KubeResourceRef reference;
  final String state;
  final KubeJson diff;
  factory KubeHelmObjectComparison.fromJson(KubeJson json) => KubeHelmObjectComparison(
    reference: KubeResourceRef.fromJson(json['reference'] as KubeJson),
    state: json['state'] as String,
    diff: json['diff'] as KubeJson,
  );
}
