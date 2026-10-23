// Website guide fixtures for 0.9.2 key trust and sync limits. They wrap the
// mock devices service and render production widgets; nothing confirms,
// approves or pins a real key, and all codes are synthetic examples.
import 'dart:convert';

import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/devices_service.dart';
import 'package:consolecrypt/core/services/enrollment_service.dart';
import 'package:consolecrypt/sharing/enrollment_models.dart';
import 'package:consolecrypt/sharing/sharing_models.dart';

/// 0.9.2 key-trust and sync-limit cases handled by `_keyTrustScreen` in
/// design_preview.dart.
const guideKeyTrustScreens = [
  'vault-key-mismatch',
  'devices-key-unconfirmed',
  'approval-pairing',
  'sharing-verify',
  'sharing-verify-banner',
  'sharing-policy',
  'enrollment-endorse',
  'sync-limits',
];

/// A synthetic six-group code from a seed (never derived from a key).
VerificationCode guideCode(int seed) =>
    VerificationCode([for (var i = 0; i < 6; i++) ((seed * 7919 + i * 104729) % 100000).toString().padLeft(5, '0')]);

/// The mock devices of the account plus a vault key that is "not
/// confirmed" and a pending approval announced by "Office Mac".
final class GuideKeyTrustDevices implements DevicesService {
  GuideKeyTrustDevices(this._inner, {this.unconfirmed = false, this.pairing = false});
  final DevicesService _inner;
  final bool unconfirmed, pairing;

  @override
  Future<VaultKeyStatus> vaultKeyStatus() async =>
      VaultKeyStatus(pinned: true, unconfirmed: unconfirmed, fingerprint: '7f3a91c2d4e85b06');

  @override
  Future<bool> confirmVaultKey(SecretText passphrase) async {
    passphrase.wipe();
    return false;
  }

  @override
  Future<ApprovalProgress?> approvalProgress() async => pairing
      ? ApprovalProgress(
          requestId: const DeviceRequestId('guide-request'),
          ownCode: guideCode(3),
          vrkCommitment: true,
          approverDeviceId: const DeviceId('guide-office-mac'),
          approverName: 'Office Mac',
          pairingCode: guideCode(11),
        )
      : null;

  @override
  Future<void> confirmApprover(ApprovalProgress progress, VerificationCode shown) async {}

  @override
  Stream<DevicesSnapshot> watchDevices() => _inner.watchDevices();
  @override
  Future<void> refresh() => _inner.refresh();
  @override
  Future<VerificationCode> verificationCodeFor(DeviceRequestId requestId) => _inner.verificationCodeFor(requestId);
  @override
  Future<VerificationCode> currentDeviceVerificationCode() async => guideCode(3);
  @override
  Future<void> approve(DeviceRequestId requestId, {required VerificationCode confirmedCode}) =>
      throw StateError('Guide capture never approves a device.');
  @override
  Future<void> reject(DeviceRequestId requestId) => _inner.reject(requestId);
  @override
  Future<void> revoke(DeviceId deviceId, {String? reason}) => throw StateError('Guide capture never revokes.');
  @override
  Future<void> rename(DeviceId deviceId, String name) => _inner.rename(deviceId, name);
  @override
  Stream<List<NewDeviceNotice>> watchNewDevices() => _inner.watchNewDevices();
  @override
  void dismissNewDevice(DeviceId deviceId) => _inner.dismissNewDevice(deviceId);
}

/// A public enrollment packet of another own device (synthetic JSON).
const guideEnrollmentPacket = '{"kind":"consolecrypt-enrollment","demo":true}';

/// Reviews [guideEnrollmentPacket]; endorsing is never run.
final class GuideEndorseEnrollment extends UnavailableEnrollmentService {
  @override
  Future<List<EnrollmentGrant>> grants(String shareId) async => const [];
  @override
  Future<List<EnrollmentRequest>> requests(String shareId) async => const [];
  @override
  Future<List<EnrollmentRequest>> pendingRequests() async => const [];
  @override
  Future<List<EnrollmentRequest>> processAutomatic() async => const [];
  @override
  Future<EnrollmentPairing> inspectPairing(String bundle) async => EnrollmentPairing(
    shareId: 'share-7c41e0',
    grantId: 'grant-2d9b55',
    bundle: jsonEncode({'kind': 'consolecrypt-enrollment', 'demo': true}),
    requestId: 'request-5a17c3',
    code: guideCode(23).groups.join(' '),
    target: const SharingIdentity(
      instance: 'guide-instance',
      userId: 'guide-owner',
      deviceId: 'Home laptop',
      encryptionKey: 'synthetic-public-x-laptop',
      signingKey: 'synthetic-public-ed-laptop',
      code: 'CC1:00FF00FF00FF00FF00FF00FF00FF00FF00FF00FF00FF00FF00FF00FF00FF00FF',
    ),
    requestedRole: SharingRole.reader,
    expires: DateTime.now().add(const Duration(minutes: 9)),
  );
}

/// Recipients of existing secrets that were never verified (TR-18 banner).
List<PeerPin> guideBannerPins() => const [
  PeerPin(
    verificationId: 'guide-pin-1',
    deviceId: 'Colleague laptop',
    userId: 'guide-colleague',
    status: PeerTrust.chosen,
    email: 'colleague@example.test',
  ),
  PeerPin(
    verificationId: 'guide-pin-2',
    deviceId: 'On-call phone',
    userId: 'guide-oncall',
    status: PeerTrust.chosen,
    email: 'oncall@example.test',
  ),
];

/// A pause by the server (resuming in 25 minutes) and four changes it
/// refused because of the vault object limit.
SyncStatus guideLimitedSync() => SyncStatus(
  state: SyncState.throttled,
  nextRetryAt: DateTime.now().add(const Duration(minutes: 25)),
  lastSyncAt: DateTime.now().subtract(const Duration(minutes: 7)),
  pendingChanges: 3,
  failedChanges: 4,
  pushQuota: const SyncQuota(reason: SyncQuota.objectLimitReason, limit: 5000),
);
