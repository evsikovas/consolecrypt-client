import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/secret_text.dart';

/// Vault lifecycle (vault-core via app-core): create, unlock, lock,
/// recovery and trusted-device flows from CLIENT_SPEC §8 / ADR-0004.
abstract interface class VaultService {
  /// Emits the current status first, then changes.
  Stream<VaultStatus> watchStatus();

  VaultStatus get currentStatus;

  /// Creates VRK, password + recovery + device envelopes, uploads the vault.
  /// Returns the Recovery Kit, which is shown once. The vault is unlocked
  /// but `recoveryKitConfirmed` stays false until [confirmRecoveryKitSaved].
  Future<RecoveryKit> createVault({required String name, required SecretText passphrase});

  /// Called after the user re-entered 3 random words (mandatory, §8.3).
  Future<void> confirmRecoveryKitSaved();

  /// New Recovery Key → `recovery-envelope/replace` (trusted device only).
  Future<RecoveryKit> regenerateRecoveryKit();

  /// Password envelope → VRK. Untrusted devices attest afterwards (K(V)).
  Future<void> unlockWithPassphrase(SecretText passphrase);

  /// OS authentication (Touch ID / Windows Hello) → device key → device
  /// envelope → VRK. Only when `deviceUnlockAvailable`.
  Future<void> unlockWithDevice({required String reason});

  /// Recheck native availability without presenting an authentication prompt.
  Future<void> refreshDeviceUnlockAvailability();

  /// Device-local, per-profile opt-in; enabling requires fresh OS auth.
  Future<void> setDeviceUnlockEnabled(bool enabled, {required String reason});

  /// Zeroises VRK/KEKs in core memory.
  Future<void> lock();

  /// New device: `POST /v1/devices` trust request; phase → awaitingApproval.
  /// The user then compares verification codes on a trusted device.
  Future<void> requestDeviceApproval();

  Future<void> cancelDeviceApproval();

  /// No trusted device, have Recovery Key: mnemonic (24 words) or QR text
  /// → recovery envelope → VRK → attest → new password envelope.
  Future<void> recoverWithRecoveryKey({required SecretText recoveryInput, required SecretText newPassphrase});

  /// Forgot passphrase on a trusted device: OS auth → device envelope →
  /// VRK → new password envelope (`password-envelope/replace`).
  Future<void> resetPassphraseWithTrustedDevice({required SecretText newPassphrase});

  Future<void> changePassphrase({required SecretText current, required SecretText next});
}
