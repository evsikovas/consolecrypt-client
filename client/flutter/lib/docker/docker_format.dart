import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/docker/docker_service.dart';

/// Containers of one compose project, or the standalone ones ([project] null).
final class DockerContainerGroup {
  const DockerContainerGroup({
    required this.project,
    required this.containers,
    required this.running,
    required this.total,
  });
  final String? project;

  /// Containers that pass the filter, sorted by service and name.
  final List<DockerContainer> containers;

  /// Counts over the whole project, regardless of the filter.
  final int running, total;

  /// Stable key for the collapsed state.
  String get key => project ?? '';
}

bool dockerIsActive(DockerContainerState state) =>
    state == DockerContainerState.running ||
    state == DockerContainerState.paused ||
    state == DockerContainerState.restarting;

/// Groups by `composeProject` (alphabetically), standalone containers last.
/// [query] matches name, compose service and image; stopped containers are
/// hidden unless [showStopped].
List<DockerContainerGroup> groupDockerContainers(
  List<DockerContainer> containers, {
  String query = '',
  bool showStopped = true,
}) {
  final needle = query.trim().toLowerCase();
  final byProject = <String?, List<DockerContainer>>{};
  for (final container in containers) {
    byProject.putIfAbsent(container.composeProject, () => []).add(container);
  }
  final projects = byProject.keys.whereType<String>().toList()..sort();
  final groups = <DockerContainerGroup>[];
  for (final project in [...projects, if (byProject.containsKey(null)) null]) {
    final all = byProject[project]!;
    final visible =
        all
            .where((c) => showStopped || dockerIsActive(c.state))
            .where(
              (c) =>
                  needle.isEmpty ||
                  c.name.toLowerCase().contains(needle) ||
                  c.image.toLowerCase().contains(needle) ||
                  (c.composeService?.toLowerCase().contains(needle) ?? false),
            )
            .toList()
          ..sort((a, b) {
            final service = (a.composeService ?? a.name).compareTo(b.composeService ?? b.name);
            return service == 0 ? a.name.compareTo(b.name) : service;
          });
    if (visible.isEmpty) continue;
    groups.add(
      DockerContainerGroup(
        project: project,
        containers: visible,
        running: all.where((c) => c.state == DockerContainerState.running).length,
        total: all.length,
      ),
    );
  }
  return groups;
}

/// `0.0.0.0:8080→80/tcp` for a published port, `80/tcp` otherwise.
String formatDockerPort(DockerPort port) {
  final private = '${port.privatePort}/${port.protocol}';
  if (port.publicPort == null) return private;
  final ip = port.ip;
  final host = ip == null || ip.isEmpty
      ? ''
      : ip.contains(':')
      ? '[$ip]:'
      : '$ip:';
  return '$host${port.publicPort}→$private';
}

/// Distinct port labels of a container (IPv4 and IPv6 bindings collapse when
/// they are identical apart from the address family).
List<String> formatDockerPorts(List<DockerPort> ports) => {for (final port in ports) formatDockerPort(port)}.toList();

/// Container ports a forward can target (TCP only, ascending).
List<int> dockerForwardablePorts(DockerContainer container, [DockerJson? inspect]) {
  final ports = <int>{
    for (final port in container.ports)
      if (port.protocol == 'tcp') port.privatePort,
  };
  final exposed = (inspect?['Config'] as Map?)?['ExposedPorts'];
  if (exposed is Map) {
    for (final key in exposed.keys) {
      final parts = key.toString().split('/');
      final port = int.tryParse(parts.first);
      if (port != null && (parts.length < 2 || parts[1] == 'tcp')) ports.add(port);
    }
  }
  return ports.where((port) => port > 0 && port <= 65535).toList()..sort();
}

String dockerStateLabel(AppLocalizations l, DockerContainerState state) => switch (state) {
  DockerContainerState.created => l.dockerStateCreated,
  DockerContainerState.running => l.dockerStateRunning,
  DockerContainerState.paused => l.dockerStatePaused,
  DockerContainerState.restarting => l.dockerStateRestarting,
  DockerContainerState.removing => l.dockerStateRemoving,
  DockerContainerState.exited => l.dockerStateExited,
  DockerContainerState.dead => l.dockerStateDead,
  DockerContainerState.unknown => l.dockerStateUnknown,
};

GlassTone dockerStateTone(DockerContainerState state) => switch (state) {
  DockerContainerState.running => GlassTone.success,
  DockerContainerState.paused || DockerContainerState.restarting || DockerContainerState.removing => GlassTone.warning,
  DockerContainerState.created => GlassTone.info,
  DockerContainerState.dead => GlassTone.danger,
  DockerContainerState.exited => GlassTone.neutral,
  DockerContainerState.unknown => GlassTone.unknown,
};

/// Localized health or null when the container has no health check.
(String, GlassTone)? dockerHealth(AppLocalizations l, String? health) => switch (health) {
  'healthy' => (l.dockerHealthHealthy, GlassTone.success),
  'unhealthy' => (l.dockerHealthUnhealthy, GlassTone.danger),
  'starting' => (l.dockerHealthStarting, GlassTone.warning),
  _ => null,
};

String dockerActionLabel(AppLocalizations l, DockerAction action) => switch (action) {
  DockerAction.start => l.dockerActionStart,
  DockerAction.stop => l.dockerActionStop,
  DockerAction.restart => l.dockerActionRestart,
  DockerAction.pause => l.dockerActionPause,
  DockerAction.unpause => l.dockerActionUnpause,
  DockerAction.kill => l.dockerActionKill,
  DockerAction.remove => l.dockerActionRemove,
};

/// Whether [action] makes sense for a container in [state].
bool dockerActionAllowed(DockerAction action, DockerContainerState state) => switch (action) {
  DockerAction.start => !dockerIsActive(state) && state != DockerContainerState.removing,
  DockerAction.stop => state == DockerContainerState.running || state == DockerContainerState.restarting,
  DockerAction.restart => state != DockerContainerState.removing,
  DockerAction.pause => state == DockerContainerState.running,
  DockerAction.unpause => state == DockerContainerState.paused,
  DockerAction.kill => dockerIsActive(state),
  DockerAction.remove => state != DockerContainerState.removing,
};

/// Localized text for a failed Docker request.
String dockerErrorText(AppLocalizations l, Object error) {
  if (error is! DockerException) return l.dockerRequestFailed;
  final detail = error.detail.trim();
  final base = switch (error.failure) {
    DockerFailure.permissionDenied => l.dockerPermissionDeniedTitle,
    DockerFailure.notRunning => l.dockerNotRunningTitle,
    DockerFailure.connectionFailed => l.dockerConnectionFailedTitle,
    DockerFailure.other => l.dockerRequestFailed,
  };
  return detail.isEmpty || error.failure == DockerFailure.permissionDenied ? base : '$base: $detail';
}
