import 'package:consolecrypt/kubernetes/kube_service.dart';

abstract interface class KubeNetworkForwardService {
  Future<KubeForward> startNetworkForward(
    String sessionId, {
    required String namespace,
    required String pod,
    required int remotePort,
    required String localAddress,
    required bool approveNetwork,
    int localPort = 0,
  });
}

abstract interface class KubeForwardService {
  Future<KubeForward> startForward(
    String sessionId, {
    required String namespace,
    required String pod,
    required int remotePort,
    int localPort = 0,
  });
  Future<List<KubeForward>> forwards();
  Future<void> stopForward(String sessionId, String forwardId);
}

final class KubeForward {
  const KubeForward({
    required this.id,
    required this.sessionId,
    required this.contextName,
    required this.namespace,
    required this.pod,
    required this.remotePort,
    required this.address,
    required this.state,
    this.errorCode,
    this.activeConnections = 0,
    this.bytesSent = 0,
    this.bytesReceived = 0,
    this.failedConnections = 0,
  });
  final String id, sessionId, contextName, namespace, pod, address, state;
  final String? errorCode;
  final int remotePort, activeConnections, bytesSent, bytesReceived, failedConnections;
  factory KubeForward.fromJson(KubeJson json) => KubeForward(
    id: json['id'] as String,
    sessionId: json['session_id'] as String,
    contextName: json['context_name'] as String,
    namespace: json['namespace'] as String,
    pod: json['pod'] as String,
    remotePort: json['remote_port'] as int,
    address: json['address'] as String,
    state: json['state'] as String,
    errorCode: json['error_code'] as String?,
    activeConnections: json['active_connections'] as int? ?? 0,
    bytesSent: json['bytes_sent'] as int? ?? 0,
    bytesReceived: json['bytes_received'] as int? ?? 0,
    failedConnections: json['failed_connections'] as int? ?? 0,
  );
}
