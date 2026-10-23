import 'package:consolecrypt/core/models/step_up.dart';
import 'package:consolecrypt/core/services/errors.dart';

/// Presence tokens of the demo backend: the mock vault mints them after its
/// simulated OS prompt, the mock services redeem them like the core does
/// (single use, bound to the action and the object).
abstract final class MockPresenceTokens {
  static final _tokens = <String, (StepUpAction, String)>{};
  static var _next = 0;

  static String mint(StepUpAction action, String object) {
    final token = 'mock-presence-${++_next}'; // l10n-ignore: demo token
    _tokens[token] = (action, object);
    return token;
  }

  static bool redeem(String token, StepUpAction action, String object) => _tokens.remove(token) == (action, object);

  /// Whether [proof] checks out: the [passphrase] or a token for [action]
  /// on [object]. A refused token throws `passphraseRequired`. Wipes it.
  static bool check(StepUpProof proof, String passphrase, StepUpAction action, String object) {
    try {
      return switch (proof) {
        PassphraseProof(passphrase: final typed) => typed.expose() == passphrase,
        PresenceProof(:final token) =>
          redeem(token, action, object) ||
              (throw const AppException(
                AppErrorCode.passphraseRequired,
                'Confirm with the vault passphrase', // l10n-ignore: diagnostic
              )),
      };
    } finally {
      proof.wipe();
    }
  }
}
