import 'package:consolecrypt/core/models/ids.dart';
import 'package:consolecrypt/core/models/snippet.dart';

/// Mirrors `cc_models::ai::AiProviderKind` (CLIENT_SPEC §13).
enum AiProviderKind {
  ollama('ollama', 'Ollama', 'http://localhost:11434', 'llama3.1:8b'),
  lmStudio('lm_studio', 'LM Studio', 'http://localhost:1234/v1', 'qwen2.5-coder-7b'),
  deepseek('deepseek', 'DeepSeek', 'https://api.deepseek.com', 'deepseek-chat'),
  openaiCompatible('openai_compatible', 'OpenAI-compatible', '', '');

  const AiProviderKind(this.wireName, this.label, this.defaultBaseUrl, this.defaultModel);

  final String wireName;
  final String label;
  final String defaultBaseUrl;
  final String defaultModel;

  /// Mirrors `AiProviderKind::is_local_by_default`.
  bool get isLocalByDefault => this == ollama || this == lmStudio;

  bool get usuallyNeedsApiKey => this == deepseek || this == openaiCompatible;
}

/// Mirrors `cc_models::ai::PrivacyProfile` (CLIENT_SPEC §15). Private keys
/// and passwords are never sent under any profile.
enum PrivacyProfile {
  /// Redact secrets and also IPs, hostnames, usernames, DB names.
  strict('strict'),

  /// Redact secrets, keep host metadata.
  standard('standard'),

  /// For local models: more context; secrets still redacted.
  local('local');

  const PrivacyProfile(this.wireName);

  final String wireName;
}

/// Mirrors `cc_models::ai::AiProviderConfig`. The API key lives in a `Secret`
/// (`apiKeySecretId`); the UI can replace it but never reads it back.
final class AiProviderConfig {
  const AiProviderConfig({
    required this.id,
    required this.name,
    required this.provider,
    required this.baseUrl,
    required this.chatModel,
    required this.createdAt,
    required this.updatedAt,
    this.apiKeySecretId,
    this.embeddingModel,
    this.timeoutSecs = 60,
    this.streaming = true,
    this.toolSupport = false,
    this.privacyProfile = PrivacyProfile.strict,
    this.isDefault = false,
  });

  factory AiProviderConfig.draft(AiProviderKind kind) {
    final now = DateTime.now().toUtc();
    return AiProviderConfig(
      id: ObjectId.generate(),
      name: kind.label,
      provider: kind,
      baseUrl: kind.defaultBaseUrl,
      chatModel: kind.defaultModel,
      privacyProfile: kind.isLocalByDefault ? PrivacyProfile.local : PrivacyProfile.strict,
      createdAt: now,
      updatedAt: now,
    );
  }

  final ObjectId id;
  final String name;
  final AiProviderKind provider;
  final String baseUrl;
  final ObjectId? apiKeySecretId;
  final String chatModel;
  final String? embeddingModel;
  final int timeoutSecs;
  final bool streaming;
  final bool toolSupport;
  final PrivacyProfile privacyProfile;
  final bool isDefault;
  final DateTime createdAt;
  final DateTime updatedAt;

  bool get hasApiKey => apiKeySecretId != null;

  /// Whether requests leave this machine (sanitizer is mandatory then).
  bool get isRemote {
    final host = Uri.tryParse(baseUrl)?.host ?? '';
    return !const {'localhost', '127.0.0.1', '::1'}.contains(host);
  }

  AiProviderConfig copyWith({
    String? name,
    AiProviderKind? provider,
    String? baseUrl,
    String? chatModel,
    String? embeddingModel,
    bool clearEmbeddingModel = false,
    int? timeoutSecs,
    bool? streaming,
    bool? toolSupport,
    PrivacyProfile? privacyProfile,
    bool? isDefault,
    ObjectId? apiKeySecretId,
    bool clearApiKey = false,
  }) => AiProviderConfig(
    id: id,
    name: name ?? this.name,
    provider: provider ?? this.provider,
    baseUrl: baseUrl ?? this.baseUrl,
    apiKeySecretId: clearApiKey ? null : (apiKeySecretId ?? this.apiKeySecretId),
    chatModel: chatModel ?? this.chatModel,
    embeddingModel: clearEmbeddingModel ? null : (embeddingModel ?? this.embeddingModel),
    timeoutSecs: timeoutSecs ?? this.timeoutSecs,
    streaming: streaming ?? this.streaming,
    toolSupport: toolSupport ?? this.toolSupport,
    privacyProfile: privacyProfile ?? this.privacyProfile,
    isDefault: isDefault ?? this.isDefault,
    createdAt: createdAt,
    updatedAt: DateTime.now().toUtc(),
  );
}

/// Mirrors `cc_models::ai::ChatRole`.
enum ChatRole { system, user, assistant }

/// Mirrors `cc_models::ai::ChatMessage`.
final class ChatMessage {
  const ChatMessage({required this.role, required this.content, required this.createdAt});

  factory ChatMessage.now(ChatRole role, String content) =>
      ChatMessage(role: role, content: content, createdAt: DateTime.now().toUtc());

  final ChatRole role;
  final String content;
  final DateTime createdAt;
}

/// Mirrors `cc_models::ai::AiConversation` (synced only if enabled).
final class AiConversation {
  const AiConversation({
    required this.id,
    required this.title,
    required this.messages,
    required this.createdAt,
    required this.updatedAt,
    this.providerId,
  });

  final ObjectId id;
  final String title;
  final ObjectId? providerId;
  final List<ChatMessage> messages;
  final DateTime createdAt;
  final DateTime updatedAt;
}

/// What the sanitizer did before a request left the device (§15).
final class SanitizationReport {
  const SanitizationReport({required this.profile, required this.redactions, required this.remote});

  final PrivacyProfile profile;

  /// Number of redacted spans (secrets, and host metadata under Strict).
  final int redactions;

  /// Whether the provider is off-device.
  final bool remote;
}

/// A command proposed by the AI. Never executed automatically (§16).
final class GeneratedCommand {
  const GeneratedCommand({required this.command, required this.explanation, required this.suggestedRisk, this.dialect});

  final String command;
  final String explanation;

  /// The model's own risk guess — a hint only; local rules decide.
  final RiskLevel suggestedRisk;
  final SnippetType? dialect;
}

/// Streaming events from `AiService` (FRB `StreamSink<AiStreamEvent>`).
sealed class AiStreamEvent {
  const AiStreamEvent();
}

final class AiDelta extends AiStreamEvent {
  const AiDelta(this.text);

  final String text;
}

final class AiCompleted extends AiStreamEvent {
  const AiCompleted({required this.fullText, required this.report, this.command});

  final String fullText;
  final SanitizationReport report;
  final GeneratedCommand? command;
}

final class AiFailed extends AiStreamEvent {
  const AiFailed(this.message);

  final String message;
}

/// Allowed context the UI may attach (CLIENT_SPEC §14: host context,
/// selected terminal text, last command). Never secrets.
final class AiContextSelection {
  const AiContextSelection({this.hostId, this.selectedTerminalText, this.lastCommand});

  static const none = AiContextSelection();

  final ObjectId? hostId;
  final String? selectedTerminalText;
  final String? lastCommand;
}

/// `testProvider` result.
final class ProviderHealth {
  const ProviderHealth({required this.ok, required this.message, this.models = const []});

  final bool ok;
  final String message;
  final List<String> models;
}

/// AI-proposed parameterised snippet ("Convert command into snippet").
final class SnippetDraft {
  const SnippetDraft({
    required this.name,
    required this.template,
    required this.snippetType,
    this.description = '',
    this.variables = const [],
    this.tags = const [],
    this.suggestedRisk = RiskLevel.unknown,
  });

  final String name;
  final String description;
  final String template;
  final SnippetType snippetType;
  final List<SnippetVariable> variables;
  final List<String> tags;
  final RiskLevel suggestedRisk;
}
