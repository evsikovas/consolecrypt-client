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
