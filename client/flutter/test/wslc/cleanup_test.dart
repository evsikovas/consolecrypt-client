import 'package:consolecrypt/l10n/app_localizations_en.dart';
import 'package:consolecrypt/wslc/features/cleanup/cleanup_page.dart';
import 'package:consolecrypt/wslc/src/rust/models.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  final full = 'sha256:${'a' * 64}';
  ContainerSummary row(String id, String name, ContainerState state) =>
      ContainerSummary(id: id * 64, name: name, state: state, image: 'nginx:1.30');
  ContainerLinks link(String id, {String? imageId, List<String> volumes = const []}) => ContainerLinks(
    id: id * 64,
    state: ContainerState.exited,
    imageId: imageId,
    networks: const ['bridge'],
    volumes: volumes,
    binds: const [],
  );

  test('offers stopped containers and resources nothing uses', () {
    final items = cleanupCandidates(
      l10n: AppLocalizationsEn(),
      containers: [
        row('1', 'web', ContainerState.running),
        row('2', 'old', ContainerState.exited),
        row('3', 'fresh', ContainerState.created),
        row('4', 'odd', ContainerState.unknown),
      ],
      links: [
        // Inspect gives the full ID, the image list a short one.
        link('1', imageId: full, volumes: ['data']),
        link('2', imageId: full),
        link('3'),
        link('4'),
      ],
      images: [
        ImageSummary(id: 'a' * 12, repository: 'nginx', tag: '1.30', sizeDisplay: '108MB'),
        ImageSummary(id: 'b' * 12, repository: 'old', tag: 'v1', sizeDisplay: '14.1MB', containers: 0),
        ImageSummary(id: 'b' * 12, repository: 'old', tag: 'v2', sizeDisplay: '14.1MB', containers: 0),
        // The runtime counts a container the map could not link.
        ImageSummary(id: 'c' * 12, repository: 'counted', tag: 'x', sizeDisplay: '1MB', containers: 1),
      ],
      volumes: const [
        VolumeSummary(name: 'data'),
        VolumeSummary(name: 'orphan', sizeDisplay: 'N/A'),
      ],
      networks: const [
        NetworkSummary(id: 'n1', name: 'bridge'),
        NetworkSummary(id: 'n2', name: 'none'),
        NetworkSummary(id: 'n3', name: 'stale-net'),
      ],
    );
    expect(items.map((i) => i.id), [
      'container:${'2' * 64}',
      'container:${'3' * 64}',
      'image:old:v1',
      'image:old:v2',
      'volume:orphan',
      'network:stale-net',
    ]);
    expect(reclaimable(items), closeTo(14.1e6, 1), reason: 'two tags of one image are counted once');
  });
}
