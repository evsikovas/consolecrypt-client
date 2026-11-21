import 'dart:async';

import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/mock/mock_cloud.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/services/host_metrics_service.dart';
import 'package:consolecrypt/core/services/terminal_service.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

final class _MetricsTerminal extends Fake implements TerminalService, HostMetricsReader {
  final streams = <TerminalSessionId, StreamController<HostMetricsSample>>{};
  final operations = <TerminalSessionId, String>{};
  final cancelled = <String>[];

  @override
  Stream<HostMetricsSample> watchHost(Host host, TerminalSessionId id, String operationId) {
    operations[id] = operationId;
    return (streams[id] = StreamController<HostMetricsSample>(sync: true)).stream;
  }

  @override
  Future<void> cancelHostWatch(String id) async => cancelled.add(id);
  @override
  Future<void> close(TerminalSessionId id) async {}
}

void main() {
  testWidgets('transient SSH errors retry while connected and never restart after stop', (tester) async {
    final service = _MetricsTerminal();
    final host = Host.create(name: 'demo', address: 'localhost');
    final id = TerminalSessionId.generate();
    final history = TerminalHostMetrics();
    addTearDown(history.dispose);
    history.start(host, id, service);
    final first = service.operations[id];
    service.streams[id]!.addError(const AppException(AppErrorCode.serverUnreachable, 'timeout'));
    expect(history.value.error?.code, AppErrorCode.serverUnreachable);
    await tester.pump(const Duration(seconds: 15));
    final second = service.operations[id];
    expect(second, isNot(first));
    service.streams[id]!.add(HostMetricsSample.fromJson({'timestampMs': 1, 'uptimeSeconds': 1}));
    expect(history.value.error, isNull);
    service.streams[id]!.addError(const AppException(AppErrorCode.serverUnreachable, 'timeout'));
    history.stop();
    await tester.pump(const Duration(seconds: 30));
    expect(service.operations[id], second);
    expect(history.value.latest, isNull);
  });

  test('sessions sample offscreen independently and retire their watchers on disconnect, lock and dispose', () async {
    final backend = MockBackend(config: const MockConfig.test());
    addTearDown(backend.dispose);
    final service = _MetricsTerminal();
    final container = ProviderContainer(
      overrides: [
        appServicesProvider.overrideWithValue(backend.services),
        terminalServiceProvider.overrideWithValue(service),
      ],
    );
    await backend.debugSignInDemoAndUnlock();
    final tabs = container.read(terminalTabsProvider.notifier);
    final host = Host.create(name: 'demo', address: 'localhost');
    final events = <StreamController<TerminalEvent>>[];
    TerminalTab open() {
      final channel = StreamController<TerminalEvent>();
      events.add(channel);
      final tab = tabs.openHandle(
        target: SshTerminalTarget(host),
        handle: TerminalSessionHandle(
          id: TerminalSessionId.generate(),
          output: const Stream.empty(),
          events: channel.stream,
        ),
      );
      channel.add(const TerminalStateChanged(SessionConnectionState.connected));
      return tab;
    }

    final first = open(), second = open();
    await pumpEventQueue();
    final firstOperation = service.operations[first.sessionId]!;
    final secondOperation = service.operations[second.sessionId]!;
    expect(firstOperation, isNot(secondOperation));
    final stream = service.streams[first.sessionId]!;
    for (var i = 0; i <= TerminalHostMetrics.capacity; i++) {
      stream.add(HostMetricsSample.fromJson({'timestampMs': i, 'uptimeSeconds': 123}));
    }
    expect(first.hostMetrics.value.samples.length, TerminalHostMetrics.capacity);
    expect(first.hostMetrics.value.samples.first.time.millisecondsSinceEpoch, 1);
    expect(second.hostMetrics.value.latest, isNull);
    events.last.add(const TerminalStateChanged(SessionConnectionState.disconnected));
    await pumpEventQueue();
    expect(service.cancelled, [secondOperation]);
    await backend.vault.lock();
    await pumpEventQueue();
    expect(service.cancelled, [secondOperation, firstOperation]);
    await stream.close();
    expect(first.hostMetrics.value.samples, isEmpty);
    await backend.vault.unlockWithPassphrase(SecretText(MockCloud.demoPassphrase));
    await pumpEventQueue();
    final resumedOperation = service.operations[first.sessionId]!;
    expect(resumedOperation, isNot(firstOperation));
    expect(service.operations[second.sessionId], secondOperation);
    container.dispose();
    await pumpEventQueue();
    expect(service.cancelled.last, resumedOperation);
    for (final channel in events) {
      await channel.close();
    }
    for (final stream in service.streams.values) {
      await stream.close();
    }
  });
}
