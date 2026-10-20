import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';

abstract interface class KubeRelationsService {
  Future<KubeJson> relatedObjects(String sessionId, KubeResourceRef resource);
}
