import 'package:consolecrypt/db/db_forward_connect.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_port_detect.dart';
import 'package:consolecrypt/docker/docker_format.dart';
import 'package:consolecrypt/docker/docker_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// A database a container runs: the engine and the container port it
/// listens on.
final class DockerDbTarget {
  const DockerDbTarget({required this.engine, required this.remotePort});
  final DbEngine engine;
  final int remotePort;
}

/// The database of a container whose published or exposed TCP ports include
/// a standard database port, or null. The image tells MariaDB from MySQL;
/// otherwise the port decides.
DockerDbTarget? dockerDatabaseTarget(DockerContainer container, [DockerJson? inspect]) {
  final ports = dockerForwardablePorts(container, inspect);
  final byPort = ports.map(dbEngineForPort).nonNulls.firstOrNull;
  if (byPort == null) return null;
  final byImage = dbEngineForImage(container.image);
  final engine = byImage != null && ports.contains(dbForwardPort(byImage)) ? byImage : byPort;
  return DockerDbTarget(engine: engine, remotePort: dbForwardPort(engine));
}

/// Where a saved connection finds the database without a temporary forward.
typedef DockerDbEndpoint = ({String host, int port, DbRoute route});

// Bind addresses that mean "reachable on the Docker host's loopback".
const _loopbackBinds = {'', '0.0.0.0', '::', '127.0.0.1'}; // l10n-ignore: addresses

/// How a saved connection reaches [remotePort] of [container] by itself:
///  * a published port: the host port on the Docker host (directly on this
///    computer, over SSH to the saved host otherwise);
///  * over SSH, an unpublished port: the container IP on its Docker network
///    (or the Docker host itself for host networking).
/// Null when only a temporary forward can reach it (an unpublished port of
/// a local container, or a remote one without an IP).
DockerDbEndpoint? dockerDatabaseEndpoint(DockerSessionInfo session, DockerContainer container, int remotePort) {
  final local = session.hostId == DockerHost.localId;
  final route = local ? const DbRoute() : DbRoute(kind: DbRouteKind.ssh, reference: session.hostId);
  final bindings = [
    for (final port in container.ports)
      if (port.protocol == 'tcp' && port.privatePort == remotePort && port.publicPort != null) port,
  ];
  if (bindings.isNotEmpty) {
    final loopback = bindings.where((port) => _loopbackBinds.contains(port.ip ?? '')).firstOrNull;
    final binding = loopback ?? bindings.first;
    return (host: loopback != null ? '127.0.0.1' : binding.ip!, port: binding.publicPort!, route: route);
  }
  if (local) return null;
  if (container.networks.containsKey('host')) return (host: '127.0.0.1', port: remotePort, route: route);
  final ip = container.networks.values.where((ip) => ip.isNotEmpty).firstOrNull;
  return ip == null ? null : (host: ip, port: remotePort, route: route);
}

/// Database name and user from the container environment (`KEY=value`),
/// skipping values the core masked.
({String database, String user}) dockerDatabaseEnv(DockerJson inspect) {
  final env = ((inspect['Config'] as Map?)?['Env'] as List? ?? const []).whereType<String>();
  String find(List<String> names) {
    for (final entry in env) {
      final split = entry.indexOf('=');
      if (split <= 0) continue;
      final value = entry.substring(split + 1);
      if (names.contains(entry.substring(0, split)) && !value.contains('•')) return value;
    }
    return '';
  }

  return (database: find(dbDatabaseEnv), user: find(dbUserEnv));
}

/// The route of a container port only a Docker forward reaches: the core
/// restores the forward (by container name) on every connect.
DockerDbEndpoint dockerForwardEndpoint(DockerSessionInfo session, DockerContainer container, int remotePort) => (
  host: '127.0.0.1',
  port: remotePort,
  route: DbRoute(
    kind: DbRouteKind.docker,
    reference: session.hostId == DockerHost.localId ? null : session.hostId,
    container: container.name,
  ),
);

/// Opens the container's database in the Databases section: the matching
/// saved connection, or a prefilled form where only the database, login and
/// password are left. A published port (or, over SSH, the container IP) is
/// saved with its route so the connection works again later; otherwise the
/// connection keeps a Docker route and the forward is restored on connect.
Future<void> connectDockerDatabase(
  BuildContext context,
  WidgetRef ref, {
  required DockerService service,
  required DockerSessionInfo session,
  required DockerContainer container,
  required DockerDbTarget target,
}) async {
  var hints = (database: '', user: '');
  try {
    hints = dockerDatabaseEnv((await service.inspect(session.id, container.id)).inspect);
  } catch (_) {
    // The form is still prefilled with the address.
  }
  final endpoint =
      dockerDatabaseEndpoint(session, container, target.remotePort) ??
      dockerForwardEndpoint(session, container, target.remotePort);
  if (!context.mounted) return;
  await openPrefilledDatabase(
    context,
    ref,
    engine: target.engine,
    host: endpoint.host,
    port: endpoint.port,
    route: endpoint.route,
    name: container.name,
    database: hints.database,
    user: hints.user,
    // A remote engine may be production; the form lets the user say otherwise.
    production: session.hostId != DockerHost.localId,
    tag: 'docker', // l10n-ignore: tag
  );
}
