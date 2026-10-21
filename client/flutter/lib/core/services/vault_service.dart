import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/secret_text.dart';

/// Vault lifecycle (vault-core via app-core): create, unlock, lock,
/// recovery and trusted-device flows from CLIENT_SPEC §8 / ADR-0004.
abstract interface class VaultService {
  /// Emits the current status first, then changes.
  Stream<VaultStatus> watchStatus();

  VaultStatus get currentStatus;

  /// Creates VRK, password + recovery envelopes and this device's trust
  /// marker, uploads the vault.
  /// Returns the Recovery Kit, which is shown once. The vault is unlocked
  /// but `recoveryKitConfirmed` stays false until [confirmRecoveryKitSaved].
  Future<RecoveryKit> createVault({required String name, required SecretText passphrase});

  /// Called after the user re-entered 3 random words (mandatory, §8.3).
  Future<void> confirmRecoveryKitSaved();

  /// New Recovery Key → `recovery-envelope/replace` (trusted device only).
  Future<RecoveryKit> regenerateRecoveryKit();

  /// Password envelope → VRK. Untrusted devices attest afterwards (K(V)).
  Future<void> unlockWithPassphrase(SecretText passphrase);

  /// The OS (Touch ID / Windows Hello / fingerprint) releases this device's
  /// unlock key after its own prompt with [reason] → VRK. Only when
  /// `deviceUnlockAvailable`. Cancel / failure → `osAuthFailed`; changed
  /// biometrics → `osAuthInvalidated` (device unlock is switched off).
  Future<void> unlockWithDevice({required String reason});

  /// Recheck native availability without presenting an authentication prompt.
  Future<void> refreshDeviceUnlockAvailability();

  /// Device-local, per-profile opt-in. Enabling creates an OS-protected key
  /// (the OS prompts with [reason]); [allowUnprotected] confirms the warning
  /// where the OS offers no protection (Linux keyring).
  Future<void> setDeviceUnlockEnabled(bool enabled, {required String reason, bool allowUnprotected = false});

  /// The user declined switching device unlock on again (after the update
  /// or a biometrics change).
  Future<void> dismissDeviceUnlockOffer();

  /// Zeroises VRK/KEKs in core memory.
  Future<void> lock();

  /// New device: `POST /v1/devices` trust request; phase → awaitingApproval.
  /// The user then compares verification codes on a trusted device.
  Future<void> requestDeviceApproval();

  Future<void> cancelDeviceApproval();

  /// No trusted device, have Recovery Key: mnemonic (24 words) or QR text
  /// → recovery envelope → VRK → attest → new password envelope.
  Future<void> recoverWithRecoveryKey({required SecretText recoveryInput, required SecretText newPassphrase});

  /// Forgot passphrase with device unlock on: the OS releases the unlock key
  /// (prompt with [reason]) → VRK → new password envelope.
  Future<void> resetPassphraseWithTrustedDevice({required SecretText newPassphrase, required String reason});

  Future<void> changePassphrase({required SecretText current, required SecretText next});

  /// Checks the passphrase without changing the lock state (step-up
  /// confirmation for risky actions while unlocked). Wipes [passphrase].
  Future<bool> verifyPassphrase(SecretText passphrase);
}
