import 'dart:async';

import 'package:consolecrypt/core/mock/mock_config.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/docker/docker_service.dart';

/// In-memory Docker: this computer plus two SSH hosts (one whose user is not
/// in the `docker` group), two compose projects and two standalone
/// containers. Followed logs emit a few lines every [logInterval].
class MockDockerService implements DockerService {
  MockDockerService({this.latency = Duration.zero, this.logInterval = const Duration(seconds: 2)});

  final Duration latency;
  final Duration logInterval;

  static const permissionDeniedHostId = 'ssh-ci';
  static const _hosts = [
    DockerHost(id: DockerHost.localId, name: 'This computer', kind: DockerHostKind.local),
    DockerHost(id: 'ssh-web', name: 'web-01', kind: DockerHostKind.ssh, address: 'deploy@web-01.example.org'),
    DockerHost(id: permissionDeniedHostId, name: 'ci-runner', kind: DockerHostKind.ssh, address: 'ci@ci.example.org'),
  ];

  final _sessions = <String, DockerSessionInfo>{};
  final _containers = <String, List<DockerContainer>>{};
  final _forwards = <String, DockerForward>{};
  var _next = 0;

  /// Calls made through the service, for tests.
  final actions = <(String containerId, DockerAction action)>[];

  @override
  Future<List<DockerHost>> hosts() async {
    await mockDelay(latency);
    return _hosts;
  }

  @override
  Future<DockerSessionInfo> connect(String hostId) async {
    await mockDelay(latency);
    final host = _hosts.where((h) => h.id == hostId).firstOrNull;
    if (host == null) throw const DockerException(DockerFailure.connectionFailed, 'host not found');
    if (hostId == permissionDeniedHostId) throw const DockerException(DockerFailure.permissionDenied, 'docker');
    final id = 'docker-session-${++_next}';
    final containers = _seed(hostId);
    _containers[id] = containers;
    final local = host.kind == DockerHostKind.local;
    return _sessions[id] = DockerSessionInfo(
      id: id,
      hostId: hostId,
      hostName: host.name,
      engineVersion: local ? '27.3.1' : '26.1.4',
      os: 'linux',
      arch: local ? 'arm64' : 'amd64',
      containersRunning: containers.where((c) => c.state == DockerContainerState.running).length,
      containersTotal: containers.length,
      images: 14,
    );
  }

  @override
  Future<void> disconnect(String sessionId) async {
    _sessions.remove(sessionId);
    _containers.remove(sessionId);
    _forwards.removeWhere((_, f) => f.sessionId == sessionId);
  }

  List<DockerContainer> _list(String sessionId) =>
      _containers[sessionId] ?? (throw const DockerException(DockerFailure.connectionFailed, 'session closed'));

  @override
  Future<List<DockerContainer>> containers(String sessionId, {bool all = true}) async {
    await mockDelay(latency);
    final list = _list(sessionId);
    return all ? List.of(list) : list.where((c) => c.state == DockerContainerState.running).toList();
  }

  @override
  Future<DockerContainerDetail> inspect(String sessionId, String containerId) async {
    await mockDelay(latency);
    final container = _list(sessionId).where((c) => c.id == containerId).firstOrNull;
    if (container == null) throw const DockerException(DockerFailure.other, 'no such container');
    final created = container.createdAt ?? DateTime.utc(2026, 10, 1);
    return DockerContainerDetail(
      container: container,
      inspect: {
        'Id': container.id,
        'Name': '/${container.name}',
        'Created': created.toIso8601String(),
        'Path': 'docker-entrypoint.sh',
        'Args': [container.composeService ?? container.name],
        'RestartCount': container.name.contains('worker') ? 3 : 0,
        'State': {
          'Status': container.state.name,
          'Running': container.state == DockerContainerState.running,
          'StartedAt': created.add(const Duration(minutes: 1)).toIso8601String(),
          if (container.health != null) 'Health': {'Status': container.health},
        },
        'Config': {
          'Image': container.image,
          'Env': [
            'PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin',
            'APP_ENV=production',
            if (container.image.startsWith('postgres')) 'POSTGRES_PASSWORD=••••••',
            if (container.composeService == 'api') 'API_TOKEN=••••••',
          ],
          'Labels': {
            if (container.composeProject != null) 'com.docker.compose.project': container.composeProject,
            if (container.composeService != null) 'com.docker.compose.service': container.composeService,
            'org.opencontainers.image.source': 'https://example.org/${container.image.split(':').first}',
          },
          'ExposedPorts': {
            for (final port in container.ports) '${port.privatePort}/${port.protocol}': <String, Object>{},
          },
        },
        'Mounts': [
          if (container.image.startsWith('postgres') || container.image.startsWith('redis'))
            {
              'Type': 'volume',
              'Name': '${container.composeProject ?? 'data'}_data',
              'Source': '/var/lib/docker/volumes/${container.composeProject ?? 'data'}_data/_data',
              'Destination': container.image.startsWith('postgres') ? '/var/lib/postgresql/data' : '/data',
              'RW': true,
            },
          if (container.composeService == 'web')
            {'Type': 'bind', 'Source': '/srv/shop/nginx.conf', 'Destination': '/etc/nginx/nginx.conf', 'RW': false},
        ],
        'NetworkSettings': {
          'Networks': {
            for (final entry in container.networks.entries)
              entry.key: {'IPAddress': entry.value, 'Gateway': '172.18.0.1'},
          },
        },
      },
    );
  }

  @override
  Future<void> action(String sessionId, String containerId, DockerAction action) async {
    await mockDelay(latency);
    actions.add((containerId, action));
    final list = _list(sessionId);
    final index = list.indexWhere((c) => c.id == containerId);
    if (index < 0) throw const DockerException(DockerFailure.other, 'no such container');
    final current = list[index];
    if (action == DockerAction.remove) {
      list.removeAt(index);
      return;
    }
    final (state, status) = switch (action) {
      DockerAction.start ||
      DockerAction.restart ||
      DockerAction.unpause => (DockerContainerState.running, 'Up 1 second'),
      DockerAction.stop => (DockerContainerState.exited, 'Exited (0) 1 second ago'),
      DockerAction.kill => (DockerContainerState.exited, 'Exited (137) 1 second ago'),
      DockerAction.pause => (DockerContainerState.paused, 'Up 2 hours (Paused)'),
      DockerAction.remove => (current.state, current.status),
    };
    list[index] = _copy(current, state: state, status: status);
  }

  @override
  Stream<List<DockerLogLine>> logs(
    String sessionId,
    String containerId, {
    int tail = 500,
    bool follow = true,
    bool timestamps = false,
  }) {
    late final StreamController<List<DockerLogLine>> controller;
    Timer? timer;
    var n = 0;
    DockerLogLine line(DockerLogStream stream, String text) => DockerLogLine(
      stream: stream,
      text: text,
      timestamp: timestamps ? DateTime.utc(2026, 10, 21, 9).add(Duration(seconds: n)) : null,
    );
    List<DockerLogLine> batch() {
      n++;
      return [
        line(DockerLogStream.stdout, 'GET /api/orders/$n 200 ${12 + n % 30}ms'),
        if (n % 3 == 0) line(DockerLogStream.stderr, 'warning: slow query took ${200 + n}ms'),
      ];
    }

    controller = StreamController<List<DockerLogLine>>(
      onListen: () {
        if (!_containers.containsKey(sessionId)) {
          controller.addError(const DockerException(DockerFailure.connectionFailed, 'session closed'));
          return;
        }
        controller.add([
          line(DockerLogStream.stdout, 'Starting service…'),
          line(DockerLogStream.stdout, 'Listening on 0.0.0.0:8080'),
          line(DockerLogStream.stderr, 'deprecation: config key "workers" is ignored'),
          ...batch(),
        ]);
        if (follow) {
          timer = Timer.periodic(logInterval, (_) => controller.add(batch()));
        } else {
          unawaited(controller.close());
        }
      },
      onCancel: () {
        timer?.cancel();
        unawaited(controller.close());
      },
    );
    return controller.stream;
  }

  @override
  Future<DockerForward> startForward(
    String sessionId,
    String containerId, {
    required int remotePort,
    int localPort = 0,
  }) async {
    await mockDelay(latency);
    final container = _list(sessionId).where((c) => c.id == containerId).firstOrNull;
    if (container == null) throw const DockerException(DockerFailure.other, 'no such container');
    final id = 'docker-forward-${++_next}';
    return _forwards[id] = DockerForward(
      id: id,
      sessionId: sessionId,
      containerName: container.name,
      remotePort: remotePort,
      address: '127.0.0.1:${localPort == 0 ? 49152 + _next : localPort}',
      state: DockerForwardState.running,
      bytesSent: 2048,
      bytesReceived: 16384,
      activeConnections: 1,
    );
  }

  @override
  Future<List<DockerForward>> forwards() async => _forwards.values.toList();

  @override
  Future<void> stopForward(String forwardId) async => _forwards.remove(forwardId);

  /// Demo vault passphrase for revealing secrets.
  static const demoPassphrase = 'demo'; // l10n-ignore: demo value

  @override
  Future<List<DockerVolume>> volumes(String sessionId) async {
    await mockDelay(latency);
    final list = _list(sessionId);
    return [
      for (final project in {...list.map((c) => c.composeProject).nonNulls})
        DockerVolume(
          name: '${project}_data', // l10n-ignore: demo volume
          driver: 'local', // l10n-ignore: demo driver
          mountpoint: '/var/lib/docker/volumes/${project}_data/_data', // l10n-ignore: demo path
          composeProject: project,
          usedBy: [
            for (final c in list)
              if (c.composeProject == project && c.composeService != 'web') c.name,
          ],
        ),
      const DockerVolume(
        name: 'orphaned-cache',
        driver: 'local',
        mountpoint: '/var/lib/docker/volumes/orphaned-cache/_data',
      ), // l10n-ignore: demo volume
    ];
  }

  @override
  Future<List<String>> revealEnvironment(String sessionId, String containerId, SecretText passphrase) async {
    await mockDelay(latency);
    if (passphrase.expose() != demoPassphrase) throw const DockerWrongPassphrase();
    final detail = await inspect(sessionId, containerId);
    final env = ((detail.inspect['Config'] as Map)['Env'] as List).cast<String>();
    return [for (final entry in env) entry.replaceAll('••••••', 'demo-secret-value')]; // l10n-ignore: demo value
  }

  static DockerContainer _copy(DockerContainer c, {required DockerContainerState state, required String status}) =>
      DockerContainer(
        id: c.id,
        name: c.name,
        image: c.image,
        state: state,
        status: status,
        createdAt: c.createdAt,
        ports: state == DockerContainerState.running || state == DockerContainerState.paused
            ? c.ports
            : [for (final port in c.ports) DockerPort(privatePort: port.privatePort, protocol: port.protocol)],
        composeProject: c.composeProject,
        composeService: c.composeService,
        networks: c.networks,
        health: state == DockerContainerState.running ? c.health : null,
      );

  static String _id(String seed) {
    final hash = seed.codeUnits.fold<int>(7, (h, c) => (h * 31 + c) & 0x7fffffff);
    final hex = hash.toRadixString(16).padLeft(8, '0');
    return (hex * 8).substring(0, 64);
  }

  static List<DockerContainer> _seed(String hostId) {
    final created = DateTime.utc(2026, 10, 18, 7, 30);
    DockerContainer c(
      String name,
      String image,
      DockerContainerState state,
      String status, {
      String? project,
      String? service,
      List<DockerPort> ports = const [],
      String? health,
      String ip = '172.18.0.2',
    }) => DockerContainer(
      id: _id('$hostId/$name'),
      name: name,
      image: image,
      state: state,
      status: status,
      createdAt: created,
      ports: ports,
      composeProject: project,
      composeService: service,
      networks: {project == null ? 'bridge' : '${project}_default': ip},
      health: health,
    );
    const running = DockerContainerState.running;
    return [
      c(
        'shop-web-1',
        'nginx:1.27-alpine',
        running,
        'Up 3 days',
        project: 'shop',
        service: 'web',
        ports: const [
          DockerPort(privatePort: 80, publicPort: 8080, protocol: 'tcp', ip: '0.0.0.0'),
          DockerPort(privatePort: 80, publicPort: 8080, protocol: 'tcp', ip: '::'),
        ],
        ip: '172.18.0.5',
      ),
      c(
        'shop-api-1',
        'example/shop-api:2.4.1',
        running,
        'Up 3 days (healthy)',
        project: 'shop',
        service: 'api',
        ports: const [DockerPort(privatePort: 3000, protocol: 'tcp')],
        health: 'healthy',
        ip: '172.18.0.4',
      ),
      c(
        'shop-postgres-1',
        'postgres:16',
        running,
        'Up 3 days (healthy)',
        project: 'shop',
        service: 'postgres',
        ports: const [DockerPort(privatePort: 5432, protocol: 'tcp')],
        health: 'healthy',
        ip: '172.18.0.2',
      ),
      c(
        'shop-redis-1',
        'redis:7-alpine',
        running,
        'Up 3 days',
        project: 'shop',
        service: 'redis',
        ports: const [DockerPort(privatePort: 6379, protocol: 'tcp')],
        ip: '172.18.0.3',
      ),
      c(
        'monitoring-prometheus-1',
        'prom/prometheus:v2.54.1',
        running,
        'Up 26 hours',
        project: 'monitoring',
        service: 'prometheus',
        ports: const [DockerPort(privatePort: 9090, publicPort: 9090, protocol: 'tcp', ip: '127.0.0.1')],
        ip: '172.19.0.2',
      ),
      c(
        'monitoring-grafana-1',
        'grafana/grafana:11.2.0',
        DockerContainerState.exited,
        'Exited (1) 2 hours ago',
        project: 'monitoring',
        service: 'grafana',
        ports: const [DockerPort(privatePort: 3000, protocol: 'tcp')],
        ip: '172.19.0.3',
      ),
      c(
        'queue-worker',
        'example/worker:latest',
        DockerContainerState.restarting,
        'Restarting (1) 5 seconds ago',
        health: 'unhealthy',
      ),
      c('backup-once', 'alpine:3.20', DockerContainerState.created, 'Created', ip: ''),
    ];
  }
}
