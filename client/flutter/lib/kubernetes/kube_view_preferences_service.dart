import 'package:consolecrypt/kubernetes/kube_service.dart';

abstract interface class KubeViewPreferencesService {
  Future<KubeJson> readKubeViewPreferences(String contextId);
  Future<void> updateKubeViewPreferences(String contextId, {required KubeJson patch});
}
