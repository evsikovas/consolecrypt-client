import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';

abstract interface class KubeRolloutService {
  Future<List<KubeJson>> rolloutHistory(String sessionId, KubeResourceRef resource);
  Future<KubeJson> rolloutDiff(String sessionId, KubeResourceRef resource, int before, int after);
}
