import 'dart:async';

import 'package:consolecrypt/ai/local/local_ai_runtime.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/core/util/value_stream.dart';

/// Adds a device-only provider while all inference still passes through the
/// native AI service and its sanitizer, context boundaries and approval gate.
class DeviceLocalAiService implements AiService, AiConversationHistory {
  DeviceLocalAiService({
    required this.delegate,
    required this.bridge,
    required this.runtime,
    required this.settings,
    required this.profiles,
    required this.vault,
  }) {
    _scope = _currentScope;
    _runtimeSelection = _currentRuntimeSelection;
    _subscriptions.addAll([
      delegate.watchProviders().listen((value) {
        _remote = value;
        _publish();
      }),
      settings.watchLocal().listen((_) => _publish()),
      profiles.watchProfiles().listen(
        (value) => _scopeChanged((value.active?.id.value, _currentScope.$2, _currentScope.$3)),
      ),
      vault.watchStatus().listen((value) => _scopeChanged((_currentScope.$1, value.vaultId?.value, value.isUnlocked))),
      runtime.statuses.listen((status) {
        if (_runtimeSelection != _currentRuntimeSelection) {
          _runtimeSelection = _currentRuntimeSelection;
          _generation++;
          for (final cancel in _cancelRequests.toList()) {
            cancel();
          }
          _clearRegistration();
        }
        if (status.phase != LocalAiPhase.ready && status.phase != LocalAiPhase.starting) _clearRegistration();
        _publish();
      }),
    ]);
  }

  final AiService delegate;
  final DeviceLocalAiBridge bridge;
  final LocalAiRuntime runtime;
  final SettingsService settings;
  final ProfileService profiles;
  final VaultService vault;
  final _providers = ValueStreamController<List<AiProviderConfig>>(const []);
  final _subscriptions = <StreamSubscription<Object?>>[];
  List<AiProviderConfig> _remote = const [];
  late (String?, String?, bool) _scope;
  late (int, String) _runtimeSelection;
  int _generation = 0;
  bool _disposed = false;
  String? _registration;
  final _cancelRequests = <void Function()>{};

  (String?, String?, bool) get _currentScope =>
      (profiles.currentProfiles.active?.id.value, vault.currentStatus.vaultId?.value, vault.currentStatus.isUnlocked);
  (int, String) get _currentRuntimeSelection => (runtime.status.selectionRevision, runtime.selectedModel.id);

  void _scopeChanged((String?, String?, bool) next) {
    if (next != _scope) {
      _scope = next;
      _generation++;
      for (final cancel in _cancelRequests.toList()) {
        cancel();
      }
      _clearRegistration();
      unawaited(runtime.stop().catchError((Object _) {}));
    }
    _publish();
  }

  void _clearRegistration() {
    final id = _registration;
    _registration = null;
    if (id != null) unawaited(bridge.clearDeviceLocalProvider(id).catchError((Object _) {}));
  }

  void _publish() {
    if (_disposed) return;
    // A selected, not-yet-installed model must not turn a local default into
    // cloud inference while another downloaded model remains on this device.
    final available = _localProviderAvailable;
    final preferred = available && settings.currentLocal.preferDeviceLocalAi;
    final now = DateTime.fromMillisecondsSinceEpoch(0, isUtc: true);
    _providers.value = [
      // Preserve the existing fallback order when the device default is off.
      for (final provider in _remote)
        if (provider.id != deviceLocalAiProviderId) preferred ? provider.copyWith(isDefault: false) : provider,
      if (available)
        AiProviderConfig(
          id: deviceLocalAiProviderId,
          name: 'ConsoleCrypt',
          provider: AiProviderKind.openaiCompatible,
          baseUrl: 'http://127.0.0.1',
          chatModel: runtime.selectedModel.id,
          privacyProfile: PrivacyProfile.local,
          isDefault: preferred,
          timeoutSecs: 300,
          createdAt: now,
          updatedAt: now,
        ),
    ];
  }

  bool _isCurrent(int generation, (String?, String?, bool) scope) =>
      !_disposed &&
      generation == _generation &&
      scope == _currentScope &&
      scope.$3 &&
      _runtimeSelection == _currentRuntimeSelection;

  bool get _localProviderAvailable =>
      _currentScope.$3 &&
      (runtime.status.hasInstalledModels ||
          (!runtime.status.isInitialized && settings.currentLocal.preferDeviceLocalAi));

  Future<void> _register(LocalAiEndpoint endpoint, int generation, (String?, String?, bool) scope) async {
    if (!_isCurrent(generation, scope) || scope.$1 == null || scope.$2 == null) {
      throw StateError('Local AI request cancelled: vault changed');
    }
    if (endpoint.baseUrl.scheme != 'http' ||
        endpoint.baseUrl.host != '127.0.0.1' ||
        endpoint.baseUrl.path != '/v1' ||
        !endpoint.baseUrl.hasPort ||
        endpoint.modelId != runtime.selectedModel.id) {
      throw StateError('Invalid local AI endpoint');
    }
    final id = ObjectId.generate().value;
    await bridge.registerDeviceLocalProvider(
      profileId: scope.$1!,
      vaultId: scope.$2!,
      registrationId: id,
      port: endpoint.baseUrl.port,
      apiKey: endpoint.apiKey,
      model: endpoint.modelId,
    );
    if (!_isCurrent(generation, scope)) {
      await bridge.clearDeviceLocalProvider(id);
      throw StateError('Local AI request cancelled: vault changed');
    }
    _registration = id;
  }

  Stream<AiStreamEvent> _stream(Stream<AiStreamEvent> Function() request) => Stream<AiStreamEvent>.multi((output) {
    final generation = _generation;
    final scope = _currentScope;
    var cancelled = false;
    StreamSubscription<AiStreamEvent>? subscription;
    final done = Completer<void>();
    void cancel() {
      cancelled = true;
      unawaited(subscription?.cancel());
      if (!done.isCompleted) done.complete();
      output.close();
    }

    _cancelRequests.add(cancel);
    output.onCancel = () async {
      cancelled = true;
      _cancelRequests.remove(cancel);
      await subscription?.cancel();
      if (!done.isCompleted) done.complete();
    };
    unawaited(() async {
      try {
        await runtime.withEndpoint((endpoint) async {
          if (cancelled) return;
          await _register(endpoint, generation, scope);
          if (cancelled || !_isCurrent(generation, scope)) return;
          subscription = request().listen(
            (event) {
              if (!cancelled && _isCurrent(generation, scope)) output.add(event);
            },
            onError: (Object error, StackTrace stack) {
              if (!cancelled) output.addError(error, stack);
              if (!done.isCompleted) done.complete();
            },
            onDone: () {
              if (!done.isCompleted) done.complete();
            },
          );
          await done.future;
        });
      } catch (error) {
        if (!cancelled && _isCurrent(generation, scope)) output.add(AiFailed(error.toString()));
      } finally {
        _cancelRequests.remove(cancel);
        if (!cancelled) unawaited(output.close());
      }
    }());
  });

  /// Runs [request] while the built-in model is started and registered with
  /// the native core, so features that call the core directly (Kubernetes AI)
  /// can use [deviceLocalAiProviderId]. The lease ends when [request] completes.
  Future<T> withDeviceLocalProvider<T>(Future<T> Function() request) => _call(request);

  Future<T> _call<T>(Future<T> Function() request) {
    final generation = _generation;
    final scope = _currentScope;
    return runtime.withEndpoint((endpoint) async {
      await _register(endpoint, generation, scope);
      final result = await request();
      if (!_isCurrent(generation, scope)) throw StateError('Local AI request cancelled: vault changed');
      return result;
    });
  }

  @override
  Stream<List<AiProviderConfig>> watchProviders() => _providers.stream;

  // Conversations are kept by the native service for every provider.
  @override
  Future<List<AiConversation>> listConversations() async =>
      delegate is AiConversationHistory ? (delegate as AiConversationHistory).listConversations() : const [];

  @override
  Future<void> deleteConversation(ObjectId id) async {
    if (delegate is AiConversationHistory) await (delegate as AiConversationHistory).deleteConversation(id);
  }

  @override
  void continueConversation(ObjectId providerId, ObjectId id) {
    if (delegate is AiConversationHistory) (delegate as AiConversationHistory).continueConversation(providerId, id);
  }

  @override
  Stream<AiStreamEvent> chat({
    required ObjectId providerId,
    required List<ChatMessage> history,
    AiContextSelection context = AiContextSelection.none,
  }) => providerId == deviceLocalAiProviderId
      ? _stream(() => delegate.chat(providerId: providerId, history: history, context: context))
      : delegate.chat(providerId: providerId, history: history, context: context);

  @override
  Stream<AiStreamEvent> generateCommand({
    required ObjectId providerId,
    required String request,
    SnippetType? dialect,
    AiContextSelection context = AiContextSelection.none,
  }) => providerId == deviceLocalAiProviderId
      ? _stream(
          () => delegate.generateCommand(providerId: providerId, request: request, dialect: dialect, context: context),
        )
      : delegate.generateCommand(providerId: providerId, request: request, dialect: dialect, context: context);

  @override
  Future<SnippetDraft> draftSnippet({required String command, ObjectId? providerId}) {
    final effective =
        providerId ??
        (_localProviderAvailable && settings.currentLocal.preferDeviceLocalAi ? deviceLocalAiProviderId : null);
    return effective == deviceLocalAiProviderId
        ? _call(() => delegate.draftSnippet(command: command, providerId: effective))
        : delegate.draftSnippet(command: command, providerId: effective);
  }

  @override
  Future<ProviderHealth> testProvider(ObjectId id) =>
      id == deviceLocalAiProviderId ? _call(() => delegate.testProvider(id)) : delegate.testProvider(id);

  @override
  Future<AiProviderConfig> saveProvider(AiProviderConfig config, {SecretText? apiKey, bool clearApiKey = false}) {
    if (config.id == deviceLocalAiProviderId) {
      apiKey?.wipe();
      return Future.error(StateError('Device-local AI settings cannot be saved in the vault'));
    }
    return delegate.saveProvider(config, apiKey: apiKey, clearApiKey: clearApiKey);
  }

  @override
  Future<void> deleteProvider(ObjectId id) => id == deviceLocalAiProviderId
      ? Future.error(StateError('Remove the local model in device settings'))
      : delegate.deleteProvider(id);

  Future<void> dispose() async {
    _disposed = true;
    _generation++;
    for (final cancel in _cancelRequests.toList()) {
      cancel();
    }
    _clearRegistration();
    for (final subscription in _subscriptions) {
      await subscription.cancel();
    }
    await _providers.close();
  }
}
