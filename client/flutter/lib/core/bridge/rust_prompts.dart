import 'dart:async';

import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/bridge/rust_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/services/prompt_service.dart';
import 'package:consolecrypt/core/util/value_stream.dart';
import 'package:consolecrypt/src/rust/api/app.dart' as rs_app;

/// [PromptService] over the core's prompt stream: receives the prompts no
/// terminal tab claimed (`RustBackend.onUnclaimedPrompt`) while a presenter
/// is attached; without one they are declined at once.
final class RustPromptService implements PromptService {
  RustPromptService(this._hub) {
    _hub
      ..onUnclaimedPrompt = _claim
      ..onSessionReset.add(_declineAll);
  }

  /// Matches app-core's default prompt timeout; the core treats an
  /// unanswered prompt as rejected / cancelled after it.
  static const timeout = Duration(minutes: 5);

  final RustBackend _hub;
  final _pending = ValueStreamController<List<CorePrompt>>(const []);
  final Map<String, Timer> _expiry = {};
  int _presenters = 0;

  Future<bool> _claim(Json json) async {
    if (_presenters == 0) return false;
    final prompt = corePromptFromJson(json);
    if (prompt == null) return false;
    _pending.value = List.unmodifiable([..._pending.value, prompt]);
    _expiry[prompt.requestId] = Timer(timeout, () => _remove(prompt.requestId));
    return true;
  }

  bool _remove(String requestId) {
    _expiry.remove(requestId)?.cancel();
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
    if (!_remove(requestId)) return;
    final answer = switch (decision) {
      HostKeyDecision.acceptAndSave => rs_app.HostKeyAnswer.acceptAndSave,
      HostKeyDecision.acceptOnce => rs_app.HostKeyAnswer.acceptOnce,
      HostKeyDecision.reject => rs_app.HostKeyAnswer.reject,
    };
    try {
      await guard(() => rs_app.promptAnswerHostKey(requestId: requestId, answer: answer));
    } on AppException catch (e) {
      // Timed out in the core meanwhile.
      if (e.code != AppErrorCode.notFound) rethrow;
    }
  }

  @override
  Future<void> answerSecret(String requestId, SecretText? secret) async {
    final prompt = _pending.value.where((p) => p.requestId == requestId).firstOrNull;
    if (prompt == null || !_remove(requestId)) {
      secret?.wipe();
      return;
    }
    final bytes = secret?.exposeBytes();
    secret?.wipe();
    try {
      await guard(
        () => prompt is PassphraseCorePrompt
            ? rs_app.promptAnswerPassphrase(requestId: requestId, passphrase: bytes)
            : rs_app.promptAnswerPassword(requestId: requestId, password: bytes),
      );
    } on AppException catch (e) {
      if (e.code != AppErrorCode.notFound) rethrow;
    } finally {
      bytes?.fillRange(0, bytes.length, 0);
    }
  }

  @override
  void Function() attachPresenter() {
    _presenters++;
    var detached = false;
    return () {
      if (detached) return;
      detached = true;
      _presenters--;
      if (_presenters == 0) _declineAll();
    };
  }

  void _declineAll() {
    for (final p in [..._pending.value]) {
      if (p is HostKeyCorePrompt) {
        unawaited(_quiet(answerHostKey(p.requestId, HostKeyDecision.reject)));
      } else {
        unawaited(_quiet(answerSecret(p.requestId, null)));
      }
    }
  }

  static Future<void> _quiet(Future<void> f) async {
    try {
      await f;
    } on Object {
      // Already answered / timed out.
    }
  }

  Future<void> dispose() async {
    _declineAll();
    for (final t in _expiry.values) {
      t.cancel();
    }
    _expiry.clear();
    await _pending.close();
  }
}
