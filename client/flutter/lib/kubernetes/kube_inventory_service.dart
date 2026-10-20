import 'package:consolecrypt/kubernetes/kube_service.dart';

abstract interface class KubeInventoryManagementService {
  Future<List<String>> scanConfigFiles(String directory);
  Future<void> exportContext(String contextId, String path, {bool includeCredentials = false});
  Future<void> updateContext(
    String contextId, {
    required String name,
    required String namespace,
    required bool production,
    required bool allowExecInProd,
    required bool allowPortForwardInProd,
    required bool recordEvents,
  });
}

final class KubeContextPreferences {
  const KubeContextPreferences({
    this.allowExecInProd = false,
    this.allowPortForwardInProd = false,
    this.recordEvents = false,
  });
  final bool allowExecInProd, allowPortForwardInProd, recordEvents;
  factory KubeContextPreferences.fromJson(KubeJson json) => KubeContextPreferences(
    allowExecInProd: json['allow_exec_in_prod'] as bool? ?? false,
    allowPortForwardInProd: json['allow_port_forward_in_prod'] as bool? ?? false,
    recordEvents: json['event_history_enabled'] as bool? ?? false,
  );
}
