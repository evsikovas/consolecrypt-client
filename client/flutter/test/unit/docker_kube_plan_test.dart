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
