import 'dart:convert';

import 'package:consolecrypt/docker/docker_kube_plan.dart';
import 'package:consolecrypt/docker/docker_service.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yaml/yaml.dart';

const _container = DockerContainer(
  id: 'id',
  name: 'dev-http',
  image: 'python:3.14.4-slim-bookworm',
  state: DockerContainerState.running,
  ports: [DockerPort(privatePort: 8080, protocol: 'tcp')],
);
const _session = DockerSessionInfo(
  id: 'docker',
  hostId: 'local',
  hostName: 'local',
  engineVersion: '1',
  os: 'linux',
  arch: 'arm64',
);
const _pythonArgs = ['-m', 'http.server', '8080', '--directory', '/tmp'];

DockerKubeSource _inspect(Map<String, dynamic> fields) => DockerKubeSource.inspect(
  DockerContainerDetail(
    container: _container,
    inspect: {
      'HostConfig': {'NetworkMode': 'bridge'},
      'Mounts': <Object>[],
      ...fields,
    },
  ),
  _session,
);

DockerKubeTarget _target(List<String> startup) => DockerKubeTarget(
  name: 'dev-http',
  namespace: 'test',
  image: 'docker.io/library/python:3.14.4-slim-bookworm',
  port: 8080,
  startup: startup,
);

Map<String, dynamic> _containerSpec(Map<String, dynamic> deployment) {
  final spec = deployment['spec'] as Map<String, dynamic>;
  final template = spec['template'] as Map<String, dynamic>;
  final pod = template['spec'] as Map<String, dynamic>;
  return (pod['containers'] as List).single as Map<String, dynamic>;
}

void main() {
  late DockerKubeParser parser;
  setUp(() => parser = DockerKubeParser());
  tearDown(() => parser.cancel());

  test('the Python HTTP server process and TCP port survive the YAML and API round trip', () async {
    final source = _inspect({
      'Path': 'python',
      'Args': _pythonArgs,
      'Config': {
        'Image': _container.image,
        'Entrypoint': null,
        'Cmd': ['python', ..._pythonArgs],
        'Env': ['PASSWORD=never-copy-environment'],
      },
    });
    expect(source.blocker, isNull);
    expect(source.ports, [8080]);
    final target = _target(source.startup);
    expect(target.template, startsWith('apiVersion: apps/v1\nkind: Deployment\nmetadata:\n'));
    expect(target.template, contains('\n---\napiVersion: v1\nkind: Service'));
    expect(target.template, isNot(contains('never-copy-environment')));
    final manifests = await parser.parse(target.template, target);
    final pod = _containerSpec(jsonDecode(manifests!.first.manifest) as Map<String, dynamic>);
    expect(pod['command'], ['python']);
    expect(pod['args'], _pythonArgs);
    expect(pod['ports'], [
      {'containerPort': 8080, 'protocol': 'TCP'},
    ]);
    expect((pod['readinessProbe'] as Map)['tcpSocket'], {'port': 8080});
  });

  DockerKubeSource imageSource(String configured, List<String> repoDigests) => DockerKubeSource.inspect(
    DockerContainerDetail(
      container: _container,
      inspect: {
        'HostConfig': {'NetworkMode': 'bridge'},
        'Mounts': <Object>[],
        'Path': 'app',
        'Args': <String>[],
        'Config': {'Image': configured},
      },
    ),
    _session,
    repoDigests: repoDigests,
  );

  test('local-only tag and its equivalent Hub spellings cannot generate or validate a deployment', () async {
    final source = imageSource('local-stateless-api:local', []);
    expect(source.sourceImage, 'local-stateless-api:local');
    expect(source.image, isNull);
    for (final spelling in [
      'local-stateless-api:local',
      'library/local-stateless-api:local',
      'docker.io/local-stateless-api:local',
      'docker.io/library/local-stateless-api:local',
      'index.docker.io/library/local-stateless-api:local',
      'registry-1.docker.io:443/library/local-stateless-api:local',
    ]) {
      final target = DockerKubeTarget(name: 'api', namespace: 'test', image: spelling, port: 8080, source: source);
      expect(target.valid, isFalse, reason: spelling);
      expect(source.acceptsImage(spelling, pullSecret: 'regcred'), isFalse, reason: spelling);
      expect(await parser.parse(target.template, target), isNull);
      expect(await parser.mergeAi(target.aiTemplate, target), isNull);
    }
    final target = DockerKubeTarget(
      name: 'api',
      namespace: 'test',
      image: 'team/published-api:v2',
      port: 8080,
      source: source,
    );
    expect(target.valid, isTrue);
    final parsed = await parser.parse(target.template, target);
    expect(
      _containerSpec(jsonDecode(parsed!.first.manifest) as Map<String, dynamic>)['image'],
      'docker.io/team/published-api:v2',
    );
  });

  test('only a digest from the configured repository prefills Docker Hub and private images', () {
    final version = 'sha256:${'a' * 64}';
    final other = 'sha256:${'b' * 64}';
    final source = imageSource('python:3', ['unrelated/base@$other', 'python:local', 'python@$version']);
    expect(source.image, 'docker.io/library/python@$version');
    expect(source.acceptsImage('python:3'), isTrue);
    expect(imageSource('team/python:3', ['python@$version']).image, isNull);
    expect(
      imageSource('registry.example.org:5000/api:1', ['registry.example.org:5000/api@$version']).image,
      'registry.example.org:5000/api@$version',
    );
    expect(imageSource('local-stateless-api:local', ['python@$version']).image, isNull);
    expect(imageSource('sha256:${'c' * 64}', ['python@$version']).image, isNull);
  });

  test('fully qualified pins or named repositories can use explicit, node or default service account credentials', () {
    final version = 'sha256:${'c' * 64}';
    for (final input in [
      'registry.example.org/team/api:v1',
      'registry.example.org/team/api@$version',
      'registry.example.org/team/api:v1@$version',
    ]) {
      final source = imageSource(input, []);
      expect(source.image, isNull);
      expect(source.acceptsImage(input), isTrue);
      expect(source.acceptsImage(input, pullSecret: 'registry-auth'), isTrue);
      final target = DockerKubeTarget(
        name: 'api',
        namespace: 'test',
        port: 80,
        image: input,
        source: source,
        pullSecret: 'registry-auth',
      );
      expect(target.valid, isTrue);
      expect(_containerSpec(target.deployment)['image'], input);
    }
    final source = imageSource('team/api:v1', []);
    expect(source.acceptsImage('team/api:v1', pullSecret: 'registry-auth'), isFalse);
    expect(source.acceptsImage('docker.io/team/api:v1'), isTrue);
    expect(source.acceptsImage('docker.io/team/api:v1', pullSecret: 'registry-auth'), isTrue);
    final pinnedHub = imageSource('alpine@$version', []);
    expect(pinnedHub.acceptsImage('docker.io/library/alpine@$version'), isTrue);
    final fullHub = imageSource('docker.io/library/alpine:3', []);
    expect(fullHub.acceptsImage('docker.io/library/alpine:3', pullSecret: 'team.registry-auth'), isTrue);
    final observed = imageSource('python:3', ['python@$version']);
    expect(observed.resolveImage('python:3'), 'docker.io/library/python@$version');
    expect(imageSource('python@$version', ['python@sha256:${'d' * 64}']).image, isNull);
  });

  test('inspect uses the resolved process or combines config vectors without shell splitting', () {
    final config = {
      'Entrypoint': ['/entrypoint', 'one word'],
      'Cmd': ['serve', '', ' spaced '],
    };
    expect(_inspect({'Config': config}).startup, ['/entrypoint', 'one word', 'serve', '', ' spaced ']);
    expect(
      _inspect({
        'Config': config,
        'Path': 'runtime',
        'Args': ['arg'],
      }).startup,
      ['runtime', 'arg'],
    );
    expect(_inspect({'Path': '/run', 'Args': null}).startup, ['/run']);
    expect(
      _inspect({
        'Config': {
          'Entrypoint': [''],
          'Cmd': ['python', ..._pythonArgs],
        },
      }).startup,
      ['python', ..._pythonArgs],
    );
  });

  test('masked, malformed and missing startup vectors block migration', () {
    for (final fields in <Map<String, dynamic>>[
      {
        'Path': 'app',
        'Args': ['--token', dockerSecretMask],
      },
      {
        'Path': 'app',
        'Args': ['abc'],
        'Config': {
          'Cmd': ['--token=$dockerSecretMask'],
        },
      },
      {'Path': dockerSecretMask, 'Args': <String>[]},
      {
        'Path': 'app',
        'Args': ['ab\u0000cd'],
      },
      {
        'Path': 'app',
        'Args': [12],
      },
      {
        'Config': {'Cmd': 'python -m http.server'},
      },
      {
        'Config': {
          'Entrypoint': [''],
        },
      },
      {},
    ]) {
      expect(_inspect(fields).blocker, DockerKubeBlocker.startup);
      expect(_inspect(fields).startup, isEmpty);
    }
  });

  test('local YAML keeps argv boundaries, ambiguous YAML scalars and literal Docker dollars', () async {
    const args = [
      '',
      ' y ',
      'false',
      'on',
      '10',
      '-n',
      ':',
      '*label',
      '&anchor',
      'app && echo *x',
      'line\none\t"two": #comment',
      r'\path\to',
      r'$(HOME)',
      r'$$',
      '\u0085\u2028\u2029',
    ];
    final target = _target(['run', ...args]);
    final result = await parser.parse(target.template, target);
    final spec = _containerSpec(jsonDecode(result!.first.manifest) as Map<String, dynamic>);
    expect(spec['args'], [for (final arg in args) arg.replaceAll(r'$', r'$$')]);
  });

  test('AI never sees argv and only validated tuning reaches the local review', () async {
    final target = _target(['private-program', 'private-directory']);
    expect(target.aiTemplate, isNot(contains('command:')));
    expect(target.prompt, isNot(contains('private-program')));
    expect(target.prompt, isNot(contains('private-directory')));
    expect(target.prompt, endsWith(target.aiTemplate));
    final edited = target.aiTemplate.replaceFirst('128Mi', '256Mi');
    final merged = await parser.mergeAi(edited, target);
    expect(merged, isNotNull);
    expect(merged, contains('256Mi'));
    expect(merged, contains('private-program'));
    final result = await parser.parse(merged!, target);
    final spec = _containerSpec(jsonDecode(result!.first.manifest) as Map<String, dynamic>);
    expect(spec['command'], ['private-program']);
    expect(spec['args'], ['private-directory']);
    expect((spec['resources'] as Map)['requests'], {'cpu': '100m', 'memory': '256Mi'});
    expect(loadYamlDocuments(merged), hasLength(2));
    expect(await parser.mergeAi(target.template, target), isNull);
    expect(await parser.mergeAi(edited.replaceFirst('ClusterIP', 'NodePort'), target), isNull);
    expect(await parser.parse(edited, target), isNull);
    expect(await parser.parse(merged.replaceFirst('private-directory', '/other'), target), isNull);
  });
}
