import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// Step-up confirmations with the OS check (Touch ID / Windows Hello /
/// fingerprint) instead of the vault passphrase. The core releases the
/// device unlock key behind the OS prompt, checks it and mints a single-use
/// presence token; the key itself never reaches the app.
abstract interface class UserPresence {
  /// The OS check offered for [action] now; null = passphrase only (device
  /// unlock off or unprotected, preference off, passphrase-only action).
  DeviceAuthKind? kindFor(StepUpAction action);

  /// The OS prompt (with [reason]) → a presence token for [action] on
  /// [object], valid once for 30 s. Throws [AppException] on cancel,
  /// failure, lockout or when the core does not offer it.
  Future<String> verify(StepUpAction action, String object, {required String reason});

  /// For confirmations the app enforces itself: checks [proof] now (a
  /// token is used up). False when it does not check out.
  Future<bool> confirm(StepUpProof proof, {required StepUpAction action, required String object});
}

final userPresenceProvider = Provider<UserPresence>(
  (ref) => VaultUserPresence(ref.watch(vaultServiceProvider), ref.watch(vaultStatusProvider).value?.deviceUnlock),
);

final class VaultUserPresence implements UserPresence {
  const VaultUserPresence(this._vault, this._info);
  final VaultService _vault;
  final DeviceUnlockInfo? _info;

  @override
  DeviceAuthKind? kindFor(StepUpAction action) {
    final info = _info;
    if (action.passphraseOnly || info == null || !info.presenceAvailable) return null;
    return info.kind;
  }

  @override
  Future<String> verify(StepUpAction action, String object, {required String reason}) {
    if (kindFor(action) == null) {
      throw const AppException(
        AppErrorCode.passphraseRequired,
        'Confirm with the vault passphrase', // l10n-ignore: diagnostic
      );
    }
    return _vault.verifyUserPresence(action: action, object: object, reason: reason);
  }

  @override
  Future<bool> confirm(StepUpProof proof, {required StepUpAction action, required String object}) => switch (proof) {
    PassphraseProof(:final passphrase) => _vault.verifyPassphrase(passphrase),
    PresenceProof() => _vault.confirmStepUp(proof, action: action, object: object),
  };
}
