import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/docker/docker_kube_plan.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_manifest_service.dart';
import 'package:consolecrypt/kubernetes/kube_operations_service.dart';

enum DockerKubeRolloutState {
  ready,
  progressing,
  imagePull,
  crashLoop,
  terminated,
  unschedulable,
  containerConfig,
  deadlineExceeded,
  replicaFailure,
  unavailable,
  replaced,
}

final class DockerKubeRollout {
  const DockerKubeRollout(
    this.state, {
    this.image,
    this.reason,
    this.terminationReason,
    this.exitCode,
    this.restarts,
    this.registryDenied = false,
    this.podDetailsUnavailable = false,
  });
  final DockerKubeRolloutState state;
  final int? exitCode, restarts;
  final String? image, reason, terminationReason;
  final bool podDetailsUnavailable, registryDenied;
  bool get isProcessOrImageFailure =>
      state == DockerKubeRolloutState.imagePull ||
      state == DockerKubeRolloutState.crashLoop ||
      state == DockerKubeRolloutState.terminated;
}

/// Create-only execution. A partial deployment keeps explicit receipts;
/// retries never replace existing resources or delete somebody else's work.
final class DockerKubeDeployment {
  DockerKubeDeployment({required this.service, required this.sessionId, required this.current, required this.kinds});
  final KubeManifestService service;
  final String sessionId;
  final bool Function() current;
  final List<KubeResourceKind> kinds;
  final created = <KubeResourceRef>[];
  String? _deploymentUid;
  List<DockerKubeDocument>? _validated;
  bool _cancelled = false;
  bool get validated => _validated != null && !_cancelled;
  void invalidate() => _validated = null;
  void cancel() {
    _cancelled = true;
    invalidate();
  }

  void _check() {
    if (_cancelled || !current()) throw StateError('Deployment scope expired');
  }

  Future<void> validate(List<DockerKubeDocument> documents) async {
    invalidate();
    if (created.isNotEmpty || documents.length != 2) throw StateError('Already started');
    // Native verifies the complete discovery record, including verbs. Never
    // send the parser's identity-only constants to the native resource API.
    final resolved = documents.map((document) {
      final identity = document.resource;
      final kind = kinds.singleWhere(
        (k) =>
            k.id == identity.api.id &&
            k.kind == identity.api.kind &&
            k.namespaced == identity.api.namespaced &&
            k.verbs.contains('create'),
      );
      return DockerKubeDocument(
        KubeResourceRef(api: kind, name: identity.name, namespace: identity.namespace),
        document.manifest,
      );
    }).toList();
    for (final document in resolved) {
      _check();
      await _create(document, dryRun: true);
      _check();
    }
    _validated = List.unmodifiable(resolved);
  }

  Future<void> deploy({required void Function() onProgress}) async {
    _check();
    final documents = _validated;
    invalidate(); // One explicit attempt per server validation.
    if (documents == null || created.isNotEmpty) throw StateError('Dry-run required');
    for (final document in documents) {
      _check();
      final result = await _create(document, dryRun: false);
      created.add(document.resource);
      if (document.resource.api.id == DockerKubeParser.deploymentKind.id) {
        _deploymentUid = _resourceUid(result, document.resource);
      }
      if (current()) onProgress();
      _check();
    }
  }

  Future<KubeResourceDetail> _create(DockerKubeDocument document, {required bool dryRun}) async {
    final manifest = SecretText(document.manifest);
    try {
      return await service.createResource(sessionId, document.resource, manifest, dryRun: dryRun);
    } finally {
      manifest.wipe();
    }
  }

  Future<DockerKubeRollout> readRollout(KubeBrowserService browser, {required bool Function() current}) async {
    // Reads remain available after the production write window expires.
    void checkRead() {
      if (_cancelled || !current()) throw StateError('Deployment scope expired');
    }

    checkRead();
    final ref = created.where((ref) => ref.api.id == DockerKubeParser.deploymentKind.id).firstOrNull;
    final uid = _deploymentUid;
    const unavailable = DockerKubeRollout(DockerKubeRolloutState.unavailable);
    if (ref == null || uid == null || !ref.api.canRead) return unavailable;
    final KubeResourceDetail detail;
    try {
      detail = await browser.getResource(sessionId, ref);
    } catch (_) {
      checkRead();
      return unavailable;
    }
    checkRead();
    final observedUid = _resourceUid(detail, ref);
    if (observedUid == null) return unavailable;
    if (observedUid != uid) return const DockerKubeRollout(DockerKubeRolloutState.replaced);
    final controller = _deploymentState(detail.object);
    if (controller == DockerKubeRolloutState.ready) return DockerKubeRollout(controller);
    final fallback = DockerKubeRollout(controller, podDetailsUnavailable: true);
    if (browser is! KubeOperationsService) return fallback;
    final replicas = kinds.where((k) => k.id == 'apps/v1/replicasets' && k.namespaced && k.canRead).firstOrNull;
    if (replicas == null) return fallback;
    try {
      final pods = await (browser as KubeOperationsService).workloadPods(sessionId, ref);
      checkRead();
      // workloadPods selects by labels. Resolve the controlling ReplicaSet and
      // verify both owner UIDs before attributing a Pod's failure to this run.
      final owners = <(String, String), bool>{};
      var partial = false;
      for (final pod in pods) {
        final meta = pod.object['metadata'];
        if (meta is! Map || meta['namespace'] != ref.namespace || meta['deletionTimestamp'] != null) continue;
        final problem = _podProblem(pod.object);
        if (problem == null) continue;
        final owner = _controller(meta, 'ReplicaSet');
        if (owner == null) continue;
        if (!owners.containsKey(owner)) {
          // Bound requests even when unrelated Pods match the workload selector.
          if (owners.length >= 8) return fallback;
          final rsRef = KubeResourceRef(api: replicas, namespace: ref.namespace, name: owner.$1);
          final KubeResourceDetail rs;
          try {
            rs = await browser.getResource(sessionId, rsRef);
          } catch (_) {
            checkRead();
            partial = true;
            owners[owner] = false;
            continue;
          }
          checkRead();
          final rsMeta = rs.object['metadata'];
          owners[owner] =
              rsMeta is Map &&
              _resourceUid(rs, rsRef) == owner.$2 &&
              rsMeta['deletionTimestamp'] == null &&
              _controller(rsMeta, 'Deployment') == (ref.name, uid);
        }
        if (owners[owner]!) return problem;
      }
      return partial ? fallback : DockerKubeRollout(controller);
    } catch (_) {
      checkRead();
      return fallback;
    }
  }
}

String? _resourceUid(KubeResourceDetail detail, KubeResourceRef ref) {
  final metadata = detail.object['metadata'];
  if (metadata is! Map<String, dynamic> || metadata['name'] != ref.name || metadata['namespace'] != ref.namespace) {
    return null;
  }
  final uid = metadata['uid'];
  return uid is String && uid.isNotEmpty ? uid : null;
}

(String, String)? _controller(Map<dynamic, dynamic> metadata, String kind) {
  final owners = metadata['ownerReferences'];
  if (owners is List) {
    for (final owner in owners.whereType<Map<String, dynamic>>()) {
      final name = owner['name'], uid = owner['uid'];
      if (owner['controller'] == true &&
          owner['kind'] == kind &&
          owner['apiVersion'] == 'apps/v1' &&
          name is String &&
          name.isNotEmpty &&
          uid is String &&
          uid.isNotEmpty) {
        return (name, uid);
      }
    }
  }
  return null;
}

DockerKubeRolloutState _deploymentState(Map<String, dynamic> object) {
  final metadata = object['metadata'] as Map<String, dynamic>;
  final spec = object['spec'];
  final status = object['status'];
  if (metadata['deletionTimestamp'] != null || status is! Map) return DockerKubeRolloutState.progressing;
  final generation = metadata['generation'];
  if (generation is! int || generation != status['observedGeneration']) return DockerKubeRolloutState.progressing;
  final desired = spec is Map ? spec['replicas'] : null;
  if (desired is int &&
      desired > 0 &&
      ['replicas', 'updatedReplicas', 'readyReplicas', 'availableReplicas'].every((k) => status[k] == desired)) {
    return DockerKubeRolloutState.ready;
  }
  final conditions = status['conditions'];
  if (conditions is List) {
    for (final condition in conditions.whereType<Map<String, dynamic>>()) {
      if (condition['type'] == 'Progressing' &&
          condition['status'] == 'False' &&
          condition['reason'] == 'ProgressDeadlineExceeded') {
        return DockerKubeRolloutState.deadlineExceeded;
      }
      if (condition['type'] == 'ReplicaFailure' && condition['status'] == 'True') {
        return DockerKubeRolloutState.replicaFailure;
      }
    }
  }
  return DockerKubeRolloutState.progressing;
}

DockerKubeRollout? _podProblem(Map<String, dynamic> pod) {
  final status = pod['status'];
  if (status is! Map) return null;
  final conditions = status['conditions'];
  if (conditions is List &&
      conditions.whereType<Map<String, dynamic>>().any(
        (c) => c['type'] == 'PodScheduled' && c['status'] == 'False' && c['reason'] == 'Unschedulable',
      )) {
    return const DockerKubeRollout(DockerKubeRolloutState.unschedulable);
  }
  for (final group in ['initContainerStatuses', 'containerStatuses']) {
    final entries = status[group];
    if (entries is! List) continue;
    for (final entry in entries.whereType<Map<String, dynamic>>()) {
      final state = entry['state'];
      if (state is! Map) continue;
      final rawImage = entry['image'];
      final image = rawImage is String && DockerKubeTarget.dockerImage(rawImage) != null ? rawImage : null;
      final restarts = entry['restartCount'];
      DockerKubeRollout termination(DockerKubeRolloutState status, Object? terminated, {String? reason}) {
        final code = terminated is Map ? terminated['exitCode'] : null;
        final cause = terminated is Map ? terminated['reason'] : null;
        // Runtime status messages can contain arguments and credentials. Use only known reason codes.
        const known = {'OOMKilled', 'Error', 'Completed', 'ContainerCannotRun', 'StartError', 'DeadlineExceeded'};
        return DockerKubeRollout(
          status,
          image: image,
          reason: reason,
          exitCode: code is int ? code : null,
          restarts: restarts is int && restarts >= 0 ? restarts : null,
          terminationReason: cause is String && known.contains(cause) ? cause : null,
        );
      }

      final waiting = state['waiting'];
      if (waiting is Map) {
        switch (waiting['reason']) {
          case 'ImagePullBackOff' || 'ErrImagePull' || 'InvalidImageName':
            final message = waiting['message'];
            return DockerKubeRollout(
              DockerKubeRolloutState.imagePull,
              image: image,
              reason: waiting['reason'] as String,
              registryDenied:
                  message is String &&
                  RegExp(
                    r'pull access denied|denied: requested access|unauthorized: authentication required|no basic auth credentials',
                    caseSensitive: false,
                  ).hasMatch(message),
            );
          case 'CreateContainerConfigError' || 'CreateContainerError' || 'RunContainerError':
            return const DockerKubeRollout(DockerKubeRolloutState.containerConfig);
          case 'CrashLoopBackOff':
            final last = entry['lastState'];
            return termination(
              DockerKubeRolloutState.crashLoop,
              last is Map ? last['terminated'] : null,
              reason: 'CrashLoopBackOff',
            );
        }
      }
      final terminated = state['terminated'];
      if (terminated is Map) {
        final code = terminated['exitCode'];
        if (group == 'initContainerStatuses' && code == 0) continue;
        return termination(DockerKubeRolloutState.terminated, terminated);
      }
    }
  }
  return null;
}
