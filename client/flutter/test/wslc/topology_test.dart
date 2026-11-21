import 'package:consolecrypt/wslc/features/topology/topology_page.dart';
import 'package:consolecrypt/wslc/src/rust/models.dart';
import 'package:flutter_test/flutter_test.dart';

ContainerLinks links(
  String id,
  String name, {
  String? image,
  List<String> volumes = const [],
  List<String> binds = const [],
  List<String> networks = const ['bridge'],
}) => ContainerLinks(
  id: id * 64,
  name: name,
  state: ContainerState.running,
  imageId: image,
  networks: networks,
  volumes: volumes,
  binds: binds,
);

void main() {
  test('map links shared resources and flags unused ones', () {
    final model = MapModel.build(
      links: [
        links('a', 'web', image: 'sha256:${'1' * 64}', volumes: ['data'], binds: [r'D:\site']),
        links('b', 'api', image: 'sha256:${'1' * 64}', volumes: ['data'], networks: ['bridge', 'backend']),
      ],
      images: [
        ImageSummary(id: 'sha256:${'1' * 64}', repository: 'nginx', tag: '1.30'),
        ImageSummary(id: 'sha256:${'2' * 64}', repository: 'old', tag: '1'),
      ],
      volumes: const [
        VolumeSummary(name: 'data'),
        VolumeSummary(name: 'orphan'),
      ],
      networks: const [
        NetworkSummary(id: 'n1', name: 'bridge'),
        NetworkSummary(id: 'n2', name: 'host'),
        NetworkSummary(id: 'n3', name: 'old-net'),
      ],
    );
    final [images, containers, resources] = model.columns;
    final storage = resources.where((n) => n.kind != NodeKind.network);
    final nets = resources.where((n) => n.kind == NodeKind.network);
    expect(resources.last.kind, NodeKind.network, reason: 'networks go last');
    expect(containers.map((n) => n.label), ['web', 'api']);
    expect(images.first.label, 'nginx');
    expect(images.first.degree, 2);
    expect(images.last.unused, isTrue, reason: 'old image has no containers');
    expect(storage.map((n) => n.key), ['data', 'orphan', r'D:\site']);
    expect(storage.firstWhere((n) => n.key == 'orphan').unused, isTrue);
    expect(storage.last.label, 'site');
    expect(nets.map((n) => n.key), containsAll(['bridge', 'backend', 'host']));
    expect(nets.firstWhere((n) => n.key == 'host').unused, isFalse, reason: 'built-in');
    expect(nets.firstWhere((n) => n.key == 'old-net').unused, isTrue);
    expect(model.edges, hasLength(2 + 3 + 3));
  });

  test('a container without an image ID is matched by repository and tag', () {
    final model = MapModel.build(
      links: [
        const ContainerLinks(
          id: 'c',
          state: ContainerState.exited,
          imageName: 'nginx:1.30',
          networks: [],
          volumes: [],
          binds: [],
        ),
      ],
      images: [ImageSummary(id: 'sha256:${'1' * 64}', repository: 'nginx', tag: '1.30')],
      volumes: const [],
      networks: const [],
    );
    expect(model.columns.first, hasLength(1));
    expect(model.columns.first.single.degree, 1);
  });
}
