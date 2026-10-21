import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/bridge/rust_account.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/docker/docker_service.dart';
import 'package:consolecrypt/src/rust/api/docker.dart' as native;

/// Docker engines through the native core: the local socket or a saved SSH
/// host's engine socket. Logs are polled in batches.
final class RustDockerService implements DockerService {
  @override
  Future<List<DockerHost>> hosts() async => [
    for (final host in (jsonDecode(await guard(native.dockerHosts)) as List).cast<DockerJson>())
      DockerHost.fromJson(host),
  ];

  @override
  Future<DockerSessionInfo> connect(String hostId) async {
    final json = jsonDecode(await guard(() => native.dockerConnect(hostId: hostId))) as DockerJson;
    if (json['outcome'] == 'connected') return DockerSessionInfo.fromJson(json);
    throw DockerException(switch (json['failure']) {
      'permission_denied' => DockerFailure.permissionDenied,
      'not_running' => DockerFailure.notRunning,
      'connection_failed' => DockerFailure.connectionFailed,
      _ => DockerFailure.other,
    }, json['detail'] as String? ?? '');
  }

  @override
  Future<void> disconnect(String sessionId) => guard(() => native.dockerDisconnect(sessionId: sessionId));

  @override
  Future<List<DockerContainer>> containers(String sessionId, {bool all = true}) async => [
    for (final item in (jsonDecode(
      await guard(() => native.dockerContainers(sessionId: sessionId, all: all)),
    ) as List).cast<DockerJson>())
      DockerContainer.fromJson(item),
  ];

  @override
  Future<DockerContainerDetail> inspect(String sessionId, String containerId) async {
    final inspect =
        jsonDecode(await guard(() => native.dockerInspect(sessionId: sessionId, container: containerId))) as DockerJson;
    final listed = (await containers(sessionId)).where((c) => c.id == containerId || c.name == containerId);
    final container =
        listed.firstOrNull ??
        DockerContainer(
          id: inspect['Id'] as String? ?? containerId,
          name: (inspect['Name'] as String? ?? containerId).replaceFirst('/', ''),
          image: (inspect['Config'] as Map?)?['Image'] as String? ?? '',
          state: DockerContainerState.unknown,
        );
    return DockerContainerDetail(container: container, inspect: inspect);
  }

  @override
  Future<void> action(String sessionId, String containerId, DockerAction action) =>
      guard(() => native.dockerAction(sessionId: sessionId, container: containerId, action: action.name));

  @override
  Stream<List<DockerLogLine>> logs(
    String sessionId,
    String containerId, {
    int tail = 500,
    bool follow = true,
    bool timestamps = false,
  }) {
    String? stream;
    var cancelled = false;
    late final StreamController<List<DockerLogLine>> controller;
    Future<void> run() async {
      try {
        stream = await guard(
          () => native.dockerLogsStart(
            sessionId: sessionId,
            container: containerId,
            tail: tail,
            follow: follow,
            timestamps: timestamps,
          ),
        );
        while (!cancelled) {
          final batch = jsonDecode(await guard(() => native.dockerLogsPoll(streamId: stream!, waitMs: 1000)));
          if (batch == null) break;
          final lines = [for (final line in (batch as List).cast<DockerJson>()) DockerLogLine.fromJson(line)];
          if (lines.isNotEmpty && !cancelled) controller.add(lines);
        }
      } on AppException catch (error, stack) {
        if (!cancelled) controller.addError(error, stack);
      } finally {
        if (!controller.isClosed) await controller.close();
      }
    }

    controller = StreamController<List<DockerLogLine>>(
      onListen: () => unawaited(run()),
      onCancel: () async {
        cancelled = true;
        final id = stream;
        if (id != null) {
          try {
            await native.dockerLogsStop(streamId: id);
          } catch (_) {}
        }
      },
    );
    return controller.stream;
  }

  @override
  Future<List<DockerVolume>> volumes(String sessionId) async => [
    for (final item
        in (jsonDecode(await guard(() => native.dockerVolumes(sessionId: sessionId))) as List).cast<DockerJson>())
      DockerVolume.fromJson(item),
  ];

  @override
  Future<List<String>> revealEnvironment(String sessionId, String containerId, SecretText passphrase) async {
    try {
      final json = await withSecret(
        passphrase,
        (bytes) => guard(
          () => native.dockerRevealEnvironment(sessionId: sessionId, container: containerId, passphrase: bytes),
        ),
      );
      return (jsonDecode(json) as List).cast<String>();
    } on AppException catch (error) {
      if (error.code == AppErrorCode.wrongPassphrase) throw const DockerWrongPassphrase();
      rethrow;
    }
  }

  @override
  Future<DockerComposeProject> composeProject(String sessionId, String project) async {
    try {
      final json = await guard(() => native.dockerComposeProject(sessionId: sessionId, project: project));
      return DockerComposeProject.fromJson(jsonDecode(json) as DockerJson);
    } on AppException catch (error) {
      final failure = switch (error.args['rule']) {
        'compose_project_not_found' => DockerComposeFailure.projectGone,
        'compose_files_unknown' || 'compose_files_invalid' => DockerComposeFailure.filesUnknown,
        _ => null,
      };
      if (failure != null) throw DockerComposeException(failure);
      rethrow;
    }
  }

  @override
  Future<String> composeUnlock(String sessionId, String project, SecretText passphrase) async {
    try {
      return await withSecret(
        passphrase,
        (bytes) => guard(() => native.dockerComposeUnlock(sessionId: sessionId, project: project, passphrase: bytes)),
      );
    } on AppException catch (error) {
      if (error.code == AppErrorCode.wrongPassphrase) throw const DockerWrongPassphrase();
      rethrow;
    }
  }

  @override
  Future<DockerComposeSaveResult> composeSave(
    String sessionId,
    String project,
    String grant,
    List<DockerComposeEdit> edits, {
    bool apply = true,
  }) async {
    // The edited texts travel as one UTF-8 buffer that is zeroed here and
    // in the core.
    final payload = Uint8List.fromList(
      utf8.encode(
        jsonEncode([
          for (final edit in edits) {'path': edit.path, 'content': edit.content.expose(), 'digest': edit.digest},
        ]),
      ),
    );
    for (final edit in edits) {
      edit.content.wipe();
    }
    try {
      final json = await guard(
        () => native.dockerComposeSave(
          sessionId: sessionId,
          project: project,
          grant: grant,
          edits: payload,
          apply: apply,
        ),
      );
      return dockerComposeSaveResultFromJson(jsonDecode(json) as DockerJson);
    } finally {
      payload.fillRange(0, payload.length, 0);
    }
  }

  @override
  Stream<DockerComposeJobUpdate> composeJob(String jobId) async* {
    while (true) {
      final update = DockerComposeJobUpdate.fromJson(
        jsonDecode(await guard(() => native.dockerComposeJobPoll(jobId: jobId, waitMs: 1000))) as DockerJson,
      );
      if (update.lines.isNotEmpty || update.done) yield update;
      if (update.done) return;
    }
  }

  @override
  Future<DockerForward> startForward(
    String sessionId,
    String containerId, {
    required int remotePort,
    int localPort = 0,
  }) async => DockerForward.fromJson(
    jsonDecode(
      await guard(
        () => native.dockerForwardStart(
          sessionId: sessionId,
          container: containerId,
          remotePort: remotePort,
          localPort: localPort,
        ),
      ),
    ) as DockerJson,
  );

  @override
  Future<List<DockerForward>> forwards() async => [
    for (final item in (jsonDecode(await guard(native.dockerForwards)) as List).cast<DockerJson>())
      DockerForward.fromJson(item),
  ];

  @override
  Future<void> stopForward(String forwardId) => guard(() => native.dockerForwardStop(forwardId: forwardId));
}
