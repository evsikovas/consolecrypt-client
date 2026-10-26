import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/secret_text.dart';

/// LLM providers and AI features (ai-core). Everything sent to a remote
/// provider passes the Context Sanitizer first (§15). The AI never runs
/// commands: callers only Insert/Run after explicit user action (§16).
abstract interface class AiService {
  Stream<List<AiProviderConfig>> watchProviders();

  /// [apiKey] replaces the stored key (kept as a Secret); the existing key
  /// is never returned to the UI. [clearApiKey] removes it.
  Future<AiProviderConfig> saveProvider(AiProviderConfig config, {SecretText? apiKey, bool clearApiKey = false});

  Future<void> deleteProvider(ObjectId id);

  Future<ProviderHealth> testProvider(ObjectId id);

  /// Streaming chat completion.
  Stream<AiStreamEvent> chat({
    required ObjectId providerId,
    required List<ChatMessage> history,
    AiContextSelection context = AiContextSelection.none,
  });

  /// "Generate Command": streams the answer; [AiCompleted.command] carries
  /// the proposed command.
  Stream<AiStreamEvent> generateCommand({
    required ObjectId providerId,
    required String request,
    SnippetType? dialect,
    AiContextSelection context = AiContextSelection.none,
  });

  /// "Convert terminal command into parameterized snippet".
  Future<SnippetDraft> draftSnippet({required String command, ObjectId? providerId});
}

/// Optional capability: Ask AI conversations of this unlock and those kept in
/// the vault ("Keep AI conversations in the vault"), to continue or delete.
abstract interface class AiConversationHistory {
  /// Newest first.
  Future<List<AiConversation>> listConversations();

  Future<void> deleteConversation(ObjectId id);

  /// The next [AiService.chat] with [providerId] and more than one user turn
  /// continues [id]: the core gives the model its stored turns again.
  void continueConversation(ObjectId providerId, ObjectId id);
}

/// Optional production capability. Endpoint and auth live only in an unlocked
/// native AI session; calling this never saves an AiProvider/Secret vault object.
abstract interface class DeviceLocalAiBridge {
  Future<void> registerDeviceLocalProvider({
    required String profileId,
    required String vaultId,
    required String registrationId,
    required int port,
    required String apiKey,
    required String model,
  });

  Future<void> clearDeviceLocalProvider(String registrationId);
}

const deviceLocalAiProviderId = ObjectId('00000000-0000-4000-8000-000000000a11');
