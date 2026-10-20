import 'package:consolecrypt/kubernetes/kube_service.dart';

abstract interface class KubeHistoryService {
  Future<KubeJson> historyStatus(String sessionId);
  Future<List<KubeJson>> eventHistory(
    String contextId, {
    String? namespace,
    bool warningOnly = false,
    int? beforeMs,
    String? beforeUid,
    int limit = 200,
  });
  Future<void> clearEventHistory(String contextId);
}
