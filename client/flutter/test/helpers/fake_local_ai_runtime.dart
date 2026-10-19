import 'dart:async';

import 'package:consolecrypt/ai/local/local_ai_runtime.dart';
import 'package:consolecrypt/core/util/value_stream.dart';

class FakeLocalAiRuntime implements LocalAiRuntime {
  FakeLocalAiRuntime({bool installed = true})
    : state = ValueStreamController(
        LocalAiRuntimeStatus(
          phase: installed ? LocalAiPhase.stopped : LocalAiPhase.notInstalled,
          isInstalled: installed,
        ),
      );
  final ValueStreamController<LocalAiRuntimeStatus> state;
  final endpoint = LocalAiEndpoint(
    baseUrl: Uri.parse('http://127.0.0.1:45678/v1'),
    apiKey: 'a' * 64,
    modelId: LocalAiCatalog.model.id,
  );
  Completer<LocalAiEndpoint>? startGate;
  Completer<void>? installGate;
  int starts = 0, stops = 0, installs = 0, removals = 0, cancellations = 0, leases = 0;

  @override
  LocalAiRuntimeStatus get status => state.value;
  @override
  Stream<LocalAiRuntimeStatus> get statuses => state.stream;
  @override
  LocalAiModel get selectedModel => LocalAiCatalog.model;
  @override
  Future<void> initialize() async {}
  @override
  Future<void> selectModel(String id) async {}
  @override
  Future<LocalAiEndpoint> ensureReady() async {
    starts++;
    state.value = LocalAiRuntimeStatus(phase: LocalAiPhase.starting, isInstalled: status.isInstalled);
    final ready = await (startGate?.future ?? Future.value(endpoint));
    state.value = const LocalAiRuntimeStatus(phase: LocalAiPhase.ready, isInstalled: true);
    return ready;
  }

  @override
  Future<T> withEndpoint<T>(Future<T> Function(LocalAiEndpoint) action) async {
    leases++;
    try {
      return await action(await ensureReady());
    } finally {
      leases--;
    }
  }

  @override
  Future<void> install() async {
    installs++;
    state.value = const LocalAiRuntimeStatus(phase: LocalAiPhase.installing, downloadedBytes: 50, totalBytes: 100);
    await installGate?.future;
    state.value = const LocalAiRuntimeStatus(phase: LocalAiPhase.stopped, isInstalled: true);
  }

  @override
  void cancelInstall() {
    cancellations++;
  }

  @override
  Future<void> stop() async {
    stops++;
    state.value = LocalAiRuntimeStatus(
      phase: status.isInstalled ? LocalAiPhase.stopped : LocalAiPhase.notInstalled,
      isInstalled: status.isInstalled,
    );
  }

  @override
  Future<void> uninstall() async {
    removals++;
    state.value = const LocalAiRuntimeStatus(phase: LocalAiPhase.notInstalled);
  }

  @override
  void touch() {}
  @override
  Future<void> dispose() => state.close();
  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}
