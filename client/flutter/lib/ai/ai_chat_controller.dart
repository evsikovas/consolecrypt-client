import 'dart:async';

import 'package:consolecrypt/ai/command_palette.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// Why the last chat request failed; the screen turns it into localized text.
enum AiChatError {
  /// No AI provider is configured.
  noProvider,

  /// The request or the provider failed ([AiChatState.errorDetail] may hold
  /// the provider's raw diagnostic).
  requestFailed,
}

final class AiChatState {
  const AiChatState({
    this.providerId,
    this.messages = const [],
    this.streaming,
    this.error,
    this.errorDetail,
    this.stopped = const {},
    this.lastReport,
  });

  final ObjectId? providerId;
  final List<ChatMessage> messages;

  /// Partial assistant answer while streaming.
  final String? streaming;
  final AiChatError? error;

  /// Raw (English/provider) diagnostic shown as secondary detail; never
  /// localized.
  final String? errorDetail;

  /// Indices in [messages] of assistant answers cut short by [AiChatController.stop].
  final Set<int> stopped;
  final SanitizationReport? lastReport;

  bool get isStreaming => streaming != null;

  AiChatState copyWith({
    ObjectId? providerId,
    List<ChatMessage>? messages,
    String? streaming,
    bool clearStreaming = false,
    AiChatError? error,
    String? errorDetail,
    bool clearError = false,
    Set<int>? stopped,
    SanitizationReport? lastReport,
  }) => AiChatState(
    providerId: providerId ?? this.providerId,
    messages: messages ?? this.messages,
    streaming: clearStreaming ? null : (streaming ?? this.streaming),
    error: clearError ? null : (error ?? this.error),
    errorDetail: clearError ? null : (error != null ? errorDetail : this.errorDetail),
    stopped: stopped ?? this.stopped,
    lastReport: lastReport ?? this.lastReport,
  );
}

/// Streaming chat. Conversations currently live in memory only.
// TODO(client/ui): persist conversations as `AiConversation` vault objects when
// "Keep AI conversations in the vault" is on — needs an app-core storage API;
// next: add save/list conversation calls to AiService at M7.
class AiChatController extends Notifier<AiChatState> {
  StreamSubscription<AiStreamEvent>? _sub;

  @override
  AiChatState build() {
    ref.onDispose(() => unawaited(_sub?.cancel()));
    ref.listen(activeProfileProvider.select((p) => p?.id), (prev, next) {
      if (prev != next) clear();
    });
    return const AiChatState();
  }

  /// Selected provider, falling back to the default one.
  ObjectId? effectiveProviderId(List<AiProviderConfig> providers) {
    final selected = state.providerId;
    if (selected != null && providers.any((p) => p.id == selected)) return selected;
    return defaultProvider(providers)?.id;
  }

  void selectProvider(ObjectId id) => state = state.copyWith(providerId: id);

  Future<void> send(String text, {AiContextSelection context = AiContextSelection.none}) async {
    final prompt = text.trim();
    if (prompt.isEmpty || state.isStreaming) return;
    final providers = ref.read(aiProvidersProvider).value ?? const <AiProviderConfig>[];
    final providerId = effectiveProviderId(providers);
    if (providerId == null) {
      state = state.copyWith(error: AiChatError.noProvider);
      return;
    }
    final history = [...state.messages, ChatMessage.now(ChatRole.user, prompt)];
    state = state.copyWith(messages: history, streaming: '', clearError: true, providerId: providerId);
    await _sub?.cancel();
    _sub = ref.read(aiServiceProvider).chat(providerId: providerId, history: history, context: context).listen((event) {
      switch (event) {
        case AiDelta(:final text):
          state = state.copyWith(streaming: (state.streaming ?? '') + text);
        case AiCompleted(:final fullText, :final report):
          state = state.copyWith(
            messages: [...state.messages, ChatMessage.now(ChatRole.assistant, fullText)],
            clearStreaming: true,
            lastReport: report,
          );
        case AiFailed(:final message):
          state = state.copyWith(error: AiChatError.requestFailed, errorDetail: message, clearStreaming: true);
      }
    }, onError: (Object e) => state = state.copyWith(error: AiChatError.requestFailed, clearStreaming: true));
  }

  /// Stops streaming; keeps what arrived so far.
  Future<void> stop() async {
    await _sub?.cancel();
    _sub = null;
    final partial = state.streaming;
    if (partial == null || partial.isEmpty) {
      state = state.copyWith(clearStreaming: true);
      return;
    }
    // The screen appends a localized "…(stopped)" marker to this answer.
    state = state.copyWith(
      messages: [...state.messages, ChatMessage.now(ChatRole.assistant, partial)],
      stopped: {...state.stopped, state.messages.length},
      clearStreaming: true,
    );
  }

  void clear() {
    unawaited(_sub?.cancel());
    _sub = null;
    state = AiChatState(providerId: state.providerId);
  }
}

final aiChatControllerProvider = NotifierProvider<AiChatController, AiChatState>(AiChatController.new);
