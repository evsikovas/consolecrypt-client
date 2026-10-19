import 'dart:async';

import 'package:consolecrypt/ai/command_palette.dart';
import 'package:consolecrypt/ai/device_local_ai_service.dart';
import 'package:consolecrypt/ai/local/local_ai_runtime.dart';
import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/ai_service.dart';
import 'package:consolecrypt/core/util/value_stream.dart';
import 'package:flutter_test/flutter_test.dart';

import '../helpers/fake_local_ai_runtime.dart';
import '../helpers/test_app.dart';

class _Delegate implements AiService, DeviceLocalAiBridge {
  final providers = ValueStreamController<List<AiProviderConfig>>([
    AiProviderConfig.draft(AiProviderKind.deepseek).copyWith(isDefault: true),
  ]);
  final registrations = <({String profile, String vault, String registration, int port, String model})>[];
  final cleared = <String>[];
  final calls = <String>[];
  final snippetProviders = <ObjectId?>[];
  StreamController<AiStreamEvent>? response;
  Completer<void>? registerGate;
  int saves = 0;
  Future<void> dispose() async {
    await response?.close();
    await providers.close();
  }

  @override
  Stream<List<AiProviderConfig>> watchProviders() => providers.stream;
  @override
  Future<void> registerDeviceLocalProvider({
    required String profileId,
    required String vaultId,
    required String registrationId,
    required int port,
    required String apiKey,
    required String model,
  }) async {
    registrations.add((profile: profileId, vault: vaultId, registration: registrationId, port: port, model: model));
    calls.add('register');
    await registerGate?.future;
  }

  @override
  Future<void> clearDeviceLocalProvider(String registrationId) async {
    cleared.add(registrationId);
  }

  @override
  Stream<AiStreamEvent> chat({
    required ObjectId providerId,
    required List<ChatMessage> history,
    AiContextSelection context = AiContextSelection.none,
  }) {
    calls.add('chat:${providerId.value}:${context.selectedTerminalText}');
    return response?.stream ??
        Stream.value(
          const AiCompleted(
            fullText: 'ok',
            report: SanitizationReport(profile: PrivacyProfile.local, redactions: 1, remote: false),
          ),
        );
  }

  @override
  Stream<AiStreamEvent> generateCommand({
    required ObjectId providerId,
    required String request,
    SnippetType? dialect,
    AiContextSelection context = AiContextSelection.none,
  }) {
    calls.add('generate');
    return Stream.value(
      const AiCompleted(
        fullText: 'review',
        report: SanitizationReport(profile: PrivacyProfile.local, redactions: 1, remote: false),
        command: GeneratedCommand(
          command: 'rm -rf /tmp/test',
          explanation: 'review',
          suggestedRisk: RiskLevel.destructive,
        ),
      ),
    );
  }

  @override
  Future<SnippetDraft> draftSnippet({required String command, ObjectId? providerId}) async {
    calls.add('snippet');
    snippetProviders.add(providerId);
    return const SnippetDraft(name: 'draft', template: 'ls', snippetType: SnippetType.bash);
  }

  @override
  Future<ProviderHealth> testProvider(ObjectId id) async => const ProviderHealth(ok: true, message: 'ready');
  @override
  Future<AiProviderConfig> saveProvider(AiProviderConfig config, {SecretText? apiKey, bool clearApiKey = false}) async {
    saves++;
    return config;
  }

  @override
  Future<void> deleteProvider(ObjectId id) async {}
}

Future<void> _flush() => Future<void>.delayed(Duration.zero);

void main() {
  test('local default persists only in device settings and never rewrites vault providers', () async {
    final backend = testBackend();
    await backend.debugSignInDemoAndUnlock();
    final runtime = FakeLocalAiRuntime(installed: false);
    final delegate = _Delegate();
    final service = DeviceLocalAiService(
      delegate: delegate,
      bridge: delegate,
      runtime: runtime,
      settings: backend.services.settings,
      profiles: backend.services.profiles,
      vault: backend.services.vault,
    );
    final values = <List<AiProviderConfig>>[];
    final subscription = service.watchProviders().listen(values.add);
    await _flush();
    expect(values.last, hasLength(1));
    expect(values.last.single.isDefault, isTrue);
    await runtime.install();
    await _flush();
    expect(values.last.last.id, deviceLocalAiProviderId);
    expect(values.last.last.isDefault, isTrue);
    expect(values.last.first.isDefault, isFalse);
    expect(delegate.providers.value.single.isDefault, isTrue);
    expect(delegate.saves, 0);
    await backend.services.settings.updateLocal(const LocalSettings(preferDeviceLocalAi: false));
    await _flush();
    expect(values.last.last.isDefault, isFalse);
    expect(values.last.first.isDefault, isTrue);
    // Existing single-provider installs need not have an explicit default flag.
    // Opting out of the local default must preserve their fallback selection.
    delegate.providers.value = [delegate.providers.value.single.copyWith(isDefault: false)];
    await _flush();
    expect(defaultProvider(values.last)?.id, delegate.providers.value.single.id);
    final encoded = localSettingsToJson(backend.services.settings.currentLocal);
    expect(encoded['prefer_device_local_ai'], isFalse);
    expect(localSettingsFromJson(encoded).preferDeviceLocalAi, isFalse);
    expect(localSettingsFromJson({}).preferDeviceLocalAi, isTrue);
    await expectLater(service.saveProvider(values.last.last), throwsStateError);
    expect(delegate.saves, 0);
    await runtime.uninstall();
    await _flush();
    expect(values.last, hasLength(1));
    expect(runtime.starts, 0, reason: 'listing and default changes must not start the model');
    await subscription.cancel();
    await service.dispose();
    await runtime.dispose();
    await delegate.dispose();
    await backend.dispose();
  });

  test('local chat, commands and snippets use the native delegate with a request lease', () async {
    final backend = testBackend();
    await backend.debugSignInDemoAndUnlock();
    final runtime = FakeLocalAiRuntime();
    final delegate = _Delegate()..response = StreamController<AiStreamEvent>();
    final service = DeviceLocalAiService(
      delegate: delegate,
      bridge: delegate,
      runtime: runtime,
      settings: backend.services.settings,
      profiles: backend.services.profiles,
      vault: backend.services.vault,
    );
    await _flush();
    final output = service
        .chat(
          providerId: deviceLocalAiProviderId,
          history: [ChatMessage.now(ChatRole.user, 'question')],
          context: const AiContextSelection(selectedTerminalText: 'selected'),
        )
        .toList();
    await _flush();
    expect(runtime.leases, 1);
    expect(delegate.calls, ['register', 'chat:${deviceLocalAiProviderId.value}:selected']);
    expect(delegate.registrations.single.profile, backend.services.profiles.currentProfiles.active!.id.value);
    expect(delegate.registrations.single.vault, backend.services.vault.currentStatus.vaultId!.value);
    delegate.response!.add(const AiDelta('answer'));
    await delegate.response!.close();
    expect((await output).single, isA<AiDelta>());
    expect(runtime.leases, 0);
    final events = await service.generateCommand(providerId: deviceLocalAiProviderId, request: 'delete').toList();
    expect((events.single as AiCompleted).command!.suggestedRisk, RiskLevel.destructive);
    await service.draftSnippet(command: 'ls', providerId: deviceLocalAiProviderId);
    await service.draftSnippet(command: 'pwd');
    expect(delegate.snippetProviders, [deviceLocalAiProviderId, deviceLocalAiProviderId]);
    expect(delegate.calls, containsAllInOrder(['generate', 'snippet']));
    expect(delegate.saves, 0);
    expect(runtime.leases, 0);
    await service.dispose();
    await runtime.dispose();
    await delegate.dispose();
    await backend.dispose();
  });

  for (final duringRegistration in [false, true]) {
    test('lock cancels a pending local ${duringRegistration ? 'registration' : 'startup'} before inference', () async {
      final backend = testBackend();
      await backend.debugSignInDemoAndUnlock();
      final runtime = FakeLocalAiRuntime();
      final delegate = _Delegate();
      if (duringRegistration) {
        delegate.registerGate = Completer<void>();
      } else {
        runtime.startGate = Completer();
      }
      final service = DeviceLocalAiService(
        delegate: delegate,
        bridge: delegate,
        runtime: runtime,
        settings: backend.services.settings,
        profiles: backend.services.profiles,
        vault: backend.services.vault,
      );
      await _flush();
      final output = service
          .chat(providerId: deviceLocalAiProviderId, history: [ChatMessage.now(ChatRole.user, 'private')])
          .toList();
      await _flush();
      await backend.services.vault.lock();
      await _flush();
      runtime.startGate?.complete(runtime.endpoint);
      delegate.registerGate?.complete();
      await _flush();
      expect(await output, isEmpty);
      expect(delegate.calls.where((call) => call.startsWith('chat')), isEmpty);
      expect(runtime.stops, greaterThan(0));
      expect(runtime.leases, 0);
      if (duringRegistration) expect(delegate.cleared, contains(delegate.registrations.single.registration));
      await service.dispose();
      await runtime.dispose();
      await delegate.dispose();
      await backend.dispose();
    });
  }

  test('offline initialization cannot temporarily route a saved local default to the cloud', () async {
    final backend = testBackend();
    await backend.debugSignInDemoAndUnlock();
    final runtime = FakeLocalAiRuntime(installed: false);
    runtime.state.value = const LocalAiRuntimeStatus(phase: LocalAiPhase.notInstalled, isInitialized: false);
    final delegate = _Delegate();
    final service = DeviceLocalAiService(
      delegate: delegate,
      bridge: delegate,
      runtime: runtime,
      settings: backend.services.settings,
      profiles: backend.services.profiles,
      vault: backend.services.vault,
    );
    final values = <List<AiProviderConfig>>[];
    final subscription = service.watchProviders().listen(values.add);
    await _flush();
    expect(defaultProvider(values.last)?.id, deviceLocalAiProviderId);
    await expectLater(service.draftSnippet(command: 'private command'), throwsA(isA<LocalAiException>()));
    expect(delegate.calls, isEmpty);
    // A completed scan proving this is a fresh installation restores the
    // existing provider selection without ever auto-installing a local model.
    runtime.emit(LocalAiPhase.notInstalled);
    await _flush();
    expect(defaultProvider(values.last)?.id, delegate.providers.value.single.id);
    await subscription.cancel();
    await service.dispose();
    await runtime.dispose();
    await delegate.dispose();
    await backend.dispose();
  });

  test('selecting an uninstalled model retains local default without cloud fallback', () async {
    final backend = testBackend();
    await backend.debugSignInDemoAndUnlock();
    final runtime = FakeLocalAiRuntime();
    final delegate = _Delegate();
    final service = DeviceLocalAiService(
      delegate: delegate,
      bridge: delegate,
      runtime: runtime,
      settings: backend.services.settings,
      profiles: backend.services.profiles,
      vault: backend.services.vault,
    );
    final values = <List<AiProviderConfig>>[];
    final subscription = service.watchProviders().listen(values.add);
    await _flush();
    await runtime.selectModel(LocalAiCatalog.qwen4B.id);
    await _flush();
    expect(defaultProvider(values.last)?.id, deviceLocalAiProviderId);
    expect(values.last.last.chatModel, LocalAiCatalog.qwen4B.id);
    expect(runtime.status.isInstalled, isFalse);
    expect(runtime.status.hasInstalledModels, isTrue);
    final events = await service.chat(providerId: defaultProvider(values.last)!.id, history: const []).toList();
    expect(events.single, isA<AiFailed>());
    await expectLater(service.draftSnippet(command: 'private command'), throwsA(isA<LocalAiException>()));
    expect(delegate.calls, isEmpty, reason: 'no native/cloud request for an uninstalled local selection');
    expect(delegate.snippetProviders, isEmpty);
    expect(backend.services.settings.currentLocal.preferDeviceLocalAi, isTrue);
    await subscription.cancel();
    await service.dispose();
    await runtime.dispose();
    await delegate.dispose();
    await backend.dispose();
  });

  for (final stage in ['startup', 'registration', 'stream']) {
    test('model switch invalidates local $stage and rejects its late response', () async {
      final backend = testBackend();
      await backend.debugSignInDemoAndUnlock();
      final runtime = FakeLocalAiRuntime();
      final oldEndpoint = runtime.endpoint;
      var cancelled = false;
      final delegate = _Delegate()..response = StreamController<AiStreamEvent>(onCancel: () => cancelled = true);
      if (stage == 'startup') runtime.startGate = Completer();
      if (stage == 'registration') delegate.registerGate = Completer();
      final service = DeviceLocalAiService(
        delegate: delegate,
        bridge: delegate,
        runtime: runtime,
        settings: backend.services.settings,
        profiles: backend.services.profiles,
        vault: backend.services.vault,
      );
      await _flush();
      final output = service.chat(providerId: deviceLocalAiProviderId, history: const []).toList();
      await _flush();
      await runtime.selectModel(LocalAiCatalog.qwen9B.id);
      // A generation guard must also reject the interval before the status
      // stream listener has delivered the selection event.
      runtime.startGate?.complete(oldEndpoint);
      delegate.registerGate?.complete();
      await _flush();
      if (stage == 'stream') delegate.response!.add(const AiDelta('stale model output'));
      expect(await output, isEmpty);
      await _flush();
      expect(runtime.leases, 0);
      if (stage == 'stream') {
        expect(cancelled, isTrue);
      } else {
        expect(delegate.calls.where((call) => call.startsWith('chat')), isEmpty);
      }
      if (stage != 'startup') expect(delegate.cleared, contains(delegate.registrations.single.registration));
      await service.dispose();
      await runtime.dispose();
      if (stage != 'stream') {
        // Never-listened single-subscription controllers can close only after
        // listening, unlike the active stream cancelled above.
        delegate.response!.stream.listen((_) {});
      }
      await delegate.dispose();
      await backend.dispose();
    });
  }

  test('cancelling a local stream releases its lease and cancels the underlying stream', () async {
    final backend = testBackend();
    await backend.debugSignInDemoAndUnlock();
    final runtime = FakeLocalAiRuntime();
    var cancelled = false;
    final delegate = _Delegate()..response = StreamController<AiStreamEvent>(onCancel: () => cancelled = true);
    final service = DeviceLocalAiService(
      delegate: delegate,
      bridge: delegate,
      runtime: runtime,
      settings: backend.services.settings,
      profiles: backend.services.profiles,
      vault: backend.services.vault,
    );
    await _flush();
    final subscription = service.chat(providerId: deviceLocalAiProviderId, history: const []).listen((_) {});
    await _flush();
    expect(runtime.leases, 1);
    await subscription.cancel();
    await _flush();
    expect(cancelled, isTrue);
    expect(runtime.leases, 0);
    await service.dispose();
    await runtime.dispose();
    await delegate.response!.close();
    await delegate.dispose();
    await backend.dispose();
  });
}
