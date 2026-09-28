import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/secret_text.dart';

export 'package:consolecrypt/core/models/prompt.dart';

/// Core prompts raised outside terminal tabs (SFTP, tunnels, exec, AI runs):
/// unknown host keys, ask-at-connect passwords and SSH key passphrases.
///
/// The UI mounts one presenter (a dialog host near the app root) and calls
/// [attachPresenter]; while no presenter is attached, prompts are declined
/// at once so a connection never hangs until the core's timeout. Terminal
/// tabs still claim the prompts of the session they are connecting.
abstract interface class PromptService {
  /// Pending prompts, oldest first (current value first, then changes). An
  /// answered, timed-out or cancelled prompt disappears.
  Stream<List<CorePrompt>> watchPending();

  List<CorePrompt> get pending;

  /// Answer a [HostKeyCorePrompt].
  Future<void> answerHostKey(String requestId, HostKeyDecision decision);

  /// Answer a [PasswordCorePrompt] / [PassphraseCorePrompt]; `null` cancels.
  /// [secret] is wiped afterwards.
  Future<void> answerSecret(String requestId, SecretText? secret);

  /// Register a presenter; returns the function that detaches it.
  void Function() attachPresenter();
}
