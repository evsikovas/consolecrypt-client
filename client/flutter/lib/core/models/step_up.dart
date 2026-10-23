import 'package:consolecrypt/core/security/secret_text.dart';

/// What a step-up confirmation is for (core `StepUpAction`, wire names in
/// [wire]). The OS check (Touch ID / Windows Hello / fingerprint) mints a
/// single-use presence token bound to the action and its object.
enum StepUpAction {
  /// A change the app confirms before it calls the core (Kubernetes
  /// changes, reviewed database statements, …).
  confirm('confirm'),

  /// Production write window of a database session (object: session id).
  dbProductionWrite('db_production_write'),

  /// Container environment / unmasked inspect (object: `<session>/<container>`).
  dockerReveal('docker_reveal'),

  /// Editing a Compose project (object: `<session>/<project>`).
  dockerCompose('docker_compose'),
  changePassphrase('change_passphrase'),
  recoveryKit('recovery_kit'),
  disableDeviceUnlock('disable_device_unlock'),
  backupExport('backup_export'),
  backupRestore('backup_restore'),
  approveDevice('approve_device'),
  accountSecurity('account_security'),
  enterpriseAccount('enterprise_account');

  const StepUpAction(this.wire);
  final String wire;

  /// See [passphraseOnlyStepUps].
  bool get passphraseOnly => passphraseOnlyStepUps.contains(this);
}

/// Confirmed with the vault passphrase only, never with the OS check: they
/// change who or what can open the vault (passphrase, Recovery Kit, device
/// unlock itself, a new device), move the whole vault out or in (backups)
/// or change the account / organisation sign-in. A stolen unlocked device
/// with a known device PIN must not be enough for them. The core keeps the
/// same list (`presence.rs`, `PASSPHRASE_ONLY`) and refuses tokens for them.
const passphraseOnlyStepUps = {
  StepUpAction.changePassphrase,
  StepUpAction.recoveryKit,
  StepUpAction.disableDeviceUnlock,
  StepUpAction.backupExport,
  StepUpAction.backupRestore,
  StepUpAction.approveDevice,
  StepUpAction.accountSecurity,
  StepUpAction.enterpriseAccount,
};

/// What a core-enforced step-up gets: the vault passphrase or a presence
/// token from `VaultService.verifyUserPresence`.
sealed class StepUpProof {
  const StepUpProof();

  /// Wipes the passphrase copy (a token is single-use anyway).
  void wipe();
}

final class PassphraseProof extends StepUpProof {
  const PassphraseProof(this.passphrase);
  final SecretText passphrase;

  @override
  void wipe() => passphrase.wipe();

  @override
  String toString() => 'PassphraseProof(<redacted>)';
}

final class PresenceProof extends StepUpProof {
  const PresenceProof(this.token);
  final String token;

  @override
  void wipe() {}

  @override
  String toString() => 'PresenceProof(<redacted>)';
}
