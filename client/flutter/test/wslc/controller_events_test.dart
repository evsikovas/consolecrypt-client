import 'package:consolecrypt/wslc/app/desk_controller.dart';
import 'package:consolecrypt/wslc/src/rust/api/desk.dart';
import 'package:consolecrypt/wslc/src/rust/models.dart';
import 'package:flutter_test/flutter_test.dart';

/// Minimal runtime: one container that a single event reports as started.
class EventsApi implements WslcApi {
  int containerReads = 0;
  int imageReads = 0;
  bool sent = false;

  @override
  Future<RuntimeInfo> probeRuntime({String? cliPath}) async => const RuntimeInfo(
    cliPath: r'C:\Program Files\WSL\wslc.exe',
    cliVersion: 'wslc 3.0.3.0',
    versionSupported: true,
    versionVerified: true,
    readable: true,
  );

  @override
  Future<List<ContainerSummary>> listContainers() async {
    containerReads++;
    return [
      ContainerSummary(
        id: 'a' * 64,
        name: 'web',
        state: containerReads > 1 ? ContainerState.running : ContainerState.exited,
      ),
    ];
  }

  @override
  Future<List<ImageSummary>> listImages() async {
    imageReads++;
    return const [];
  }

  @override
  Future<List<VolumeSummary>> listVolumes() async => const [];

  @override
  Future<List<NetworkSummary>> listNetworks() async => const [];

  @override
  Future<void> startEvents() async {}

  @override
  Future<EventBatch> takeEvents() async {
    final events = sent
        ? const <DeskEvent>[]
        : [DeskEvent(kind: 'container', action: 'start', id: 'a' * 64, name: 'web', time: 1)];
    sent = true;
    return EventBatch(events: events, running: true);
  }

  @override
  Future<List<ContainerStats>> containerStats() async => const [];

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

void main() {
  final api = EventsApi();

  test('an event re-reads only the affected section and goes live', () async {
    final controller = DeskController(api: api);
    final received = <DeskEvent>[];
    controller.events.listen(received.add);
    await controller.connect(null);
    expect(api.containerReads, 1);
    expect(api.imageReads, 1);
    await Future<void>.delayed(const Duration(milliseconds: 1200));
    expect(controller.eventsLive, isTrue);
    expect(received.single.action, 'start');
    expect(api.containerReads, 2);
    expect(api.imageReads, 1, reason: 'a container start does not touch images');
    expect(controller.containers.single.state, ContainerState.running);
    controller.dispose();
  });
}
