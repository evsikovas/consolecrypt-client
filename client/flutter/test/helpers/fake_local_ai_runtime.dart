import 'dart:async';

import 'package:consolecrypt/ai/local/local_ai_runtime.dart';
import 'package:consolecrypt/core/util/value_stream.dart';

class FakeLocalAiRuntime implements LocalAiRuntime {
  FakeLocalAiRuntime({bool installed = true})
    : state = ValueStreamController(
        LocalAiRuntimeStatus(
          phase: installed ? LocalAiPhase.stopped : LocalAiPhase.notInstalled,
          isInstalled: installed,
          runtimeInstalled: true,
          installedModelIds: installed ? {LocalAiCatalog.model.id} : const {},
        ),
      );
  final ValueStreamController<LocalAiRuntimeStatus> state;
  LocalAiEndpoint get endpoint =>
      LocalAiEndpoint(baseUrl: Uri.parse('http://127.0.0.1:45678/v1'), apiKey: 'a' * 64, modelId: selectedModel.id);
  Completer<LocalAiEndpoint>? startGate;
  Completer<void>? installGate;
  int starts = 0, stops = 0, installs = 0, removals = 0, cancellations = 0, leases = 0;

  @override
  LocalAiRuntimeStatus get status => state.value;
  @override
  Stream<LocalAiRuntimeStatus> get statuses => state.stream;
  @override
  LocalAiModel get selectedModel => LocalAiCatalog.models.firstWhere((model) => model.id == status.modelId);
  void emit(
    LocalAiPhase phase, {
    Set<String>? installed,
    int? revision,
    String? modelId,
    int received = 0,
    int total = 0,
  }) {
    final ids = installed ?? status.installedModelIds;
    final selected = modelId ?? status.modelId;
    state.value = LocalAiRuntimeStatus(
      phase: phase,
      isInstalled: ids.contains(selected),
      runtimeInstalled: true,
      installedModelIds: ids,
      selectionRevision: revision ?? status.selectionRevision,
      modelId: selected,
      downloadedBytes: received,
      totalBytes: total,
    );
  }

  @override
  Future<void> initialize() async {}
  @override
  Future<void> selectModel(String id) async {
    stops++;
    emit(
      status.installedModelIds.contains(id) ? LocalAiPhase.stopped : LocalAiPhase.notInstalled,
      modelId: id,
      revision: status.selectionRevision + 1,
    );
  }

  @override
  Future<LocalAiEndpoint> ensureReady() async {
    starts++;
    if (!status.isInstalled) throw const LocalAiException('model_not_installed');
    final revision = status.selectionRevision;
    emit(LocalAiPhase.starting);
    final ready = await (startGate?.future ?? Future.value(endpoint));
    if (revision == status.selectionRevision) emit(LocalAiPhase.ready);
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
    emit(LocalAiPhase.installing, received: 50, total: 100);
    await installGate?.future;
    emit(LocalAiPhase.stopped, installed: {...status.installedModelIds, selectedModel.id});
  }

  @override
  void cancelInstall() {
    cancellations++;
  }

  @override
  Future<void> stop() async {
    stops++;
    emit(status.isInstalled ? LocalAiPhase.stopped : LocalAiPhase.notInstalled);
  }

  @override
  Future<void> uninstall() async {
    removals++;
    emit(
      LocalAiPhase.notInstalled,
      installed: {...status.installedModelIds}..remove(selectedModel.id),
      revision: status.selectionRevision + 1,
    );
  }

  @override
  void touch() {}
  @override
  Future<void> dispose() => state.close();
  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}
