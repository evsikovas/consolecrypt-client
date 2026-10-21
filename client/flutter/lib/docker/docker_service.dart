/// Docker on this computer or on a saved SSH host: the Engine API is reached
/// through the local socket or the host's `/var/run/docker.sock` over SSH.
/// The native core does all networking; this file is the UI contract.
library;

typedef DockerJson = Map<String, dynamic>;

enum DockerHostKind { local, ssh }

/// Where containers are listed from.
final class DockerHost {
  const DockerHost({required this.id, required this.name, required this.kind, this.address = ''});

  /// `local` for this computer, otherwise the saved SSH host id.
  final String id;
  final String name, address;
  final DockerHostKind kind;
  static const localId = 'local'; // l10n-ignore: wire id
  factory DockerHost.fromJson(DockerJson json) => DockerHost(
    id: json['id'] as String,
    name: json['name'] as String,
    kind: json['kind'] == 'ssh' ? DockerHostKind.ssh : DockerHostKind.local,
    address: json['address'] as String? ?? '',
  );
}

/// An open connection to one Docker engine.
final class DockerSessionInfo {
  const DockerSessionInfo({
    required this.id,
    required this.hostId,
    required this.hostName,
    required this.engineVersion,
    this.os = '',
    this.arch = '',
    this.containersRunning = 0,
    this.containersTotal = 0,
    this.images = 0,
  });
  final String id, hostId, hostName, engineVersion, os, arch;
  final int containersRunning, containersTotal, images;
  factory DockerSessionInfo.fromJson(DockerJson json) => DockerSessionInfo(
    id: json['id'] as String,
    hostId: json['host_id'] as String,
    hostName: json['host_name'] as String? ?? '',
    engineVersion: json['engine_version'] as String? ?? '',
    os: json['os'] as String? ?? '',
    arch: json['arch'] as String? ?? '',
    containersRunning: json['containers_running'] as int? ?? 0,
    containersTotal: json['containers_total'] as int? ?? 0,
    images: json['images'] as int? ?? 0,
  );
}

/// Why connecting failed, so the UI can say what to do.
enum DockerFailure {
  /// The SSH user cannot open the Docker socket (not in the `docker` group).
  permissionDenied,

  /// No Docker socket/daemon on the host.
  notRunning,

  /// SSH or local connection failed.
  connectionFailed,
  other,
}

final class DockerPort {
  const DockerPort({required this.privatePort, required this.protocol, this.publicPort, this.ip});
  final int privatePort;
  final int? publicPort;
  final String protocol;
  final String? ip;
  factory DockerPort.fromJson(DockerJson json) => DockerPort(
    privatePort: json['private_port'] as int,
    publicPort: json['public_port'] as int?,
    protocol: json['protocol'] as String? ?? 'tcp',
    ip: json['ip'] as String?,
  );
}

enum DockerContainerState { created, running, paused, restarting, removing, exited, dead, unknown }

final class DockerContainer {
  const DockerContainer({
    required this.id,
    required this.name,
    required this.image,
    required this.state,
    this.status = '',
    this.createdAt,
    this.ports = const [],
    this.composeProject,
    this.composeService,
    this.networks = const {},
    this.health,
  });

  /// Full container id; the UI shows the first 12 characters.
  final String id, name, image, status;
  final DockerContainerState state;
  final DateTime? createdAt;
  final List<DockerPort> ports;

  /// `com.docker.compose.project` / `.service` labels.
  final String? composeProject, composeService;

  /// Network name → container IP.
  final Map<String, String> networks;

  /// `healthy`, `unhealthy`, `starting` or null.
  final String? health;
  String get shortId => id.length > 12 ? id.substring(0, 12) : id;
  factory DockerContainer.fromJson(DockerJson json) => DockerContainer(
    id: json['id'] as String,
    name: json['name'] as String,
    image: json['image'] as String? ?? '',
    state: DockerContainerState.values.firstWhere(
      (state) => state.name == json['state'],
      orElse: () => DockerContainerState.unknown,
    ),
    status: json['status'] as String? ?? '',
    createdAt: json['created'] == null
        ? null
        : DateTime.fromMillisecondsSinceEpoch((json['created'] as int) * 1000, isUtc: true),
    ports: [for (final port in (json['ports'] as List? ?? const [])) DockerPort.fromJson(port as DockerJson)],
    composeProject: json['compose_project'] as String?,
    composeService: json['compose_service'] as String?,
    networks: (json['networks'] as Map? ?? const {}).cast<String, String>(),
    health: json['health'] as String?,
  );
}

/// `docker inspect` of one container (environment values of keys that look
/// secret are masked natively).
final class DockerContainerDetail {
  const DockerContainerDetail({required this.container, required this.inspect});
  final DockerContainer container;
  final DockerJson inspect;
}

enum DockerAction { start, stop, restart, pause, unpause, kill, remove }

enum DockerLogStream { stdout, stderr }

final class DockerLogLine {
  const DockerLogLine({required this.stream, required this.text, this.timestamp});
  final DockerLogStream stream;
  final String text;
  final DateTime? timestamp;
  factory DockerLogLine.fromJson(DockerJson json) => DockerLogLine(
    stream: json['stream'] == 'stderr' ? DockerLogStream.stderr : DockerLogStream.stdout,
    text: json['text'] as String,
    timestamp: json['timestamp'] == null ? null : DateTime.tryParse(json['timestamp'] as String),
  );
}

enum DockerForwardState { running, failed, stopped }

/// A local port forwarded to a container port through the SSH host (or
/// directly on this computer).
final class DockerForward {
  const DockerForward({
    required this.id,
    required this.sessionId,
    required this.containerName,
    required this.remotePort,
    required this.address,
    required this.state,
    this.activeConnections = 0,
    this.bytesSent = 0,
    this.bytesReceived = 0,
    this.errorCode,
  });
  final String id, sessionId, containerName, address;
  final int remotePort, activeConnections, bytesSent, bytesReceived;
  final DockerForwardState state;
  final String? errorCode;
  factory DockerForward.fromJson(DockerJson json) => DockerForward(
    id: json['id'] as String,
    sessionId: json['session_id'] as String,
    containerName: json['container_name'] as String,
    remotePort: json['remote_port'] as int,
    address: json['address'] as String,
    state: DockerForwardState.values.firstWhere(
      (state) => state.name == json['state'],
      orElse: () => DockerForwardState.failed,
    ),
    activeConnections: json['active_connections'] as int? ?? 0,
    bytesSent: json['bytes_sent'] as int? ?? 0,
    bytesReceived: json['bytes_received'] as int? ?? 0,
    errorCode: json['error_code'] as String?,
  );
}

final class DockerException implements Exception {
  const DockerException(this.failure, [this.detail = '']);
  final DockerFailure failure;

  /// For permission errors: the socket owner group (e.g. `docker`).
  final String detail;
  @override
  String toString() => 'DockerException(${failure.name})';
}

abstract interface class DockerService {
  /// This computer (when a local socket exists) followed by saved SSH hosts.
  Future<List<DockerHost>> hosts();

  /// Throws [DockerException] with the reason when the engine is unreachable.
  Future<DockerSessionInfo> connect(String hostId);
  Future<void> disconnect(String sessionId);

  Future<List<DockerContainer>> containers(String sessionId, {bool all = true});
  Future<DockerContainerDetail> inspect(String sessionId, String containerId);
  Future<void> action(String sessionId, String containerId, DockerAction action);

  /// Last [tail] lines, then new lines while [follow] (cancel the
  /// subscription to stop).
  Stream<List<DockerLogLine>> logs(
    String sessionId,
    String containerId, {
    int tail = 500,
    bool follow = true,
    bool timestamps = false,
  });

  Future<DockerForward> startForward(
    String sessionId,
    String containerId, {
    required int remotePort,
    int localPort = 0,
  });
  Future<List<DockerForward>> forwards();
  Future<void> stopForward(String forwardId);
}
