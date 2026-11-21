import 'dart:async';

import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/mock/mock_docker.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/models/saved_forward.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/docker/docker_providers.dart';
import 'package:consolecrypt/docker/docker_service.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_forward_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:consolecrypt/l10n/app_localizations_ru.dart';
import 'package:consolecrypt/tunnels/saved_forward_tile.dart';
import 'package:consolecrypt/tunnels/saved_forwards.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import '../helpers/test_app.dart';

class _Docker extends MockDockerService {
  final targets = <String>[];
  Completer<DockerSessionInfo>? pendingConnect;
  @override
  Future<DockerSessionInfo> connect(String hostId) => pendingConnect?.future ?? super.connect(hostId);
  @override
  Future<DockerForward> startForward(
    String sessionId,
    String containerId, {
    required int remotePort,
    int localPort = 0,
  }) {
    targets.add(containerId);
    return super.startForward(sessionId, containerId, remotePort: remotePort, localPort: localPort);
  }
}

class _Kube implements KubeService, KubeBrowserService, KubeForwardService, KubeNetworkForwardService {
  bool production = false, allow = true;
  int starts = 0, networkStarts = 0, disconnects = 0;
  final running = <KubeForward>[];
  @override
  Future<KubeSession> connect(String contextId) async => KubeSession(
    id: 'session',
    contextId: contextId,
    serverVersion: '1.32',
    production: production,
    allowPortForwardInProd: allow,
  );
  @override
  Future<void> disconnect(String sessionId) async {
    disconnects++;
    running.clear();
  }

  @override
  Future<List<KubeForward>> forwards() async => List.of(running);
  @override
  Future<KubeForward> startForward(
    String sessionId, {
    required String namespace,
    required String pod,
    required int remotePort,
    int localPort = 0,
  }) async {
    starts++;
    final f = KubeForward(
      id: 'forward-$starts',
      sessionId: sessionId,
      contextName: 'cluster',
      namespace: namespace,
      pod: pod,
      remotePort: remotePort,
      address: '127.0.0.1:${localPort == 0 ? 45678 : localPort}',
      state: 'running',
    );
    running.add(f);
    return f;
  }

  @override
  Future<KubeForward> startNetworkForward(
    String sessionId, {
    required String namespace,
    required String pod,
    required int remotePort,
    required String localAddress,
    required bool approveNetwork,
    int localPort = 0,
  }) {
    networkStarts++;
    return startForward(sessionId, namespace: namespace, pod: pod, remotePort: remotePort, localPort: localPort);
  }

  @override
  Future<void> stopForward(String sessionId, String forwardId) async => running.removeWhere((f) => f.id == forwardId);
  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  setUp(() => debugDefaultTargetPlatformOverride = TargetPlatform.windows);
  tearDown(() => debugDefaultTargetPlatformOverride = null);
  final profile = Profile(
    id: const ProfileId('profile-a'),
    name: 'A',
    kind: ProfileKind.local,
    createdAt: DateTime.utc(2026),
  );
  final other = Profile(
    id: const ProfileId('profile-b'),
    name: 'B',
    kind: ProfileKind.local,
    createdAt: DateTime.utc(2026),
  );
  const unlocked = VaultStatus(phase: VaultPhase.unlocked);
  SavedForward plan({
    SavedForwardKind kind = SavedForwardKind.docker,
    String address = '127.0.0.1',
    String owner = 'profile-a',
  }) => SavedForward(
    id: 'preset',
    profile: owner,
    kind: kind,
    connectionId: kind == SavedForwardKind.docker ? 'local' : 'cluster',
    target: kind == SavedForwardKind.docker ? 'shop-postgres-1' : 'postgres-0',
    namespace: kind == SavedForwardKind.docker ? '' : 'default',
    remotePort: 5432,
    localPort: 0,
    localAddress: address,
  );

  test('presets round-trip without credentials or runtime handles; invalid values ignored', () {
    final original = LocalSettings(
      savedForwards: [
        plan(),
        plan(kind: SavedForwardKind.kubernetes),
      ],
      clipboardClearSeconds: 60,
    );
    final json = localSettingsToJson(original);
    final restored = localSettingsFromJson(json);
    expect(restored.savedForwards.map((p) => p.toJson()), original.savedForwards.map((p) => p.toJson()));
    expect(restored.clipboardClearSeconds, 60);
    expect(localSettingsFromJson({}).savedForwards, isEmpty);
    expect(
      localSettingsFromJson({
        'saved_forwards': [
          null,
          <String, Object?>{},
          {...plan().toJson(), 'local_port': -1},
        ],
      }).savedForwards,
      isEmpty,
    );
    expect(json['saved_forwards'].toString(), isNot(contains('session')));
    expect(json['saved_forwards'].toString(), isNot(contains('password')));
  });

  test('occupied local endpoint surfaces its address for both native error codes', () {
    final l = AppLocalizationsRu();
    for (final (kind, rule) in [
      (SavedForwardKind.docker, 'local_port_busy'),
      (SavedForwardKind.kubernetes, 'local_bind_failed'),
    ]) {
      final saved = SavedForward.tryFromJson({...plan(kind: kind).toJson(), 'local_port': 8200})!;
      final error = AppException(
        AppErrorCode.validation,
        'diagnostic',
        reason: AppErrorReason.invalidField,
        args: {'field': kind.name, 'rule': rule},
      );
      expect(savedForwardErrorText(l, saved, error), l.tunnelsLocalAddressUnavailable('127.0.0.1:8200'));
    }
  });

  test('Docker saved once, restored by current container name and can stop/restart/remove', () async {
    final backend = testBackend(), docker = _Docker();
    final c = ProviderContainer(
      overrides: [
        appServicesProvider.overrideWithValue(backend.services),
        activeProfileProvider.overrideWithValue(profile),
        vaultStatusProvider.overrideWith((ref) => Stream.value(unlocked)),
        dockerServiceProvider.overrideWithValue(docker),
      ],
    );
    addTearDown(() async {
      c.dispose();
      await backend.dispose();
    });
    c.listen(savedForwardsProvider, (_, _) {});
    await c.read(vaultStatusProvider.future);
    final m = c.read(savedForwardManagerProvider);
    final saved = await m.save(plan());
    await m.save(
      SavedForward(
        id: 'duplicate',
        profile: saved.profile,
        kind: saved.kind,
        connectionId: saved.connectionId,
        target: saved.target,
        remotePort: saved.remotePort,
        localPort: saved.localPort,
      ),
    );
    expect(backend.services.settings.currentLocal.savedForwards, hasLength(1));
    await m.start(saved);
    expect(docker.targets, [saved.target]);
    expect(m.handles[saved.id]!.address, startsWith('127.0.0.1:'));
    await m.start(saved);
    expect(docker.targets, hasLength(1));
    await docker.stopForward(m.handles[saved.id]!.forwardId);
    await m.refresh();
    expect(m.handles, isEmpty);
    await m.start(saved);
    expect(docker.targets, hasLength(2));
    await m.stop(saved);
    expect(await docker.forwards(), isEmpty);
    expect(backend.services.settings.currentLocal.savedForwards, hasLength(1));
    await m.remove(saved);
    expect(backend.services.settings.currentLocal.savedForwards, isEmpty);
  });

  test('Kubernetes restart rechecks production policy and explicit network approval', () async {
    final backend = testBackend(), kube = _Kube();
    final c = ProviderContainer(
      overrides: [
        appServicesProvider.overrideWithValue(backend.services),
        activeProfileProvider.overrideWithValue(profile),
        vaultStatusProvider.overrideWith((ref) => Stream.value(unlocked)),
        kubeServiceProvider.overrideWithValue(kube),
      ],
    );
    addTearDown(() async {
      c.dispose();
      await backend.dispose();
    });
    c.listen(savedForwardsProvider, (_, _) {});
    await c.read(vaultStatusProvider.future);
    final m = c.read(savedForwardManagerProvider), saved = plan(kind: SavedForwardKind.kubernetes);
    await m.save(saved);
    await m.start(saved);
    expect(kube.starts, 1);
    await m.stop(saved);
    kube.production = true;
    kube.allow = false;
    await expectLater(m.start(saved), throwsA(isA<AppException>()));
    expect(kube.starts, 1);
    kube.production = false;
    final network = plan(kind: SavedForwardKind.kubernetes, address: '0.0.0.0');
    await expectLater(m.start(network), throwsA(isA<AppException>()));
    expect(kube.networkStarts, 0);
    await m.start(network, approveNetwork: true);
    expect(kube.networkStarts, 1);
    await m.stop(network);
    kube.production = true;
    kube.allow = true;
    await expectLater(m.start(network, approveNetwork: true), throwsA(isA<AppException>()));
    expect(kube.networkStarts, 1);
  });

  test('fixed-port presets from settings reuse only a live forward to the saved host or context', () async {
    final backend = testBackend(), docker = _Docker(), kube = _Kube();
    final c = ProviderContainer(
      overrides: [
        appServicesProvider.overrideWithValue(backend.services),
        activeProfileProvider.overrideWithValue(profile),
        vaultStatusProvider.overrideWith((ref) => Stream.value(unlocked)),
        dockerServiceProvider.overrideWithValue(docker),
        kubeServiceProvider.overrideWithValue(kube),
      ],
    );
    addTearDown(() async {
      c.dispose();
      await backend.dispose();
    });
    final settings = localSettingsFromJson({
      'saved_forwards': [
        {
          'id': 'a',
          'kind': 'docker',
          'profile': 'profile-a',
          'connection_id': 'ssh-web',
          'target': 'shop-postgres-1',
          'namespace': '',
          'remote_port': 5432,
          'local_address': '127.0.0.1',
          'local_port': 44442,
        },
        {
          'id': 'b',
          'kind': 'kubernetes',
          'profile': 'profile-a',
          'connection_id': 'cluster',
          'namespace': 'vault',
          'target': 'vault-0',
          'remote_port': 8200,
          'local_address': '127.0.0.1',
          'local_port': 8200,
        },
      ],
    });
    await backend.services.settings.updateLocal(settings);
    c.listen(savedForwardsProvider, (_, _) {});
    await c.read(vaultStatusProvider.future);
    final manager = c.read(savedForwardManagerProvider);
    final dockerPlan = settings.savedForwards.first, kubePlan = settings.savedForwards.last;
    final wrongHost = await docker.connect('local');
    await docker.startForward(wrongHost.id, dockerPlan.target, remotePort: 5432, localPort: 44442);
    KubeForward nativeForward(String contextId) => KubeForward.fromJson({
      'id': 'pod-forward-$contextId',
      'session_id': 'browser',
      'context_id': contextId,
      'context_name': 'same display name',
      'namespace': 'vault',
      'pod': 'vault-0',
      'remote_port': 8200,
      'address': '127.0.0.1:8200',
      'state': 'running',
    });
    kube.running.add(nativeForward('other-cluster'));
    await manager.refresh();
    expect(manager.handles, isEmpty);

    await docker.disconnect(wrongHost.id);
    final host = await docker.connect('ssh-web');
    final existing = await docker.startForward(host.id, dockerPlan.target, remotePort: 5432, localPort: 44442);
    kube.running
      ..clear()
      ..add(nativeForward('cluster'));
    await manager.start(dockerPlan);
    await manager.refresh();
    expect(manager.handles['a']!.forwardId, existing.id);
    expect(manager.handles['b']!.forwardId, 'pod-forward-cluster');
    expect(manager.handles.values.every((h) => !h.owned), isTrue);
    expect(await docker.forwards(), hasLength(1));
    final autoPlan = SavedForward.tryFromJson({...dockerPlan.toJson(), 'id': 'auto', 'local_port': 0})!;
    final automatic = await docker.startForward(host.id, dockerPlan.target, remotePort: 5432);
    await manager.start(autoPlan);
    expect(manager.handles['auto']!.forwardId, automatic.id);
    await manager.stop(autoPlan);
    await manager.stop(dockerPlan);
    await manager.stop(kubePlan);
    expect(await docker.containers(host.id), isNotEmpty);
    expect(kube.disconnects, 0);
    await manager.start(dockerPlan);
    await manager.start(kubePlan);
    expect(manager.handles['a']!.address, '127.0.0.1:44442');
    expect(manager.handles['b']!.address, '127.0.0.1:8200');
    expect(kube.running.single.pod, 'vault-0');
    expect(kube.running.single.namespace, 'vault');
    expect(manager.handles.values.every((h) => h.owned), isTrue);
  });

  test('profile transition hides presets and revokes an in-flight restart', () async {
    final backend = testBackend(), docker = _Docker();
    final c = ProviderContainer(
      overrides: [
        appServicesProvider.overrideWithValue(backend.services),
        activeProfileProvider.overrideWithValue(profile),
        vaultStatusProvider.overrideWith((ref) => Stream.value(unlocked)),
        dockerServiceProvider.overrideWithValue(docker),
      ],
    );
    addTearDown(() async {
      c.dispose();
      await backend.dispose();
    });
    c.listen(savedForwardsProvider, (_, _) {});
    await c.read(vaultStatusProvider.future);
    await c.read(localSettingsProvider.future);
    final m = c.read(savedForwardManagerProvider);
    await m.save(plan());
    await Future<void>.delayed(Duration.zero);
    expect(c.read(savedForwardsProvider), hasLength(1));
    final session = await docker.connect('local');
    docker.pendingConnect = Completer<DockerSessionInfo>();
    final starting = m.start(plan());
    final check = expectLater(starting, throwsA(isA<AppException>()));
    c.updateOverrides([
      appServicesProvider.overrideWithValue(backend.services),
      activeProfileProvider.overrideWithValue(other),
      vaultStatusProvider.overrideWith((ref) => Stream.value(unlocked)),
      dockerServiceProvider.overrideWithValue(docker),
    ]);
    expect(c.read(savedForwardsProvider), isEmpty);
    docker.pendingConnect!.complete(session);
    await check;
    expect(docker.targets, isEmpty);
    expect(backend.services.settings.currentLocal.savedForwards, hasLength(1));
    await expectLater(c.read(savedForwardManagerProvider).start(plan()), throwsA(isA<AppException>()));
  });
}
