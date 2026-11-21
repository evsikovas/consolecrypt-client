import 'dart:convert';

import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/docker/docker_kube_deployment.dart';
import 'package:consolecrypt/docker/docker_kube_plan.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_manifest_service.dart';
import 'package:consolecrypt/kubernetes/kube_operations_service.dart';
import 'package:flutter_test/flutter_test.dart';

const _deploy = KubeResourceKind(
  group: 'apps',
  version: 'v1',
  resource: 'deployments',
  kind: 'Deployment',
  namespaced: true,
  verbs: ['create', 'get'],
);
const _service = KubeResourceKind(
  group: '',
  version: 'v1',
  resource: 'services',
  kind: 'Service',
  namespaced: true,
  verbs: ['create', 'get'],
);
const _rs = KubeResourceKind(
  group: 'apps',
  version: 'v1',
  resource: 'replicasets',
  kind: 'ReplicaSet',
  namespaced: true,
  verbs: ['get'],
);
const _target = DockerKubeTarget(name: 'app', namespace: 'work', image: 'example.org/web:v1', port: 8080);
const _ready = <String, dynamic>{
  'observedGeneration': 1,
  'replicas': 1,
  'updatedReplicas': 1,
  'readyReplicas': 1,
  'availableReplicas': 1,
};

Map<String, dynamic> _owner(String kind, String name, String uid) => {
  'apiVersion': 'apps/v1',
  'kind': kind,
  'name': name,
  'uid': uid,
  'controller': true,
};
KubeResourceRow _pod(Map<String, dynamic> status, {String namespace = 'work', String rsUid = 'rs-uid'}) =>
    KubeResourceRow(
      name: 'app-pod',
      uid: 'pod-uid',
      resourceVersion: '42',
      namespace: namespace,
      object: {
        'metadata': {
          'name': 'app-pod',
          'namespace': namespace,
          'uid': 'pod-uid',
          'ownerReferences': [_owner('ReplicaSet', 'app-rs', rsUid)],
        },
        'status': status,
      },
    );

class _Cluster implements KubeBrowserService, KubeManifestService, KubeOperationsService {
  final reads = <String>[];
  final pods = <KubeResourceRow>[];
  Map<String, dynamic> status = {};
  String? receiptUid = 'deployment-uid';
  String deploymentUid = 'deployment-uid', rsParent = 'deployment-uid';
  bool denyDeployment = false, denyPods = false, denyServiceCreate = false;
  void Function()? afterPods;

  @override
  Future<KubeResourceDetail> createResource(
    String sessionId,
    KubeResourceRef resource,
    SecretText manifest, {
    bool dryRun = true,
  }) async {
    if (resource.api == _service && !dryRun && denyServiceCreate) throw StateError('conflict');
    return KubeResourceDetail(
      object: {
        'metadata': {'namespace': resource.namespace, 'name': resource.name, 'uid': dryRun ? 'dry-uid' : receiptUid},
      },
      yaml: '',
    );
  }

  @override
  Future<KubeResourceDetail> getResource(String sessionId, KubeResourceRef resource) async {
    expect(sessionId, 'session');
    expect(resource.namespace, 'work');
    reads.add(resource.api.kind);
    if (resource.api == _deploy) {
      if (denyDeployment) throw StateError('forbidden');
      return KubeResourceDetail(
        object: {
          'metadata': {'namespace': 'work', 'name': 'app', 'uid': deploymentUid, 'generation': 1},
          'spec': {'replicas': 1},
          'status': status,
        },
        yaml: '',
      );
    }
    expect(resource.api, _rs);
    expect(resource.name, 'app-rs');
    return KubeResourceDetail(
      object: {
        'metadata': {
          'namespace': 'work',
          'name': 'app-rs',
          'uid': 'rs-uid',
          'ownerReferences': [_owner('Deployment', 'app', rsParent)],
        },
      },
      yaml: '',
    );
  }

  @override
  Future<List<KubeResourceRow>> workloadPods(String sessionId, KubeResourceRef resource) async {
    expect(sessionId, 'session');
    expect(resource.api, _deploy);
    reads.add('Pods');
    if (denyPods) throw StateError('forbidden');
    afterPods?.call();
    return pods;
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

void main() {
  late _Cluster cluster;
  late DockerKubeDeployment deployment;
  var write = true, scope = true;
  setUp(() {
    cluster = _Cluster();
    write = true;
    scope = true;
    deployment = DockerKubeDeployment(
      service: cluster,
      sessionId: 'session',
      current: () => write,
      kinds: const [_deploy, _service, _rs],
    );
  });
  Future<void> create({bool serviceFirst = false}) async {
    final documents = [
      DockerKubeDocument(
        const KubeResourceRef(api: _deploy, namespace: 'work', name: 'app'),
        jsonEncode(_target.deployment),
      ),
      DockerKubeDocument(
        const KubeResourceRef(api: _service, namespace: 'work', name: 'app'),
        jsonEncode(_target.service),
      ),
    ];
    await deployment.validate(serviceFirst ? documents.reversed.toList() : documents);
    await deployment.deploy(onProgress: () {});
  }

  Future<DockerKubeRollout> read() => deployment.readRollout(cluster, current: () => scope);
  test(
    'reads the Deployment receipt by kind after write access expires, even when Service was created first',
    () async {
      await create(serviceFirst: true);
      write = false;
      cluster.status = _ready;
      expect((await read()).state, DockerKubeRolloutState.ready);
      expect(cluster.reads, ['Deployment']);
    },
  );
  test('same-name replacement never contributes readiness or Pod diagnoses', () async {
    await create();
    cluster.deploymentUid = 'recreated';
    cluster.status = _ready;
    expect((await read()).state, DockerKubeRolloutState.replaced);
    expect(cluster.reads, ['Deployment']);
  });
  test('a missing creation UID cannot be recovered from dry-run or adopted by name', () async {
    cluster.receiptUid = null;
    await create();
    expect(deployment.created, hasLength(2));
    expect((await read()).state, DockerKubeRolloutState.unavailable);
    expect(cluster.reads, isEmpty);
  });
  test('requires the current generation and all updated replicas', () async {
    await create();
    for (final change in [
      {'observedGeneration': 0},
      {'replicas': 2},
      {'updatedReplicas': 0},
      {'availableReplicas': 0},
    ]) {
      cluster.status = {..._ready, ...change};
      expect((await read()).state, DockerKubeRolloutState.progressing);
    }
  });
  test('a partial create preserves status checks and permits retrying failed reads', () async {
    cluster.denyServiceCreate = true;
    await expectLater(create(), throwsStateError);
    expect(deployment.created.map((r) => r.api.kind), ['Deployment']);
    cluster.denyDeployment = true;
    expect((await read()).state, DockerKubeRolloutState.unavailable);
    cluster.denyDeployment = false;
    cluster.status = _ready;
    expect((await read()).state, DockerKubeRolloutState.ready);
  });
  for (final (state, last, expected, code) in [
    (
      {
        'waiting': {'reason': 'ErrImagePull'},
      },
      null,
      DockerKubeRolloutState.imagePull,
      null,
    ),
    (
      {
        'waiting': {'reason': 'CreateContainerConfigError'},
      },
      null,
      DockerKubeRolloutState.containerConfig,
      null,
    ),
    (
      {
        'waiting': {'reason': 'CrashLoopBackOff'},
      },
      {
        'terminated': {'exitCode': 0},
      },
      DockerKubeRolloutState.crashLoop,
      0,
    ),
    (
      {
        'terminated': {'exitCode': 0},
      },
      null,
      DockerKubeRolloutState.terminated,
      0,
    ),
    (
      {
        'terminated': {'exitCode': 137},
      },
      null,
      DockerKubeRolloutState.terminated,
      137,
    ),
    (
      {'running': <String, dynamic>{}},
      {
        'terminated': {'exitCode': 1},
      },
      DockerKubeRolloutState.progressing,
      null,
    ),
  ]) {
    test('reports only the verified current container state $expected with exit code $code', () async {
      await create();
      cluster.pods.add(
        _pod({
          'containerStatuses': [
            {'state': state, 'lastState': last},
          ],
        }),
      );
      final result = await read();
      expect(result.state, expected);
      expect(result.exitCode, code);
    });
  }
  test('reports Pod scheduling conditions and ignores successfully completed init containers', () async {
    await create();
    cluster.pods.add(
      _pod({
        'conditions': [
          {'type': 'PodScheduled', 'status': 'False', 'reason': 'Unschedulable'},
        ],
      }),
    );
    expect((await read()).state, DockerKubeRolloutState.unschedulable);
    cluster.pods
      ..clear()
      ..add(
        _pod({
          'initContainerStatuses': [
            {
              'state': {
                'terminated': {'exitCode': 0},
              },
            },
          ],
        }),
      );
    expect((await read()).state, DockerKubeRolloutState.progressing);
  });
  test('labels do not prove ownership across namespaces, replacement ReplicaSets or other Deployments', () async {
    await create();
    final status = {
      'containerStatuses': [
        {
          'state': {
            'waiting': {'reason': 'ImagePullBackOff'},
          },
        },
      ],
    };
    cluster.pods.add(_pod(status, namespace: 'other'));
    cluster.pods.add(_pod(status, rsUid: 'previous-rs'));
    expect((await read()).state, DockerKubeRolloutState.progressing);
    expect(cluster.reads, ['Deployment', 'Pods', 'ReplicaSet']);
    cluster.pods.add(_pod(status));
    cluster.rsParent = 'other-deployment';
    expect((await read()).state, DockerKubeRolloutState.progressing);
  });
  test('insufficient Pod rights preserve current Deployment conditions; unknown details can be retried', () async {
    await create();
    cluster.denyPods = true;
    final pending = await read();
    expect(pending.state, DockerKubeRolloutState.progressing);
    expect(pending.podDetailsUnavailable, isTrue);
    for (final (condition, expected) in [
      ({'type': 'ReplicaFailure', 'status': 'True'}, DockerKubeRolloutState.replicaFailure),
      (
        {'type': 'Progressing', 'status': 'False', 'reason': 'ProgressDeadlineExceeded'},
        DockerKubeRolloutState.deadlineExceeded,
      ),
    ]) {
      cluster.status = {
        'observedGeneration': 1,
        'conditions': [condition],
      };
      expect((await read()).state, expected);
    }
    cluster.status = _ready;
    expect((await read()).state, DockerKubeRolloutState.ready);
  });
  test('revoking the session during Pod lookup prevents further requests or stale results', () async {
    await create();
    cluster.afterPods = () => scope = false;
    cluster.pods.add(
      _pod({
        'containerStatuses': [
          {
            'state': {
              'waiting': {'reason': 'ImagePullBackOff'},
            },
          },
        ],
      }),
    );
    await expectLater(read(), throwsStateError);
    expect(cluster.reads, ['Deployment', 'Pods']);
    scope = true;
    deployment.cancel();
    await expectLater(read(), throwsStateError);
    expect(cluster.reads, ['Deployment', 'Pods']);
  });
}
