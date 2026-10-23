import 'package:consolecrypt/core/models/ids.dart';

/// Mirrors `cc_protocol::version::Platform`.
enum DevicePlatform {
  macos('macos', 'macOS'),
  windows('windows', 'Windows'),
  linux('linux', 'Linux'),
  ios('ios', 'iOS'),
  android('android', 'Android'),
  cli('cli', 'CLI');

  const DevicePlatform(this.wireName, this.label);

  final String wireName;
  final String label;
}

/// Mirrors `cc_protocol::devices::DeviceStatus` (account level).
enum DeviceStatus { active, revoked }

/// Mirrors `cc_protocol::devices::DeviceInfo` (public keys omitted: the UI
/// only needs the verification code derived from them, computed in core).
final class DeviceInfo {
  const DeviceInfo({
    required this.deviceId,
    required this.name,
    required this.platform,
    required this.status,
    required this.trustedVaults,
    required this.createdAt,
    this.lastSeenAt,
    this.revokedAt,
    this.isCurrent = false,
    this.trusted,
    this.quarantineUntil,
  });

  final DeviceId deviceId;
  final String name;
  final DevicePlatform platform;
  final DeviceStatus status;

  /// Vaults this device holds a device envelope for (= trusted for).
  final List<VaultId> trustedVaults;
  final DateTime createdAt;
  final DateTime? lastSeenAt;
  final DateTime? revokedAt;
  final bool isCurrent;

  /// Holds a device envelope for any vault of the account (account-level
  /// trust, protocol 1.7). `null` when unknown.
  final bool? trusted;

  /// End of the new-device quarantine while it runs (protocol 1.7): until
  /// then the device cannot revoke or sign out trusted devices.
  final DateTime? quarantineUntil;

  bool get isRevoked => status == DeviceStatus.revoked;

  bool get inQuarantine => !isRevoked && quarantineUntil != null && quarantineUntil!.isAfter(DateTime.now());

  bool isTrustedFor(VaultId? vault) => !isRevoked && vault != null && trustedVaults.contains(vault);
}

/// Mirrors `cc_protocol::devices::DeviceRequestStatus`.
enum DeviceRequestStatus { pending, approved, rejected, expired }

/// Mirrors `cc_protocol::devices::DeviceTrustRequest`.
final class DeviceTrustRequest {
  const DeviceTrustRequest({
    required this.requestId,
    required this.device,
    required this.vaultIds,
    required this.status,
    required this.createdAt,
    required this.expiresAt,
    this.approvedByDeviceId,
  });

  final DeviceRequestId requestId;
  final DeviceInfo device;
  final List<VaultId> vaultIds;
  final DeviceRequestStatus status;
  final DateTime createdAt;
  final DateTime expiresAt;
  final DeviceId? approvedByDeviceId;

  bool isExpired({DateTime? now}) => (now ?? DateTime.now()).isAfter(expiresAt);
}

/// `GET /v1/devices` snapshot.
final class DevicesSnapshot {
  const DevicesSnapshot({
    required this.devices,
    required this.pendingRequests,
    this.deviceProtection = false,
    this.vaultKeyUnconfirmed = false,
    this.vaultKeyConflicts = const [],
  });

  static const empty = DevicesSnapshot(devices: [], pendingRequests: []);

  final List<DeviceInfo> devices;
  final List<DeviceTrustRequest> pendingRequests;

  /// The server advertises new-device protection (protocol 1.7). Without it
  /// the UI behaves as before: no "unconfirmed"/quarantine marks.
  final bool deviceProtection;

  /// This device pinned the vault key without an independent check
  /// (audit H2): confirm it with the vault passphrase.
  final bool vaultKeyUnconfirmed;

  /// Devices that reported a different vault key (signed records).
  final List<DeviceId> vaultKeyConflicts;

  DeviceInfo? get current => devices.where((d) => d.isCurrent).firstOrNull;
}

/// The new device's view of its approval (protocol 1.11): once a trusted
/// device announced itself, the pairing code over both devices' keys.
final class ApprovalProgress {
  const ApprovalProgress({
    required this.requestId,
    required this.ownCode,
    this.vrkCommitment = false,
    this.approverDeviceId,
    this.approverName,
    this.pairingCode,
    this.approverConfirmed = false,
  });
  final DeviceRequestId requestId;
  final VerificationCode ownCode;
  final bool vrkCommitment;
  final DeviceId? approverDeviceId;
  final String? approverName;
  final VerificationCode? pairingCode;
  final bool approverConfirmed;
}

/// Vault key trust of the open profile (audit H2).
final class VaultKeyStatus {
  const VaultKeyStatus({this.pinned = false, this.unconfirmed = false, this.fingerprint});
  final bool pinned, unconfirmed;
  final String? fingerprint;
}

/// "A new device signed in to the account" (protocol 1.7 `device_registered`),
/// shown on trusted devices until answered with "Revoke" or "It's me".
final class NewDeviceNotice {
  const NewDeviceNotice({required this.deviceId, required this.name, required this.platform, required this.createdAt});

  final DeviceId deviceId;
  final String name;
  final DevicePlatform platform;
  final DateTime createdAt;
}

/// Result of "sign out on all devices".
final class LogoutAllResult {
  const LogoutAllResult({this.trustedDevicesKept = 0, this.quarantineUntil});

  /// Trusted devices that stayed signed in because this device is new.
  final int trustedDevicesKept;
  final DateTime? quarantineUntil;
}

/// Device verification code (ADR-0004): `h = SHA-256(device_fingerprint_input)`
/// rendered as 6 groups of 5 decimal digits, group `i` = big-endian u40 of
/// `h[5i..5i+5]` mod 100000. crypto-core computes the digest; the UI only
/// formats and compares.
final class VerificationCode {
  VerificationCode(List<String> groups) : groups = List.unmodifiable(groups) {
    if (groups.length != groupCount || groups.any((g) => g.length != 5 || int.tryParse(g) == null)) {
      throw ArgumentError('verification code must be 6 groups of 5 digits');
    }
  }

  /// Formats a 32-byte SHA-256 digest per ADR-0004.
  factory VerificationCode.fromDigest(List<int> digest) {
    if (digest.length < 30) {
      throw ArgumentError('digest too short: ${digest.length} bytes');
    }
    final groups = <String>[];
    for (var i = 0; i < groupCount; i++) {
      var v = 0;
      for (var j = 0; j < 5; j++) {
        v = v * 256 + (digest[5 * i + j] & 0xff);
      }
      groups.add((v % 100000).toString().padLeft(5, '0'));
    }
    return VerificationCode(groups);
  }

  /// Parses user input such as `12345 67890 …` (any separators).
  static VerificationCode? tryParse(String input) {
    final digits = input.replaceAll(RegExp(r'\D'), '');
    if (digits.length != groupCount * 5) return null;
    return VerificationCode([for (var i = 0; i < groupCount; i++) digits.substring(i * 5, i * 5 + 5)]);
  }

  static const groupCount = 6;

  final List<String> groups;

  String get display => groups.join(' ');

  bool matches(VerificationCode other) => display == other.display;

  @override
  bool operator ==(Object other) => other is VerificationCode && matches(other);

  @override
  int get hashCode => display.hashCode;

  @override
  String toString() => 'VerificationCode($display)';
}
