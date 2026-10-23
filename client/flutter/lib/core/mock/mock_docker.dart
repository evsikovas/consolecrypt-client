import 'dart:async';

import 'package:consolecrypt/core/mock/mock_config.dart';
import 'package:consolecrypt/core/mock/mock_presence.dart';
import 'package:consolecrypt/core/models/step_up.dart';
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
    final container = _find(sessionId, containerId);
    return DockerContainerDetail(container: container, inspect: _inspect(container, full: false));
  }

  @override
  Future<DockerJson> revealInspect(String sessionId, String containerId, StepUpProof proof) async {
    await mockDelay(latency);
    if (!MockPresenceTokens.check(proof, demoPassphrase, StepUpAction.dockerReveal, '$sessionId/$containerId')) {
      throw const DockerWrongPassphrase();
    }
    return _inspect(_find(sessionId, containerId), full: true);
  }

  /// Demo stand-in for the core's detector (the native build uses it).
  @override
  bool containsSecret(String text) => _secretPattern.hasMatch(text);

  static final _secretPattern = RegExp(
    r'(password|passwd|secret|token|api[_-]?key)\s*[=:]\s*\S|' // l10n-ignore: pattern
    r'[a-z][a-z0-9+.-]*://[^/\s:@]*:[^@\s]+@|' // l10n-ignore: pattern
    r'--(password|requirepass|token|secret|api-key)[= ]\S|' // l10n-ignore: pattern
    r'-----BEGIN [A-Z ]*PRIVATE KEY-----|\b(ghp_|glpat-|xox[abpr]-|AKIA)[A-Za-z0-9_-]{10,}', // l10n-ignore: pattern
    caseSensitive: false,
  );

  DockerContainer _find(String sessionId, String containerId) =>
      _list(sessionId).where((c) => c.id == containerId).firstOrNull ??
      (throw const DockerException(DockerFailure.other, 'no such container'));

  /// The inspect JSON; without [full] secrets read `••••••` as the core
  /// masks them.
  DockerJson _inspect(DockerContainer container, {required bool full}) {
    String secret(String value) => full ? value : '••••••'; // l10n-ignore: native mask
    final created = container.createdAt ?? DateTime.utc(2026, 10, 1);
    final redis = container.image.startsWith('redis');
    final command = redis
        ? ['redis-server', '--requirepass', secret('demo-redis-pass'), '--appendonly', 'yes'] // l10n-ignore: demo
        : [container.composeService ?? container.name];
    return {
      'Id': container.id,
      'Name': '/${container.name}',
      'Created': created.toIso8601String(),
      'Path': 'docker-entrypoint.sh',
      'Args': command,
      'RestartCount': container.name.contains('worker') ? 3 : 0,
      'State': {
        'Status': container.state.name,
        'Running': container.state == DockerContainerState.running,
        'StartedAt': created.add(const Duration(minutes: 1)).toIso8601String(),
        if (container.health != null) 'Health': {'Status': container.health},
      },
      'Config': {
        'Image': container.image,
        'Cmd': command,
        'Env': [
          'PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin',
          'APP_ENV=production',
          if (container.image.startsWith('postgres')) ...[
            'POSTGRES_DB=shop',
            'POSTGRES_USER=shop',
            'POSTGRES_PASSWORD=${secret('demo-postgres-pass')}', // l10n-ignore: demo
          ],
          if (container.composeService == 'api') ...[
            'DATABASE_URL=postgres://shop:${secret('demo-postgres-pass')}@postgres:5432/shop', // l10n-ignore: demo
            'API_TOKEN=${secret('demo-api-token')}', // l10n-ignore: demo
          ],
        ],
        'Labels': {
          if (container.composeProject != null) 'com.docker.compose.project': container.composeProject,
          if (container.composeService != null) 'com.docker.compose.service': container.composeService,
          if (container.composeService == 'api') 'com.example.deploy.token': secret('demo-deploy-token'),
          'org.opencontainers.image.source': 'https://example.org/${container.image.split(':').first}',
        },
        'ExposedPorts': {
          for (final port in container.ports) '${port.privatePort}/${port.protocol}': <String, Object>{},
        },
      },
      'Mounts': [
        if (container.image.startsWith('postgres') || redis)
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
    };
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
        final api = _containers[sessionId]!.any((c) => c.id == containerId && c.composeService == 'api');
        controller.add([
          line(DockerLogStream.stdout, 'Starting service…'),
          if (api)
            line(
              DockerLogStream.stdout,
              'connecting to postgres://shop:demo-postgres-pass@postgres:5432/shop', // l10n-ignore: demo log
            ),
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
  Future<List<String>> revealEnvironment(String sessionId, String containerId, StepUpProof proof) async {
    await mockDelay(latency);
    if (!MockPresenceTokens.check(proof, demoPassphrase, StepUpAction.dockerReveal, '$sessionId/$containerId')) {
      throw const DockerWrongPassphrase();
    }
    final full = _inspect(_find(sessionId, containerId), full: true);
    return ((full['Config'] as Map)['Env'] as List).cast<String>();
  }

  /// Compose files of the demo projects (path → text), shared by sessions.
  final composeFiles = <String, String>{
    '/srv/shop/compose.yaml': _shopCompose, // l10n-ignore: demo path
    '/srv/shop/compose.override.yaml': _shopOverride, // l10n-ignore: demo path
    '/opt/monitoring/docker-compose.yml': _monitoringCompose, // l10n-ignore: demo path
  };

  /// Files the demo SSH user may not read.
  static const composeRootOnly = '/opt/monitoring/docker-compose.secrets.yml'; // l10n-ignore: demo path

  static const _composeLayout = {
    'shop': ['/srv/shop/compose.yaml', '/srv/shop/compose.override.yaml'], // l10n-ignore: demo paths
    'monitoring': ['/opt/monitoring/docker-compose.yml', composeRootOnly], // l10n-ignore: demo paths
  };

  /// Backups written by saves, for tests.
  final composeBackups = <String>[];
  final _grants = <String>{};
  final _composeJobs = <String, String>{};

  /// Directories the demo reports as writable by other users.
  static const composeSharedDirs = ['/opt/monitoring']; // l10n-ignore: demo path

  /// Literal secrets in the demo files and how the core masks them.
  static const _composeSecrets = {
    'demo-redis-pass': '••••••', // l10n-ignore: demo
    'https://demo-sentry-key@o1.ingest.example.org/7': '••••••', // l10n-ignore: demo
    'demo-grafana-admin': '••••••', // l10n-ignore: demo
  };

  static String composeDigest(String text) =>
      text.codeUnits.fold<int>(17, (h, c) => (h * 31 + c) & 0x7fffffff).toRadixString(16);

  @override
  Future<DockerComposeProject> composeProject(String sessionId, String project) =>
      _composeProject(sessionId, project, full: false);

  @override
  Future<DockerComposeProject?> composeReveal(String sessionId, String project, String grant) async {
    if (!_grants.contains('$sessionId/$project/$grant')) return null;
    return _composeProject(sessionId, project, full: true);
  }

  Future<DockerComposeProject> _composeProject(String sessionId, String project, {required bool full}) async {
    await mockDelay(latency);
    if (!_list(sessionId).any((c) => c.composeProject == project)) {
      throw const DockerComposeException(DockerComposeFailure.projectGone);
    }
    final paths = _composeLayout[project];
    if (paths == null) throw const DockerComposeException(DockerComposeFailure.filesUnknown);
    return DockerComposeProject(
      project: project,
      workingDir: paths.first.substring(0, paths.first.lastIndexOf('/')),
      envFiles: project == 'shop' ? const ['/srv/shop/.env'] : const [], // l10n-ignore: demo path
      files: [
        for (final path in paths)
          if (path == composeRootOnly)
            DockerComposeFile(path: path, error: DockerComposeFileError.permissionDenied)
          else if (composeFiles[path] case final text?)
            DockerComposeFile(
              path: path,
              content: SecretText(full ? text : _maskCompose(text)),
              digest: composeDigest(text),
              size: text.length,
              masked: !full && _maskCompose(text) != text,
            )
          else
            DockerComposeFile(path: path, error: DockerComposeFileError.notFound),
      ],
    );
  }

  static String _maskCompose(String text) =>
      _composeSecrets.entries.fold(text, (masked, entry) => masked.replaceAll(entry.key, entry.value));

  @override
  Future<String> composeUnlock(String sessionId, String project, StepUpProof proof) async {
    await mockDelay(latency);
    if (!MockPresenceTokens.check(proof, demoPassphrase, StepUpAction.dockerCompose, '$sessionId/$project')) {
      throw const DockerWrongPassphrase();
    }
    final grant = 'compose-grant-${++_next}';
    _grants.add('$sessionId/$project/$grant');
    return grant;
  }

  /// Revokes every edit grant (as if it expired).
  void expireComposeGrants() => _grants.clear();

  @override
  Future<DockerComposeSaveResult> composeSave(
    String sessionId,
    String project,
    String grant,
    List<DockerComposeEdit> edits, {
    bool apply = true,
    bool allowSharedDirs = false,
  }) async {
    await mockDelay(latency);
    final texts = [for (final edit in edits) edit.content.expose()];
    for (final edit in edits) {
      edit.content.wipe();
    }
    if (!_grants.contains('$sessionId/$project/$grant')) return const DockerComposeLocked();
    final shared = {
      for (final edit in edits)
        if (composeSharedDirs.contains(edit.path.substring(0, edit.path.lastIndexOf('/'))))
          edit.path.substring(0, edit.path.lastIndexOf('/')),
    };
    if (shared.isNotEmpty && !allowSharedDirs) return DockerComposeUnsafeDirectory(shared.toList());
    for (final (index, edit) in edits.indexed) {
      final current = composeFiles[edit.path];
      if (current == null) return DockerComposeWriteFailed(edit.path, DockerComposeWriteError.notFound);
      if (composeDigest(current) != edit.digest) return DockerComposeConflict(edit.path);
      final tab = texts[index].split('\n').indexWhere((line) => line.contains('\t'));
      if (tab >= 0) {
        return DockerComposeInvalid(
          '${edit.path}: yaml: line ${tab + 1}: found a tab character that violates indentation', // l10n-ignore: demo output
        );
      }
    }
    final backups = <String>[];
    for (final (index, edit) in edits.indexed) {
      final backup = '${edit.path}.bak-20261021-091500'; // l10n-ignore: demo path
      backups.add(backup);
      composeBackups.add(backup);
      composeFiles[backup] = composeFiles[edit.path]!;
      composeFiles[edit.path] = texts[index];
    }
    String? jobId;
    if (apply) {
      jobId = 'compose-job-${++_next}';
      _composeJobs[jobId] = '$sessionId/$project';
    }
    return DockerComposeSaved(backups: backups, jobId: jobId);
  }

  @override
  Stream<DockerComposeJobUpdate> composeJob(String jobId) async* {
    final target = _composeJobs.remove(jobId);
    if (target == null) throw const DockerException(DockerFailure.other, 'job closed');
    final [sessionId, project] = target.split('/');
    final list = _containers[sessionId] ?? const <DockerContainer>[];
    final services = [
      for (final c in list)
        if (c.composeProject == project) c,
    ];
    await mockDelay(latency);
    yield DockerComposeJobUpdate(
      lines: [for (final c in services) 'Container ${c.name}  Recreate'], // l10n-ignore: demo output
      done: false,
    );
    await mockDelay(latency);
    for (final c in services) {
      final index = list.indexOf(c);
      if (index >= 0) list[index] = _copy(c, state: DockerContainerState.running, status: 'Up 1 second');
    }
    yield DockerComposeJobUpdate(
      lines: [for (final c in services) 'Container ${c.name}  Started'], // l10n-ignore: demo output
      done: true,
      exitCode: 0,
    );
  }

  static const _shopCompose = '''# Shop: web front, API, database and cache.
services:
  web:
    image: nginx:1.27-alpine
    ports:
      - "8080:80"
    volumes:
      - ./nginx.conf:/etc/nginx/nginx.conf:ro
    depends_on: [api]
  api:
    image: example/shop-api:2.4.1
    environment:
      DATABASE_URL: postgres://shop:\${POSTGRES_PASSWORD}@postgres:5432/shop
      API_TOKEN: \${API_TOKEN}
      SENTRY_DSN: https://demo-sentry-key@o1.ingest.example.org/7
  postgres:
    image: postgres:16
    environment:
      POSTGRES_PASSWORD: \${POSTGRES_PASSWORD}
    volumes:
      - shop_data:/var/lib/postgresql/data
  redis:
    image: redis:7-alpine
    command: ["redis-server", "--requirepass", "demo-redis-pass", "--appendonly", "yes"]
volumes:
  shop_data: {}
'''; // l10n-ignore: demo file

  static const _shopOverride = '''services:
  api:
    environment:
      LOG_LEVEL: debug
'''; // l10n-ignore: demo file

  static const _monitoringCompose = '''services:
  prometheus:
    image: prom/prometheus:v2.54.1
    ports:
      - "127.0.0.1:9090:9090"
  grafana:
    image: grafana/grafana:11.2.0
    environment:
      GF_SECURITY_ADMIN_PASSWORD: demo-grafana-admin
'''; // l10n-ignore: demo file

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
