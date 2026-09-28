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

  bool get isRevoked => status == DeviceStatus.revoked;

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
  const DevicesSnapshot({required this.devices, required this.pendingRequests});

  static const empty = DevicesSnapshot(devices: [], pendingRequests: []);

  final List<DeviceInfo> devices;
  final List<DeviceTrustRequest> pendingRequests;
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
