import 'dart:convert';

import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/docker/docker_kube_deployment.dart';
import 'package:consolecrypt/docker/docker_kube_plan.dart';
import 'package:consolecrypt/docker/docker_service.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_manifest_service.dart';
import 'package:flutter_test/flutter_test.dart';

const target = DockerKubeTarget(name: 'web', namespace: 'test', image: 'docker.io/library/nginx:1.27', port: 80);
const container = DockerContainer(
  id: 'id',
  name: 'private-container-name',
  image: 'nginx:1.27',
  state: DockerContainerState.running,
  ports: [DockerPort(privatePort: 80, protocol: 'tcp')],
);
const session = DockerSessionInfo(
  id: 'docker',
  hostId: 'local',
  hostName: 'local',
  engineVersion: '1',
  os: 'linux',
  arch: 'arm64',
);

Map<String, dynamic> pod(Map<String, dynamic> deployment) =>
    ((deployment['spec'] as Map<String, dynamic>)['template'] as Map<String, dynamic>)['spec'] as Map<String, dynamic>;
Map<String, dynamic> app(Map<String, dynamic> deployment) =>
    (pod(deployment)['containers'] as List<dynamic>).first as Map<String, dynamic>;
String source(Map<String, dynamic> deployment, [Map<String, dynamic>? service]) =>
    '${jsonEncode(deployment)}\n---\n${jsonEncode(service ?? target.service)}';

class _Manifests implements KubeManifestService {
  final calls = <(String, bool)>[];
  final secrets = <SecretText>[];
  String? fail;
  void Function()? afterCreate;
  @override
  Future<KubeResourceDetail> createResource(
    String sessionId,
    KubeResourceRef resource,
    SecretText manifest, {
    bool dryRun = true,
  }) async {
    calls.add((resource.api.kind, dryRun));
    expect(resource.api.verbs, contains('create'));
    secrets.add(manifest);
    expect(jsonDecode(manifest.expose()), isA<Map<String, dynamic>>());
    if ('${resource.api.kind}/$dryRun' == fail) throw StateError('server rejected');
    if (!dryRun) afterCreate?.call();
    return const KubeResourceDetail(object: {}, yaml: '');
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

void main() {
  test('prefills Docker Hub names, tagged digests and registry references from Docker', () {
    expect(DockerKubeTarget.dockerImage('python:3.14.4-slim'), 'docker.io/library/python:3.14.4-slim');
    expect(DockerKubeTarget.dockerImage('nginx'), 'docker.io/library/nginx:latest');
    expect(DockerKubeTarget.dockerImage('team/image:1'), 'docker.io/team/image:1');
    expect(DockerKubeTarget.dockerImage('ghcr.io/org/image:2'), 'ghcr.io/org/image:2');
    final digest = 'sha256:${'a' * 64}';
    expect(DockerKubeTarget.dockerImage('python:3@$digest'), 'docker.io/library/python@$digest');
    for (final id in [digest, 'a' * 64, 'localhost:5000/image:2', 'https://ghcr.io/org/image']) {
      expect(DockerKubeTarget.dockerImage(id), isNull);
    }
  });

  test('requires explicit registry and version, rejects URLs, credentials and local image IDs', () {
    for (final image in [
      'nginx',
      'nginx:1',
      'team/web:1',
      'localhost:5000/web:1',
      '127.0.0.1:5000/web:1',
      'https://ghcr.io/team/web:1',
      'user:password@ghcr.io/team/web:1',
      'sha256:abcd',
      'ghcr.io/team/web',
      'ghcr.io/team/web:1\n',
    ]) {
      expect(DockerKubeTarget.validImage(image), isFalse, reason: image);
    }
    expect(DockerKubeTarget.validImage(target.image), isTrue);
    expect(DockerKubeTarget.validImage('ghcr.io/team/web@sha256:${'a' * 64}'), isTrue);
    expect(DockerKubeTarget.validImage('registry.example.org:5000/team/web:v1'), isTrue);
  });

  test('inspect whitelist exposes only TCP ports and blocks data/host dependencies', () {
    DockerKubeSource inspect(Map<String, dynamic> value) =>
        DockerKubeSource.inspect(DockerContainerDetail(container: container, inspect: value), session);
    final safe = {
      'HostConfig': <String, dynamic>{'NetworkMode': 'bridge'},
      'Mounts': <dynamic>[],
      'Config': {
        'Env': ['PASSWORD=never-send'],
        'Cmd': ['secret-command'],
        'Labels': {'secret': 'hidden'},
      },
    };
    final selected = inspect(safe);
    expect(selected.unsupported, isFalse);
    expect(selected.ports, [80]);
    expect(target.prompt, isNot(contains('never-send')));
    expect(target.prompt, isNot(contains(container.name)));
    expect(inspect({}).blocker, DockerKubeBlocker.unsupportedConfiguration);
    expect(
      inspect({
        ...safe,
        'Mounts': [
          {'Type': 'bind', 'Source': '/private', 'Destination': '/config/bot.env', 'RW': false},
        ],
      }).blocker,
      DockerKubeBlocker.mounts,
    );
    expect(
      inspect({
        ...safe,
        'HostConfig': {
          'Binds': ['/private:/config/bot.env:ro'],
        },
      }).blocker,
      DockerKubeBlocker.mounts,
    );
    for (final unsafe in <Map<String, dynamic>>[
      {
        ...safe,
        'HostConfig': {'Privileged': true},
      },
      {
        ...safe,
        'HostConfig': {'NetworkMode': 'host'},
      },
      {
        ...safe,
        'HostConfig': {'NetworkMode': 'container:other'},
      },
      {
        ...safe,
        'HostConfig': {
          'Devices': ['gpu'],
        },
      },
      {
        ...safe,
        'HostConfig': {
          'CapAdd': ['SYS_ADMIN'],
        },
      },
      {
        ...safe,
        'HostConfig': {'PidMode': 'host'},
      },
    ]) {
      expect(inspect(unsafe).blocker, DockerKubeBlocker.unsupportedConfiguration);
    }
  });

  group('untrusted YAML boundary', () {
    late DockerKubeParser parser;
    setUp(() => parser = DockerKubeParser());
    tearDown(() => parser.cancel());

    test('accepts template, fenced output, reordered documents and conservative resource edits', () async {
      final deployment = target.deployment;
      app(deployment)['resources'] = {
        'requests': {'cpu': '200m', 'memory': '256Mi'},
        'limits': {'cpu': '2', 'memory': '1Gi'},
      };
      final result = await parser.parse('```yaml\n${source(deployment)}\n```', target);
      expect(result, hasLength(2));
      expect(result!.first.resource.namespace, 'test');
      expect(result.first.manifest, contains('200m'));
      expect(
        await parser.parse('${jsonEncode(target.service)}\n---\n${jsonEncode(target.deployment)}', target),
        hasLength(2),
      );
      expect(await parser.parse(target.template, target), hasLength(2));
    });

    final attacks = <String, void Function(Map<String, dynamic>)>{
      'image substitution': (d) => app(d)['image'] = 'evil.example.org/backdoor:1',
      'namespace substitution': (d) => (d['metadata'] as Map<String, dynamic>)['namespace'] = 'production',
      'name substitution': (d) => (d['metadata'] as Map<String, dynamic>)['name'] = 'existing',
      'replica escalation': (d) => (d['spec'] as Map<String, dynamic>)['replicas'] = 100,
      'host network': (d) => pod(d)['hostNetwork'] = true,
      'host volumes': (d) => pod(d)['volumes'] = [
        {
          'hostPath': {'path': '/'},
        },
      ],
      'credential mounting': (d) => pod(d)['automountServiceAccountToken'] = true,
      'sidecar': (d) => (pod(d)['containers'] as List<dynamic>).add({'name': 'evil', 'image': target.image}),
      'init container': (d) => pod(d)['initContainers'] = [
        {'name': 'evil', 'image': target.image},
      ],
      'environment secret': (d) => app(d)['env'] = [
        {'name': 'TOKEN', 'value': 'private'},
      ],
      'command execution': (d) => app(d)['command'] = ['sh', '-c', 'curl evil'],
      'privilege escalation': (d) => app(d)['securityContext'] = {'privileged': true},
      'exec probe': (d) => app(d)['readinessProbe'] = {
        'exec': {
          'command': ['sh'],
        },
      },
      'selector escape': (d) => (d['spec'] as Map<String, dynamic>)['selector'] = {
        'matchLabels': {'app': 'other'},
      },
      'invalid quantity': (d) => app(d)['resources'] = {
        'requests': {'cpu': 'oops', 'memory': '128Mi'},
        'limits': {'cpu': '1', 'memory': '512Mi'},
      },
    };
    for (final attack in attacks.entries) {
      test('rejects ${attack.key}', () async {
        final deployment = target.deployment;
        attack.value(deployment);
        expect(await parser.parse(source(deployment), target), isNull);
      });
    }
    test('rejects external Service, extra resources, aliases and oversized answers', () async {
      final service = target.service;
      (service['spec'] as Map<String, dynamic>)['type'] = 'LoadBalancer';
      expect(await parser.parse(source(target.deployment, service), target), isNull);
      expect(await parser.parse('${target.template}\n---\nkind: Secret', target), isNull);
      expect(await parser.parse('a: &a [*a]\n---\nkind: Service', target), isNull);
      expect(await parser.parse('x' * 65537, target), isNull);
      expect(await parser.parse('not yaml [', target), isNull);
    });
    test('accepts regular YAML syntax', () async {
      // A full ordinary YAML pair, not just JSON accepted as YAML.
      expect(
        await parser.parse('''apiVersion: apps/v1
kind: Deployment
metadata: {name: web, namespace: test}
spec:
  replicas: 1
  selector: {matchLabels: {app.kubernetes.io/name: web}}
  template:
    metadata: {labels: {app.kubernetes.io/name: web}}
    spec:
      automountServiceAccountToken: false
      containers:
        - name: web
          image: docker.io/library/nginx:1.27
          imagePullPolicy: Always
          ports: [{containerPort: 80, protocol: TCP}]
          securityContext: {allowPrivilegeEscalation: false, capabilities: {drop: [ALL]}}
          resources: {requests: {cpu: 100m, memory: 128Mi}, limits: {cpu: '1', memory: 512Mi}}
          readinessProbe: {tcpSocket: {port: 80}, initialDelaySeconds: 5, periodSeconds: 5}
---
${jsonEncode(target.service)}''', target),
        hasLength(2),
      );
    });
  });

  group('create-only deployment', () {
    late _Manifests service;
    late DockerKubeDeployment deployment;
    late List<DockerKubeDocument> documents;
    var active = true;
    setUp(() async {
      service = _Manifests();
      active = true;
      deployment = DockerKubeDeployment(
        service: service,
        sessionId: 'owned-session',
        current: () => active,
        kinds: [
          for (final k in [DockerKubeParser.deploymentKind, DockerKubeParser.serviceKind])
            KubeResourceKind(
              group: k.group,
              version: k.version,
              resource: k.resource,
              kind: k.kind,
              namespaced: k.namespaced,
              verbs: const ['create', 'get', 'list'],
            ),
        ],
      );
      final parser = DockerKubeParser();
      documents = (await parser.parse(target.template, target))!;
      parser.cancel();
    });
    test('requires both server dry-runs then one explicit create attempt; wipes manifests', () async {
      await expectLater(deployment.deploy(onProgress: () {}), throwsStateError);
      await deployment.validate(documents);
      expect(service.calls, [('Deployment', true), ('Service', true)]);
      expect(deployment.created, isEmpty);
      await deployment.deploy(onProgress: () {});
      expect(service.calls, [('Deployment', true), ('Service', true), ('Deployment', false), ('Service', false)]);
      expect(deployment.created, hasLength(2));
      expect(service.secrets.every((s) => s.isWiped), isTrue);
      await expectLater(deployment.deploy(onProgress: () {}), throwsStateError);
    });
    test('failed second dry-run cannot authorize any writes', () async {
      service.fail = 'Service/true';
      await expectLater(deployment.validate(documents), throwsStateError);
      expect(deployment.validated, isFalse);
      await expectLater(deployment.deploy(onProgress: () {}), throwsStateError);
      expect(service.calls.every((c) => c.$2), isTrue);
    });
    test('partial failure preserves receipts and blocks overwrite retries', () async {
      await deployment.validate(documents);
      service.fail = 'Service/false';
      await expectLater(deployment.deploy(onProgress: () {}), throwsStateError);
      expect(deployment.created.map((r) => r.api.kind), ['Deployment']);
      await expectLater(deployment.validate(documents), throwsStateError);
      expect(service.secrets.every((s) => s.isWiped), isTrue);
    });
    test('lock/write-window expiry between writes stops before Service', () async {
      await deployment.validate(documents);
      service.afterCreate = () => active = false;
      await expectLater(deployment.deploy(onProgress: () {}), throwsStateError);
      expect(service.calls.where((c) => !c.$2), [('Deployment', false)]);
      expect(deployment.created, hasLength(1));
    });
    test('cancel or edit invalidates previous dry-run', () async {
      await deployment.validate(documents);
      deployment.invalidate();
      await expectLater(deployment.deploy(onProgress: () {}), throwsStateError);
      await deployment.validate(documents);
      deployment.cancel();
      await expectLater(deployment.deploy(onProgress: () {}), throwsStateError);
      expect(deployment.created, isEmpty);
    });
  });
}
