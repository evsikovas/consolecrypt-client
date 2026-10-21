/// Docker on this computer or on a saved SSH host: the Engine API is reached
/// through the local socket or the host's `/var/run/docker.sock` over SSH.
/// The native core does all networking; this file is the UI contract.
library;

import 'package:consolecrypt/core/security/secret_text.dart';

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

/// A named volume and the containers that mount it.
final class DockerVolume {
  const DockerVolume({
    required this.name,
    this.driver = '',
    this.mountpoint = '',
    this.composeProject,
    this.usedBy = const [],
    this.createdAt,
  });
  final String name, driver, mountpoint;
  final String? composeProject;
  final List<String> usedBy;
  final DateTime? createdAt;
  factory DockerVolume.fromJson(DockerJson json) => DockerVolume(
    name: json['name'] as String,
    driver: json['driver'] as String? ?? '',
    mountpoint: json['mountpoint'] as String? ?? '',
    composeProject: json['compose_project'] as String?,
    usedBy: (json['used_by'] as List? ?? const []).cast<String>(),
    createdAt: json['created_at'] == null ? null : DateTime.tryParse(json['created_at'] as String),
  );
}

/// Why a compose file could not be shown.
enum DockerComposeFileError { notFound, permissionDenied, tooLarge, notAFile, notText, readFailed }

/// One compose file as read on the engine host. [content] is secret (compose
/// files often hold passwords); the dialog wipes it on close.
final class DockerComposeFile {
  DockerComposeFile({required this.path, this.content, this.digest, this.size = 0, this.error});
  final String path;
  final SecretText? content;

  /// SHA-256 of what was read; a save refuses when the file changed since.
  final String? digest;
  final int size;
  final DockerComposeFileError? error;
  String get name => path.split('/').last;
  factory DockerComposeFile.fromJson(DockerJson json) => DockerComposeFile(
    path: json['path'] as String,
    content: json['content'] == null ? null : SecretText(json['content'] as String),
    digest: json['digest'] as String?,
    size: json['size'] as int? ?? 0,
    error: switch (json['error']) {
      null => null,
      'not_found' => DockerComposeFileError.notFound,
      'permission_denied' => DockerComposeFileError.permissionDenied,
      'too_large' => DockerComposeFileError.tooLarge,
      'not_a_file' => DockerComposeFileError.notAFile,
      'not_text' => DockerComposeFileError.notText,
      _ => DockerComposeFileError.readFailed,
    },
  );
  void wipe() => content?.wipe();
}

/// A compose project's files, located from its containers' labels.
final class DockerComposeProject {
  const DockerComposeProject({
    required this.project,
    required this.workingDir,
    required this.files,
    this.envFiles = const [],
  });
  final String project, workingDir;
  final List<String> envFiles;
  final List<DockerComposeFile> files;
  factory DockerComposeProject.fromJson(DockerJson json) => DockerComposeProject(
    project: json['project'] as String,
    workingDir: json['working_dir'] as String? ?? '',
    envFiles: (json['env_files'] as List? ?? const []).cast<String>(),
    files: [for (final file in (json['files'] as List).cast<DockerJson>()) DockerComposeFile.fromJson(file)],
  );
  void wipe() {
    for (final file in files) {
      file.wipe();
    }
  }
}

/// A changed file: the new text and the digest of the text it was edited
/// from. The service wipes [content] once sent.
final class DockerComposeEdit {
  const DockerComposeEdit({required this.path, required this.content, required this.digest});
  final String path, digest;
  final SecretText content;
}

/// Why writing failed.
enum DockerComposeWriteError { permissionDenied, notFound, other }

sealed class DockerComposeSaveResult {
  const DockerComposeSaveResult();
}

/// Written; the originals were kept as [backups]. [jobId] streams `up -d`.
final class DockerComposeSaved extends DockerComposeSaveResult {
  const DockerComposeSaved({required this.backups, this.jobId});
  final List<String> backups;
  final String? jobId;
}

/// The configuration check failed; nothing was written.
final class DockerComposeInvalid extends DockerComposeSaveResult {
  const DockerComposeInvalid(this.output);
  final String output;
}

/// The file changed on the host since it was opened.
final class DockerComposeConflict extends DockerComposeSaveResult {
  const DockerComposeConflict(this.path);
  final String path;
}

final class DockerComposeWriteFailed extends DockerComposeSaveResult {
  const DockerComposeWriteFailed(this.path, this.reason);
  final String path;
  final DockerComposeWriteError reason;
}

/// No compose command line tool on the host.
final class DockerComposeMissing extends DockerComposeSaveResult {
  const DockerComposeMissing();
}

/// The check could not run (timeout, refused channel).
final class DockerComposeRunFailed extends DockerComposeSaveResult {
  const DockerComposeRunFailed(this.detail);
  final String detail;
}

/// The edit permission expired: ask for the passphrase again.
final class DockerComposeLocked extends DockerComposeSaveResult {
  const DockerComposeLocked();
}

DockerComposeSaveResult dockerComposeSaveResultFromJson(DockerJson json) => switch (json['outcome']) {
  'saved' => DockerComposeSaved(
    backups: (json['backups'] as List? ?? const []).cast<String>(),
    jobId: json['job_id'] as String?,
  ),
  'invalid' => DockerComposeInvalid(json['output'] as String? ?? ''),
  'conflict' => DockerComposeConflict(json['path'] as String? ?? ''),
  'write_failed' => DockerComposeWriteFailed(json['path'] as String? ?? '', switch (json['reason']) {
    'permission_denied' => DockerComposeWriteError.permissionDenied,
    'not_found' => DockerComposeWriteError.notFound,
    _ => DockerComposeWriteError.other,
  }),
  'compose_missing' => const DockerComposeMissing(),
  'locked' => const DockerComposeLocked(),
  _ => DockerComposeRunFailed(json['detail'] as String? ?? ''),
};

/// Output of `up -d` so far; [done] once it ended ([success] then says how).
final class DockerComposeJobUpdate {
  const DockerComposeJobUpdate({required this.lines, required this.done, this.exitCode, this.error});
  final List<String> lines;
  final bool done;
  final int? exitCode;

  /// `timeout`, `cancelled`, `compose_missing` or `run_failed`.
  final String? error;
  bool get success => done && error == null && exitCode == 0;
  factory DockerComposeJobUpdate.fromJson(DockerJson json) => DockerComposeJobUpdate(
    lines: (json['lines'] as List? ?? const []).cast<String>(),
    done: json['done'] as bool? ?? false,
    exitCode: json['exit_code'] as int?,
    error: json['error'] as String?,
  );
}

/// Why a project's files cannot be located.
enum DockerComposeFailure { projectGone, filesUnknown, other }

final class DockerComposeException implements Exception {
  const DockerComposeException(this.failure);
  final DockerComposeFailure failure;
  @override
  String toString() => 'DockerComposeException(${failure.name})';
}

/// The vault passphrase was wrong when revealing secrets.
final class DockerWrongPassphrase implements Exception {
  const DockerWrongPassphrase();
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

  Future<List<DockerVolume>> volumes(String sessionId);

  /// The container's environment with secret values; the core checks the
  /// vault [passphrase] first and throws [DockerWrongPassphrase] if wrong.
  /// The caller wipes [passphrase].
  Future<List<String>> revealEnvironment(String sessionId, String containerId, SecretText passphrase);

  /// The project's compose files, read on the engine host. Throws
  /// [DockerComposeException] when they cannot be located.
  Future<DockerComposeProject> composeProject(String sessionId, String project);

  /// Checks the vault [passphrase] (throws [DockerWrongPassphrase]) and
  /// returns a short-lived grant to edit [project]. Wipes [passphrase].
  Future<String> composeUnlock(String sessionId, String project, SecretText passphrase);

  /// Validates the edited files on the host, then writes them (keeping
  /// backups); with [apply] runs `up -d` as a job. Wipes the edits' content.
  Future<DockerComposeSaveResult> composeSave(
    String sessionId,
    String project,
    String grant,
    List<DockerComposeEdit> edits, {
    bool apply = true,
  });

  /// Output of an `up -d` job until it ends (the last update has `done`).
  Stream<DockerComposeJobUpdate> composeJob(String jobId);

  Future<DockerForward> startForward(
    String sessionId,
    String containerId, {
    required int remotePort,
    int localPort = 0,
  });
  Future<List<DockerForward>> forwards();
  Future<void> stopForward(String forwardId);
}
