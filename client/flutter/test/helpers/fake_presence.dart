import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/vault/user_presence.dart';

/// Stands in for the core: the OS check either passes (a token bound to
/// action and object) or fails like a cancelled / locked-out prompt.
final class FakePresence implements UserPresence {
  FakePresence({this.kind = DeviceAuthKind.touchId});
  DeviceAuthKind? kind;
  AppErrorCode? failure;
  final verified = <(StepUpAction, String, String)>[];
  final confirmed = <StepUpProof>[];

  @override
  DeviceAuthKind? kindFor(StepUpAction action) => action.passphraseOnly ? null : kind;

  @override
  Future<String> verify(StepUpAction action, String object, {required String reason}) async {
    verified.add((action, object, reason));
    if (failure case final code?) throw AppException(code, 'presence failed');
    return 'presence:${action.wire}:$object';
  }

  @override
  Future<bool> confirm(StepUpProof proof, {required StepUpAction action, required String object}) async {
    confirmed.add(proof);
    return switch (proof) {
      PassphraseProof(:final passphrase) => passphrase.expose() == 'vault-pass',
      PresenceProof(:final token) => token == 'presence:${action.wire}:$object',
    };
  }
}
