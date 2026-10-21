import 'package:consolecrypt/core/models/models.dart';

/// Devices and device trust (ADR-0004).
abstract interface class DevicesService {
  /// Devices of the account + pending trust requests (refreshed on
  /// `device_*` WebSocket events).
  Stream<DevicesSnapshot> watchDevices();

  Future<void> refresh();

  /// Code computed *locally* from the public keys the server reports for
  /// the requesting device. Shown on the approving device.
  Future<VerificationCode> verificationCodeFor(DeviceRequestId requestId);

  /// This device's own code (shown on the new device while it waits).
  Future<VerificationCode> currentDeviceVerificationCode();

  /// Encrypts VRK to the new device and signs the approval. The core
  /// recomputes the code and refuses unless it equals [confirmedCode] — the
  /// code the user confirmed matches the one on the new device.
  Future<void> approve(DeviceRequestId requestId, {required VerificationCode confirmedCode});

  Future<void> reject(DeviceRequestId requestId);

  /// Revokes sessions, device envelopes and pending requests of the device.
  Future<void> revoke(DeviceId deviceId, {String? reason});

  Future<void> rename(DeviceId deviceId, String name);

  /// New devices that signed in while this app was running (protocol 1.7),
  /// newest last; empty on servers without device protection.
  Stream<List<NewDeviceNotice>> watchNewDevices();

  /// "It's me" (or after revoking): forget the notice.
  void dismissNewDevice(DeviceId deviceId);
}
