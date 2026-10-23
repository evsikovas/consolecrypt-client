import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/sharing/sharing_models.dart';

/// The safety number every fake device computes (12 groups of 5 digits).
const fakeSafetyGroups = [
  '11111', '22222', '33333', '44444', '55555', '66666', //
  '77777', '88888', '99999', '12345', '67890', '13579',
];

/// Peer pins for sharing fakes (audit H3): choosing pins `chosen`, the
/// fake safety number or the QR text `qr:<device>` verifies.
mixin FakePeerPins {
  final Map<String, PeerPin> pins = {};
  final List<String> verifyAttempts = [];
  SharingPolicy currentPolicy = const SharingPolicy();
  List<PeerPin> banner = const [];

  PeerPin _pin(SharingIdentity identity, PeerTrust status) => PeerPin(
    verificationId: 'pin-${identity.deviceId}',
    deviceId: identity.deviceId,
    userId: identity.userId,
    status: status,
    verifiedAt: status == PeerTrust.verified ? DateTime(2026, 10, 23) : null,
  );

  Future<PeerPin> choosePeer(SharingIdentity identity, {String? email}) async =>
      pins.putIfAbsent(identity.deviceId, () {
        final status = identity.trust == PeerTrust.none ? PeerTrust.chosen : identity.trust;
        return _pin(identity, status);
      });

  Future<SafetyNumber> safetyNumber(SharingIdentity identity) async =>
      const SafetyNumber(groups: fakeSafetyGroups, ownQrPayload: 'consolecrypt-verify:v1:own');

  Future<PeerPin> verifyPeer(SharingIdentity identity, String method, String input, {String? email}) async {
    verifyAttempts.add(method);
    final ok = method == 'safety_number'
        ? input.replaceAll(RegExp(r'\D'), '') == fakeSafetyGroups.join()
        : input.trim() == 'qr:${identity.deviceId}';
    if (!ok) {
      throw const AppException(AppErrorCode.peerVerificationFailed, 'mismatch');
    }
    return pins[identity.deviceId] = _pin(identity, PeerTrust.verified);
  }

  Future<List<PeerPin>> verificationBanner() async => banner;
  Future<void> dismissVerificationBanner() async => banner = const [];
  Future<SharingPolicy> policy() async => currentPolicy;
  Future<SharingPolicy> setPolicy(SharingPolicy policy) async => currentPolicy = policy;
}
