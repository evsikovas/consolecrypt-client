import 'dart:async';

import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/prompt_service.dart';
import 'package:consolecrypt/core/util/value_stream.dart';

/// In-memory [PromptService]. Mock connections never raise prompts on their
/// own; demos and widget tests push them with [simulate] and read the
/// answers from [answers].
final class MockPromptService implements PromptService {
  final _pending = ValueStreamController<List<CorePrompt>>(const []);
  int _presenters = 0;

  /// Answers given so far: request id → `HostKeyDecision`, `'secret'` or
  /// `'cancelled'` (secret values are never kept).
  final Map<String, Object> answers = {};

  /// Whether a presenter is attached.
  bool get hasPresenter => _presenters > 0;

  /// Raises [prompt]; without a presenter it is declined at once (like the
  /// real core). Returns whether it is shown.
  bool simulate(CorePrompt prompt) {
    if (_presenters == 0) {
      answers[prompt.requestId] = prompt is HostKeyCorePrompt ? HostKeyDecision.reject : 'cancelled';
      return false;
    }
    _pending.value = List.unmodifiable([..._pending.value, prompt]);
    return true;
  }

  bool _remove(String requestId) {
    final before = _pending.value;
    final after = [
      for (final p in before)
        if (p.requestId != requestId) p,
    ];
    if (after.length == before.length) return false;
    _pending.value = List.unmodifiable(after);
    return true;
  }

  @override
  Stream<List<CorePrompt>> watchPending() => _pending.stream;

  @override
  List<CorePrompt> get pending => _pending.value;

  @override
  Future<void> answerHostKey(String requestId, HostKeyDecision decision) async {
    if (_remove(requestId)) answers[requestId] = decision;
  }

  @override
  Future<void> answerSecret(String requestId, SecretText? secret) async {
    if (_remove(requestId)) answers[requestId] = secret == null ? 'cancelled' : 'secret';
    secret?.wipe();
  }

  @override
  void Function() attachPresenter() {
    _presenters++;
    var detached = false;
    return () {
      if (detached) return;
      detached = true;
      _presenters--;
      if (_presenters == 0) {
        for (final p in [..._pending.value]) {
          if (p is HostKeyCorePrompt) {
            unawaited(answerHostKey(p.requestId, HostKeyDecision.reject));
          } else {
            unawaited(answerSecret(p.requestId, null));
          }
        }
      }
    };
  }

  Future<void> dispose() => _pending.close();
}
