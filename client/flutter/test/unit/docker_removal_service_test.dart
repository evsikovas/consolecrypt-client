import 'package:consolecrypt/core/mock/mock_docker.dart';
import 'package:consolecrypt/docker/docker_service.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('the wire contract explicitly selects every item and preserves partial results', () {
    final plan = DockerRemovalPlan.fromJson({
      'plan_id': 'p1',
      'container_id': 'c1',
      'name': 'db',
      'running': false,
      'resources': [
        {'kind': 'volume', 'id': 'data', 'name': 'data', 'eligible': true, 'reason': 'eligible'},
      ],
    });
    expect(plan.resources.single.kind, DockerRemovalKind.volume);
    expect(DockerRemovalSelection(planId: plan.planId, confirmName: plan.name).toJson(), {
      'plan_id': 'p1',
      'volumes': <String>[],
      'networks': <String>[],
      'stop_running': false,
      'confirm_name': 'db',
    });
    final result = DockerRemovalResult.fromJson({
      'container_stopped': true,
      'container_removed': true,
      'resources': [
        {'kind': 'network', 'id': 'n1', 'name': 'test', 'status': 'skipped', 'reason': 'in_use'},
        {'kind': 'volume', 'id': 'data', 'name': 'data', 'status': 'failed', 'reason': 'api_error'},
      ],
      'error': null,
    });
    expect(result.containerRemoved, isTrue);
    expect(result.resources.map((r) => r.status), [DockerRemovalStatus.skipped, DockerRemovalStatus.failed]);
    expect(result.resources.first.reason, 'in_use');
  });

  late MockDockerService service;
  late String session;
  late List<DockerContainer> containers;
  DockerContainer named(String name) => containers.singleWhere((c) => c.name == name);
  Future<DockerRemovalPlan> planFor(String name) => service.removalPlan(session, named(name).id);
  Future<DockerRemovalResult> apply(
    DockerRemovalPlan plan, {
    bool stop = false,
    List<String> volumes = const [],
    List<String> networks = const [],
  }) => service.removeWithCleanup(
    session,
    plan.containerId,
    DockerRemovalSelection(
      planId: plan.planId,
      confirmName: plan.name,
      stopRunning: stop,
      volumes: volumes,
      networks: networks,
    ),
  );
  Future<List<String>> remainingVolumes() async => (await service.volumes(session)).map((v) => v.name).toList();

  group('scoped mock removal', () {
    setUp(() async {
      service = MockDockerService();
      session = (await service.connect(DockerHost.localId)).id;
      containers = await service.containers(session);
    });

    test('legacy removal keeps volumes and refuses to force-remove a running container', () async {
      await expectLater(
        service.action(session, named('proxy-nginx-1').id, DockerAction.remove),
        throwsA(isA<DockerException>()),
      );
      await service.action(session, named('proxy-cache-1').id, DockerAction.remove);
      expect(await remainingVolumes(), contains('proxy_data'));
      expect((await service.containers(session)).map((c) => c.name), contains('proxy-nginx-1'));
    });

    test('retains volumes and all other containers when cleanup is unchecked', () async {
      final cache = await planFor('proxy-cache-1');
      final result = await apply(cache);
      expect(result.containerRemoved, isTrue);
      expect(result.containerStopped, isFalse);
      expect(result.resources, isEmpty);
      expect(await remainingVolumes(), contains('proxy_data'));
      expect(
        (await service.containers(session)).map((c) => c.id),
        containers.where((c) => c.id != cache.containerId).map((c) => c.id),
      );
      await expectLater(apply(cache), throwsA(isA<DockerException>()));
    });

    test('active containers require stop and stopped neighbors still protect networks', () async {
      final nginx = await planFor('proxy-nginx-1');
      final shared = nginx.resources.singleWhere((r) => r.kind == DockerRemovalKind.network);
      expect(shared.reason, 'shared');
      expect(shared.eligible, isFalse);
      await expectLater(apply(nginx, stop: true, networks: [shared.id]), throwsA(isA<DockerException>()));
      final refused = await apply(await planFor(nginx.name));
      expect(refused.containerRemoved, isFalse);
      expect(refused.error, 'running');
      expect(service.actions, isEmpty);
      expect((await apply(await planFor(nginx.name), stop: true)).containerStopped, isTrue);

      final cache = await planFor('proxy-cache-1');
      final net = cache.resources.singleWhere((r) => r.kind == DockerRemovalKind.network);
      final volume = cache.resources.singleWhere((r) => r.kind == DockerRemovalKind.volume);
      expect(net.eligible, isTrue);
      expect(volume.eligible, isTrue);
      final result = await apply(cache, volumes: [volume.id], networks: [net.id]);
      expect(result.resources.map((r) => r.status), [DockerRemovalStatus.removed, DockerRemovalStatus.removed]);
      expect(await remainingVolumes(), isNot(contains(volume.name)));
      expect(await remainingVolumes(), contains('orphaned-cache'));
    });

    test('stale plans, another session, a wrong name and unrelated resources fail before deletion', () async {
      final old = await planFor('shop-web-1');
      final plan = await planFor('shop-web-1');
      await expectLater(apply(old, stop: true), throwsA(isA<DockerException>()));
      final bind = plan.resources.singleWhere((r) => r.kind == DockerRemovalKind.bind);
      expect(bind.name, '/etc/nginx/nginx.conf');
      expect(bind.eligible, isFalse);
      await expectLater(apply(plan, stop: true, volumes: [bind.id]), throwsA(isA<DockerException>()));
      final otherSession = (await service.connect(DockerHost.localId)).id;
      final current = await planFor(plan.name);
      final selection = DockerRemovalSelection(planId: current.planId, confirmName: current.name, stopRunning: true);
      await expectLater(
        service.removeWithCleanup(otherSession, plan.containerId, selection),
        throwsA(isA<DockerException>()),
      );
      await expectLater(
        service.removeWithCleanup(
          session,
          plan.containerId,
          DockerRemovalSelection(planId: current.planId, confirmName: 'wrong', stopRunning: true),
        ),
        throwsA(isA<DockerException>()),
      );
      expect(service.actions, isEmpty);
      expect((await service.containers(session)).length, containers.length);
    });
  });
}
