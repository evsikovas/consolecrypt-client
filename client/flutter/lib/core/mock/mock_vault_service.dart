import 'dart:async';

import 'package:consolecrypt/core/mock/mock_cloud.dart';
import 'package:consolecrypt/core/mock/mock_config.dart';
import 'package:consolecrypt/core/mock/mock_presence.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/passphrase_strength.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/core/util/value_stream.dart';

final class MockVaultService implements VaultService {
  MockVaultService(this._cloud) {
    _sub = _cloud.changed.listen((_) => _recompute());
    _recompute();
  }

  final MockCloud _cloud;
  final ValueStreamController<VaultStatus> _status = ValueStreamController(VaultStatus.none);
  late final StreamSubscription<void> _sub;
  Timer? _autoApprove;
  Object? _signature;

  /// Test/demo platform capability; production always queries the OS.
  DeviceAuthKind? deviceAuthKind = DeviceAuthKind.touchId;
  bool deviceAuthNotEnrolled = false;
  bool deviceAuthSucceeds = true;
  DeviceUnlockProtection deviceProtection = DeviceUnlockProtection.system;

  /// Demo security key (FEAT-16), off by default: the demo backend has no
  /// FIDO2 transport. [securityKeyHolds] keeps every key request waiting
  /// for a touch that never comes (guide capture of the touch prompt).
  bool _securityKeySupported = false;
  bool _securityKeyEnabled = false;
  bool securityKeyHolds = false;

  void debugSecurityKey({bool supported = true, bool enabled = false}) {
    _securityKeySupported = supported;
    _securityKeyEnabled = enabled;
    _cloud.notifyChanged();
  }

  /// Simulates a new fingerprint: the OS key of the active profile is gone.
  void simulateBiometricChange() {
    final r = _cloud.activeRecord;
    if (r == null || !r.deviceUnlockEnabled) return;
    r
      ..deviceUnlockEnabled = false
      ..deviceUnlockReoffer = DeviceUnlockReoffer.invalidated;
    _cloud.notifyChanged();
  }

  void _recompute() {
    final r = _cloud.activeRecord;
    final signature = (
      r?.profile.id,
      r?.phase,
      r?.vault?.vaultId,
      r?.vault?.name,
      r?.recoveryKitConfirmed,
      r?.deviceTrusted,
      r?.deviceUnlockEnabled,
      r?.deviceUnlockConfirmActions,
      r?.deviceUnlockReoffer,
      deviceAuthKind,
      deviceAuthNotEnrolled,
      deviceProtection,
      _securityKeySupported,
      _securityKeyEnabled,
    );
    if (signature == _signature) return;
    _signature = signature;
    if (r == null) {
      _status.value = VaultStatus.none;
      return;
    }
    _status.value = VaultStatus(
      phase: r.phase,
      vaultId: r.vault?.vaultId,
      vaultName: r.vault?.name,
      recoveryKitConfirmed: r.recoveryKitConfirmed,
      deviceTrusted: r.deviceTrusted,
      deviceUnlock: DeviceUnlockInfo(
        enabled: r.deviceUnlockEnabled,
        kind: deviceAuthKind,
        hasVault: r.vault != null && r.deviceTrusted,
        notEnrolled: deviceAuthNotEnrolled,
        protection: deviceProtection,
        reoffer: r.deviceUnlockReoffer,
        confirmActions: r.deviceUnlockEnabled && r.deviceUnlockConfirmActions,
        presenceAvailable:
            r.deviceUnlockEnabled &&
            r.deviceUnlockConfirmActions &&
            deviceProtection == DeviceUnlockProtection.system &&
            deviceAuthKind != null &&
            r.phase == VaultPhase.unlocked,
      ),
      securityKey: SecurityKeyInfo(
        supported: _securityKeySupported,
        enabled: _securityKeyEnabled,
        pinPrompt: _securityKeySupported ? SecurityKeyPinPrompt.app : null,
        hasVault: r.vault != null && r.deviceTrusted,
      ),
    );
  }

  MockProfileRecord _active() {
    final r = _cloud.activeRecord;
    if (r == null) {
      throw const AppException(AppErrorCode.notFound, 'No active profile', reason: AppErrorReason.noActiveProfile);
    }
    return r;
  }

  MockVaultKeys _keys(MockProfileRecord r) {
    final keys = r.vault;
    if (keys == null) {
      throw const AppException(AppErrorCode.notFound, 'This profile has no vault yet', reason: AppErrorReason.noVault);
    }
    return keys;
  }

  Uri? _kitUrl(MockProfileRecord r) => r.profile.serverUrl;

  void _checkNewPassphrase(SecretText passphrase) {
    if (!PassphraseStrength.estimate(passphrase.expose()).acceptable) {
      throw const AppException(
        AppErrorCode.validation,
        'Choose a stronger passphrase (at least 12 characters, strength "Strong")',
      );
    }
  }

  @override
  Stream<VaultStatus> watchStatus() => _status.stream;

  @override
  VaultStatus get currentStatus => _status.value;

  @override
  Future<RecoveryKit> createVault({required String name, required SecretText passphrase}) async {
    final r = _active();
    if (r.vault != null) {
      throw const AppException(AppErrorCode.conflict, 'A vault already exists', reason: AppErrorReason.vaultExists);
    }
    _checkNewPassphrase(passphrase);
    await mockDelay(_cloud.config.kdfLatency);
    final keys = _cloud.newVaultKeys(name.trim().isEmpty ? 'Personal' : name.trim(), passphrase.expose());
    if (r.profile.isSynced) {
      _cloud.accounts[r.profile.accountEmail]?.vault = keys; // uploaded (POST /v1/vaults)
    }
    final p = r.profile;
    r
      ..vault = keys
      ..profile = Profile(
        id: p.id,
        name: p.name,
        kind: p.kind,
        serverUrl: p.serverUrl,
        accountEmail: p.accountEmail,
        deviceId: p.deviceId,
        vaultId: keys.vaultId,
        createdAt: p.createdAt,
      )
      ..phase = VaultPhase.unlocked
      ..recoveryKitConfirmed = false
      ..deviceTrusted = true;
    _cloud.publishProfiles();
    return _cloud.kitFor(keys, _kitUrl(r));
  }

  @override
  Future<void> confirmRecoveryKitSaved() async {
    _active().recoveryKitConfirmed = true;
    _cloud.notifyChanged();
  }

  @override
  Future<RecoveryKit> regenerateRecoveryKit() async {
    final r = _active();
    final keys = _keys(r);
    if (!r.deviceTrusted || r.phase != VaultPhase.unlocked) {
      throw const AppException(
        AppErrorCode.deviceNotTrusted,
        'Unlock the vault on a trusted device first',
        reason: AppErrorReason.trustedDeviceRequired,
      );
    }
    await mockDelay(_cloud.config.latency);
    final fresh = _cloud.newVaultKeys(keys.name, keys.passphrase, id: keys.vaultId);
    keys
      ..words = fresh.words
      ..qrPayload = fresh.qrPayload;
    return _cloud.kitFor(keys, _kitUrl(r));
  }

  @override
  Future<void> unlockWithPassphrase(SecretText passphrase) async {
    final r = _active();
    final keys = _keys(r);
    await mockDelay(_cloud.config.kdfLatency);
    if (passphrase.expose() != keys.passphrase) {
      throw const AppException(AppErrorCode.wrongPassphrase, 'Wrong passphrase');
    }
    r
      ..phase = VaultPhase.unlocked
      ..deviceTrusted = true; // attest via VAK (ADR-0004)
    _cloud.notifyChanged();
  }

  @override
  Future<void> unlockWithDevice({required String reason}) async {
    final r = _active();
    _keys(r);
    if (!r.deviceTrusted) {
      throw const AppException(AppErrorCode.deviceNotTrusted, 'This device is not trusted for the vault yet');
    }
    _checkDeviceUnlock();
    await mockDelay(_cloud.config.latency);
    r.phase = VaultPhase.unlocked;
    _cloud.notifyChanged();
  }

  void _checkDeviceUnlock() {
    if (!currentStatus.deviceUnlockAvailable) {
      throw const AppException(AppErrorCode.unsupported, 'Device unlock is not enabled or available');
    }
    if (!deviceAuthSucceeds) {
      throw const AppException(AppErrorCode.osAuthFailed, 'OS authentication cancelled');
    }
  }

  @override
  Future<void> refreshDeviceUnlockAvailability() async => _recompute();

  @override
  Future<void> setDeviceUnlockEnabled(bool enabled, {required String reason, bool allowUnprotected = false}) async {
    final r = _active();
    if (r.phase != VaultPhase.unlocked) {
      throw const AppException(AppErrorCode.notFound, 'Unlock the vault first', reason: AppErrorReason.vaultLocked);
    }
    if (enabled && (!currentStatus.deviceUnlock.canEnable || !deviceAuthSucceeds)) {
      throw const AppException(AppErrorCode.osAuthFailed, 'OS authentication unavailable or cancelled');
    }
    if (enabled && deviceProtection == DeviceUnlockProtection.convenience && !allowUnprotected) {
      throw const AppException(AppErrorCode.validation, 'Confirm unlock without OS protection first');
    }
    r.deviceUnlockEnabled = enabled;
    // A new device unlock key confirms actions by default.
    if (enabled) r.deviceUnlockConfirmActions = true;
    r.deviceUnlockReoffer = null;
    _cloud.notifyChanged();
  }

  @override
  Future<String> verifyUserPresence({
    required StepUpAction action,
    required String object,
    required String reason,
  }) async {
    if (action.passphraseOnly || !currentStatus.deviceUnlock.presenceAvailable) {
      throw const AppException(AppErrorCode.passphraseRequired, 'Confirm with the vault passphrase');
    }
    await mockDelay(_cloud.config.latency);
    if (!deviceAuthSucceeds) {
      throw const AppException(AppErrorCode.osAuthFailed, 'OS authentication cancelled');
    }
    return MockPresenceTokens.mint(action, object);
  }

  @override
  Future<bool> confirmStepUp(StepUpProof proof, {required StepUpAction action, required String object}) async {
    final keys = _keys(_active());
    if (proof is PresenceProof && action.passphraseOnly) return false;
    try {
      return MockPresenceTokens.check(proof, keys.passphrase, action, object);
    } on AppException {
      return false;
    }
  }

  @override
  Future<void> setConfirmActionsWithPresence(bool enabled) async {
    final r = _active();
    if (!r.deviceUnlockEnabled) {
      throw const AppException(AppErrorCode.unsupported, 'Device unlock is not enabled');
    }
    r.deviceUnlockConfirmActions = enabled;
    _cloud.notifyChanged();
  }

  @override
  Future<void> dismissDeviceUnlockOffer() async {
    _active().deviceUnlockReoffer = null;
    _cloud.notifyChanged();
  }

  @override
  Future<void> unlockWithSecurityKey({SecretText? pin}) async {
    pin?.wipe();
    if (!_securityKeySupported || !_securityKeyEnabled) {
      throw const AppException(AppErrorCode.unsupported, 'Security keys need the native app');
    }
    final r = _active();
    _keys(r);
    if (securityKeyHolds) await Completer<void>().future;
    await mockDelay(_cloud.config.latency);
    r.phase = VaultPhase.unlocked;
    _cloud.notifyChanged();
  }

  @override
  Future<void> setSecurityKeyEnabled(bool enabled, {SecretText? pin}) async {
    pin?.wipe();
    if (!_securityKeySupported) {
      throw const AppException(AppErrorCode.unsupported, 'Security keys need the native app');
    }
    if (_active().phase != VaultPhase.unlocked) {
      throw const AppException(AppErrorCode.notFound, 'Unlock the vault first', reason: AppErrorReason.vaultLocked);
    }
    if (enabled && securityKeyHolds) await Completer<void>().future;
    _securityKeyEnabled = enabled;
    _cloud.notifyChanged();
  }

  @override
  Future<void> lock() async {
    _cloud.lockActive();
    _cloud.notifyChanged();
  }

  @override
  Future<void> requestDeviceApproval() async {
    final r = _active();
    if (!r.profile.isSynced) {
      throw const AppException(
        AppErrorCode.unsupported,
        'Device approval needs a synced profile',
        reason: AppErrorReason.syncedProfileRequired,
      );
    }
    await mockDelay(_cloud.config.latency);
    r.phase = VaultPhase.awaitingApproval;
    _cloud.notifyChanged();
    final after = _cloud.config.autoApproveAfter;
    if (after != null) {
      _autoApprove?.cancel();
      _autoApprove = Timer(after, () => unawaited(simulateApproval()));
    }
  }

  /// Developer control: a trusted device approved this device.
  Future<void> simulateApproval() async {
    final r = _cloud.activeRecord;
    if (r == null || r.phase != VaultPhase.awaitingApproval) return;
    r
      ..deviceTrusted = true
      ..phase = VaultPhase.unlocked;
    _cloud.notifyChanged();
  }

  @override
  Future<void> cancelDeviceApproval() async {
    _autoApprove?.cancel();
    final r = _active();
    if (r.phase == VaultPhase.awaitingApproval) r.phase = VaultPhase.locked;
    _cloud.notifyChanged();
  }

  @override
  Future<void> recoverWithRecoveryKey({required SecretText recoveryInput, required SecretText newPassphrase}) async {
    final r = _active();
    final keys = _keys(r);
    if (RecoveryInput.parse(recoveryInput.expose()) == null) {
      throw const AppException(
        AppErrorCode.invalidRecoveryKey,
        'Enter all 24 words of the Recovery Key or the QR text',
      );
    }
    _checkNewPassphrase(newPassphrase);
    await mockDelay(_cloud.config.kdfLatency);
    if (!keys.matchesRecovery(recoveryInput.expose())) {
      throw const AppException(AppErrorCode.invalidRecoveryKey, 'This Recovery Key does not open the vault');
    }
    keys.passphrase = newPassphrase.expose();
    r
      ..phase = VaultPhase.unlocked
      ..deviceTrusted = true;
    _cloud.notifyChanged();
  }

  @override
  Future<void> resetPassphraseWithTrustedDevice({required SecretText newPassphrase, required String reason}) async {
    final r = _active();
    final keys = _keys(r);
    if (!r.deviceTrusted) {
      throw const AppException(AppErrorCode.deviceNotTrusted, 'This device is not trusted for the vault');
    }
    if (r.phase != VaultPhase.unlocked) _checkDeviceUnlock();
    _checkNewPassphrase(newPassphrase);
    await mockDelay(_cloud.config.kdfLatency);
    keys.passphrase = newPassphrase.expose();
    r.phase = VaultPhase.unlocked;
    _cloud.notifyChanged();
  }

  @override
  Future<bool> verifyPassphrase(SecretText passphrase) async {
    final keys = _keys(_active());
    await mockDelay(_cloud.config.kdfLatency);
    try {
      return passphrase.expose() == keys.passphrase;
    } finally {
      passphrase.wipe();
    }
  }

  @override
  Future<void> changePassphrase({required SecretText current, required SecretText next}) async {
    final keys = _keys(_active());
    await mockDelay(_cloud.config.kdfLatency);
    if (current.expose() != keys.passphrase) {
      throw const AppException(
        AppErrorCode.wrongPassphrase,
        'Current passphrase is wrong',
        reason: AppErrorReason.currentPassphraseWrong,
      );
    }
    _checkNewPassphrase(next);
    keys.passphrase = next.expose();
  }

  Future<void> dispose() async {
    _autoApprove?.cancel();
    await _sub.cancel();
    await _status.close();
  }
}
