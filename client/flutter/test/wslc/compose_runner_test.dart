import 'package:consolecrypt/l10n/app_localizations_ru.dart';
import 'package:consolecrypt/wslc/app/desk_controller.dart';
import 'package:consolecrypt/wslc/features/compose/compose_runner.dart';
import 'package:consolecrypt/wslc/src/rust/api/desk.dart';
import 'package:consolecrypt/wslc/src/rust/models.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/compose_fixture.dart';

/// Records the WSLc calls the runner makes, in order.
class _Api implements WslcApi {
  final calls = <String>[];
  List<String> networks = <String>['bridge'];
  List<String> volumes = <String>[];
  List<ImageSummary> images = <ImageSummary>[];
  List<ComposeContainer> Function(int read) project = (_) => const [];
  int _reads = 0;

  void reset() {
    calls.clear();
    networks = ['bridge'];
    volumes = [];
    images = [];
    project = (_) => const [];
    _reads = 0;
  }

  @override
  Future<List<NetworkSummary>> listNetworks() async => [for (final n in networks) NetworkSummary(id: n, name: n)];

  @override
  Future<List<VolumeSummary>> listVolumes() async => [for (final v in volumes) VolumeSummary(name: v)];

  @override
  Future<List<ImageSummary>> listImages() async => images;

  @override
  Future<void> createHostFolders({required List<String> paths}) async => calls.add('folders ${paths.join(',')}');

  @override
  Future<void> createNetwork({required NetworkSpec spec}) async => calls.add('network ${spec.name}');

  @override
  Future<void> createVolume({required VolumeSpec spec}) async => calls.add('volume ${spec.name}');

  @override
  Future<BigInt> startPull({required String image}) async {
    calls.add('pull $image');
    return BigInt.one;
  }

  @override
  Future<BigInt> startBuild({required BuildSpec spec}) async {
    calls.add('build ${spec.tags.single}');
    return BigInt.two;
  }

  @override
  Future<JobBatch> takeJob({required BigInt job}) async => const JobBatch(
    lines: [
      JobLine(text: 'layer 40%', progress: true),
      JobLine(text: 'done', progress: false),
    ],
    finished: true,
    success: true,
    exitCode: 0,
  );

  @override
  Future<void> forgetJob({required BigInt job}) async {}

  @override
  Future<List<ComposeContainer>> composeContainers({required String project}) async => this.project(_reads++);

  @override
  Future<String> runContainer({required RunSpec spec}) async {
    calls.add('create ${spec.name}');
    return 'id-${spec.name}';
  }

  @override
  Future<void> composeConnect({
    required String networkName,
    required String containerId,
    required List<String> aliases,
    String? ip,
  }) async => calls.add('connect $networkName $containerId ${aliases.join('/')}');

  @override
  Future<OperationResult> startContainer({required String containerId}) async {
    calls.add('start $containerId');
    return OperationResult(containerId: containerId, uncertain: false);
  }

  @override
  Future<void> removeContainer({required String containerId, required bool force}) async =>
      calls.add('remove $containerId');

  @override
  Future<void> removeNetwork({required String networkName}) async => calls.add('rm-network $networkName');

  @override
  Future<void> removeVolume({required String volumeName}) async => calls.add('rm-volume $volumeName');

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

class _Controller extends DeskController {
  _Controller(WslcApi api) : super(api: api, l10n: AppLocalizationsRu());
  @override
  Future<void> refreshSections(Iterable<DeskSection> targets) async {}
}

ComposeContainer _db(String health, {String id = 'id-shop-db-1'}) => ComposeContainer(
  id: id,
  name: 'shop-db-1',
  service: 'db',
  state: ContainerState.running,
  configHash: 'db-hash',
  health: health,
);

void main() {
  final api = _Api();
  setUp(() {
    api.reset();
    ComposeRunner.pollEvery = Duration.zero;
  });

  test('up creates resources, waits for health and wires networks', () async {
    // Nothing exists at first; db reports starting, then healthy.
    api.project = (read) => switch (read) {
      0 => const [],
      1 => [_db('starting')],
      _ => [_db('healthy')],
    };
    final r = ComposeRunner(demoComposePlan(), _Controller(api));
    expect(await r.up(), isTrue, reason: r.error);
    expect(api.calls, [
      r'folders D:\projects\shop\site',
      'network shop_default',
      'network shop_back',
      'volume shop_data',
      'pull postgres:16',
      'build shop-web',
      'create shop-db-1',
      'start id-shop-db-1',
      'create shop-web-1',
      'connect shop_back id-shop-web-1 web',
      'start id-shop-web-1',
    ]);
    expect(r.steps.every((s) => s.status == StepStatus.done), isTrue);
    expect(r.steps.firstWhere((s) => s.title == 'Образ postgres:16').log, [
      'done',
    ], reason: 'a progress line is replaced by the next one');
  });

  test('a second up keeps unchanged containers and recreates changed ones', () async {
    api
      ..networks = ['bridge', 'shop_default', 'shop_back']
      ..volumes = ['shop_data']
      ..images = [
        const ImageSummary(id: '1', repository: 'docker.io/library/postgres', tag: '16'),
        const ImageSummary(id: '2', repository: 'shop-web', tag: 'latest'),
      ]
      ..project = (_) => [
        _db('healthy', id: 'old-db'),
        const ComposeContainer(
          id: 'old-web',
          name: 'shop-web-1',
          service: 'web',
          state: ContainerState.running,
          configHash: 'stale',
        ),
        const ComposeContainer(id: 'old-cache', name: 'shop-cache-1', service: 'cache', state: ContainerState.exited),
      ];
    final r = ComposeRunner(demoComposePlan(), _Controller(api));
    expect(await r.up(), isTrue, reason: r.error);
    expect(api.calls, [
      r'folders D:\projects\shop\site',
      'remove old-web',
      'create shop-web-1',
      'connect shop_back id-shop-web-1 web',
      'start id-shop-web-1',
    ]);
    expect(r.steps.firstWhere((s) => s.title.startsWith('db')).note, 'без изменений, уже запущен');
    expect(r.steps.last.note, contains('shop-cache-1'));
  });

  test('an unhealthy dependency stops the run before its dependents', () async {
    api.project = (read) => read == 0 ? const [] : [_db('unhealthy')];
    final r = ComposeRunner(demoComposePlan(), _Controller(api));
    expect(await r.up(), isFalse);
    expect(r.error, contains('нездоров'));
    expect(api.calls, isNot(contains('create shop-web-1')));
    expect(r.steps.last.status, StepStatus.failed);
  });

  test('down removes containers and networks, volumes only on request', () async {
    api
      ..networks = ['bridge', 'shop_default', 'shop_back']
      ..volumes = ['shop_data']
      ..project = (_) => [_db('healthy', id: 'old-db')];
    final r = ComposeRunner(demoComposePlan(), _Controller(api));
    expect(await r.down(), isTrue, reason: r.error);
    expect(api.calls, ['remove old-db', 'rm-network shop_default', 'rm-network shop_back']);
    api.calls.clear();
    expect(await r.down(volumes: true), isTrue);
    expect(api.calls.last, 'rm-volume shop_data');
  });

  test('image references compare the way Docker resolves them', () {
    expect(normalizeImage('nginx'), 'nginx:latest');
    expect(normalizeImage('docker.io/library/nginx:1.30'), 'nginx:1.30');
    expect(normalizeImage('localhost:5000/app'), 'localhost:5000/app:latest');
    expect(normalizeImage('mcr.microsoft.com/a/b@sha256:abc'), 'mcr.microsoft.com/a/b@sha256:abc');
    expect(hasImage([const ImageSummary(id: '1', repository: 'nginx', tag: 'latest')], 'nginx'), isTrue);
  });
}
