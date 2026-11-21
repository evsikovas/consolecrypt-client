import 'package:consolecrypt/core/mock/mock_docker.dart';
import 'package:consolecrypt/docker/docker_service.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('resolves repository digest from the immutable image ID in container inspect', () async {
    final docker = MockDockerService();
    final session = (await docker.connect(DockerHost.localId)).id;
    final container = (await docker.containers(session)).singleWhere((c) => c.name == 'backup-once');
    final inspect = (await docker.inspect(session, container.id)).inspect;
    final imageId = inspect['Image'] as String;
    expect(imageId, matches(RegExp(r'^sha256:[0-9a-f]{64}$')));
    final refs = await docker.imageRepoDigests(session, imageId, container.image);
    expect(refs.single, matches(RegExp(r'^docker\.io/library/alpine@sha256:[0-9a-f]{64}$')));
    expect(refs.single.split('@').last, isNot(imageId));
    expect(await docker.imageRepoDigests(session, imageId, 'other.test/another:1'), isEmpty);
    await expectLater(
      docker.imageRepoDigests(session, 'alpine:3.20', container.image),
      throwsA(isA<DockerException>()),
    );
    await docker.action(session, container.id, DockerAction.remove);
    expect(await docker.imageRepoDigests(session, imageId, container.image), refs);
    await docker.disconnect(session);
    await expectLater(docker.imageRepoDigests(session, imageId, container.image), throwsA(isA<DockerException>()));
  });

  test('image only built locally has no repository digest to suggest', () async {
    final docker = MockDockerService();
    final session = (await docker.connect('ssh-web')).id;
    final worker = (await docker.containers(session)).singleWhere((c) => c.name == 'queue-worker');
    final inspect = (await docker.inspect(session, worker.id)).inspect;
    expect(await docker.imageRepoDigests(session, inspect['Image'] as String, worker.image), isEmpty);
  });
}
