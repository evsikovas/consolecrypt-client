import 'dart:async';

import 'package:consolecrypt/ai/ai_chat_controller.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/ai_service.dart';
import 'package:consolecrypt/core/util/value_stream.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

class _Ai extends Fake implements AiService {
  final streams = <StreamController<AiStreamEvent>>[];
  final cancellations = <Completer<void>>[];
  final requests = <String>[];

  @override
  Stream<AiStreamEvent> chat({
    required ObjectId providerId,
    required List<ChatMessage> history,
    AiContextSelection context = AiContextSelection.none,
  }) {
    requests.add(history.last.content);
    final cancel = Completer<void>();
    cancellations.add(cancel);
    final stream = StreamController<AiStreamEvent>(onCancel: () => cancel.future);
    streams.add(stream);
    return stream.stream;
  }

  @override
  Future<AiProviderConfig> saveProvider(AiProviderConfig config, {SecretText? apiKey, bool clearApiKey = false}) =>
      throw UnimplementedError();

  void finishCancellation() {
    for (final cancel in cancellations) {
      if (!cancel.isCompleted) cancel.complete();
    }
  }
}

void main() {
  late ProviderContainer container;
  late ValueStreamController<ProfilesState> profiles;
  late ValueStreamController<VaultStatus> vault;
  late _Ai ai;
  late AiChatController controller;
  late Profile profile;

  setUp(() async {
    profile = Profile(
      id: ProfileId.generate(),
      name: 'First workspace',
      kind: ProfileKind.local,
      vaultId: VaultId.generate(),
      createdAt: DateTime.now().toUtc(),
    );
    profiles = ValueStreamController(ProfilesState(profiles: [profile], activeId: profile.id));
    vault = ValueStreamController(VaultStatus(phase: VaultPhase.unlocked, vaultId: profile.vaultId));
    ai = _Ai();
    final provider = AiProviderConfig.draft(AiProviderKind.ollama).copyWith(isDefault: true);
    container = ProviderContainer(
      overrides: [
        profilesProvider.overrideWith((ref) => profiles.stream),
        vaultStatusProvider.overrideWith((ref) => vault.stream),
        aiProvidersProvider.overrideWith((ref) => Stream.value([provider])),
        aiServiceProvider.overrideWithValue(ai),
      ],
    );
    container.listen(profilesProvider, (_, _) {});
    container.listen(vaultStatusProvider, (_, _) {});
    container.listen(aiProvidersProvider, (_, _) {});
    await container.read(profilesProvider.future);
    await container.read(vaultStatusProvider.future);
    await container.read(aiProvidersProvider.future);
    controller = container.read(aiChatControllerProvider.notifier);
  });

  tearDown(() async {
    ai.finishCancellation();
    container.dispose();
    for (final stream in ai.streams) {
      await stream.close();
    }
    await profiles.close();
    await vault.close();
  });

  test('locking clears the transcript and cancels the active request', () async {
    await controller.send('Explain the selected output');
    ai.streams.single.add(const AiDelta('Partial answer'));
    await pumpEventQueue();
    expect(container.read(aiChatControllerProvider).isStreaming, isTrue);
    vault.value = VaultStatus(phase: VaultPhase.locked, vaultId: profile.vaultId);
    await pumpEventQueue();
    expect(container.read(aiChatControllerProvider).messages, isEmpty);
    expect(container.read(aiChatControllerProvider).streaming, isNull);
    expect(ai.streams.single.hasListener, isFalse);
    await controller.send('Must not start while locked');
    expect(ai.requests, hasLength(1));
  });

  test('profile switch while a previous cancellation waits cannot send the old prompt', () async {
    await controller.send('Initial question');
    ai.streams.single.add(
      const AiCompleted(
        fullText: 'Initial answer',
        report: SanitizationReport(profile: PrivacyProfile.local, redactions: 0, remote: false),
      ),
    );
    await pumpEventQueue();
    final pending = controller.send('Old workspace question');
    final other = Profile(
      id: ProfileId.generate(),
      name: 'Second workspace',
      kind: ProfileKind.local,
      vaultId: VaultId.generate(),
      createdAt: DateTime.now().toUtc(),
    );
    profiles.value = ProfilesState(profiles: [profile, other], activeId: other.id);
    await pumpEventQueue();
    ai.finishCancellation();
    await pending;
    expect(ai.requests, hasLength(1), reason: 'cancellation must invalidate the pending request');
    expect(container.read(aiChatControllerProvider).messages, isEmpty);
    expect(container.read(aiChatControllerProvider).streaming, isNull);
  });

  test('a pending stop cannot restore a transcript after clearing it', () async {
    await controller.send('Initial question');
    ai.streams.single.add(const AiDelta('Partial answer'));
    await pumpEventQueue();
    final stopping = controller.stop();
    controller.clear();
    ai.finishCancellation();
    await stopping;
    expect(container.read(aiChatControllerProvider).messages, isEmpty);
    expect(container.read(aiChatControllerProvider).streaming, isNull);
  });
}
