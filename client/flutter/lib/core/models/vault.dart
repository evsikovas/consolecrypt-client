import 'dart:convert';

import 'package:consolecrypt/core/models/ids.dart';

enum DeviceAuthKind { touchId, faceId, windowsHello, deviceCredential }

/// Capability and explicit opt-in for this profile on this installation.
final class DeviceUnlockInfo {
  const DeviceUnlockInfo({this.enabled = false, this.kind, this.hasDeviceEnvelope = false, this.notEnrolled = false});

  final bool enabled;
  final DeviceAuthKind? kind;
  final bool hasDeviceEnvelope;
  final bool notEnrolled;
  bool get canEnable => kind != null && hasDeviceEnvelope;
  bool get available => enabled && canEnable;
}

/// Where this device stands with respect to the account's vault.
enum VaultPhase {
  /// The account has no vault yet → onboarding (create passphrase).
  none,

  /// A vault exists; this device must unlock it.
  locked,

  /// This (untrusted) device asked a trusted device for approval (ADR-0004).
  awaitingApproval,

  /// Vault Root Key is in core memory.
  unlocked,
}

/// Vault state stream item from `VaultService`.
final class VaultStatus {
  const VaultStatus({
    required this.phase,
    this.vaultId,
    this.vaultName,
    this.recoveryKitConfirmed = true,
    this.deviceTrusted = false,
    this.deviceUnlock = const DeviceUnlockInfo(),
  });

  static const none = VaultStatus(phase: VaultPhase.none);

  final VaultPhase phase;
  final VaultId? vaultId;
  final String? vaultName;

  /// False between vault creation and the mandatory 3-word check.
  final bool recoveryKitConfirmed;

  /// This device holds a device envelope for the vault (T(V), ADR-0004).
  final bool deviceTrusted;

  /// OS authentication (Touch ID / Windows Hello) can unlock via the device
  /// envelope on this device.
  bool get deviceUnlockAvailable => deviceUnlock.available;
  final DeviceUnlockInfo deviceUnlock;

  bool get isUnlocked => phase == VaultPhase.unlocked;
}

/// Printable Recovery Kit (ADR-0002): vault ID, 24-word mnemonic of the
/// 256-bit Recovery Key, QR payload, server URL (none for local profiles,
/// ADR-0106), creation date.
///
/// Shown exactly once during onboarding / regeneration. `toString()` is
/// redacted and the words must be explicitly exposed.
final class RecoveryKit {
  RecoveryKit({
    required this.vaultId,
    required List<String> words,
    required String qrPayload,
    required this.createdAt,
    this.serverUrl,
  }) : _words = List.unmodifiable(words),
       // ignore: prefer_initializing_formals — named params cannot be private.
       _qrPayload = qrPayload {
    if (words.length != wordCount) {
      throw ArgumentError('recovery mnemonic must have $wordCount words');
    }
  }

  static const wordCount = 24;

  final VaultId vaultId;
  final Uri? serverUrl;
  final DateTime createdAt;
  List<String> _words;
  String _qrPayload;

  List<String> exposeWords() => _words;

  String exposeQrPayload() => _qrPayload;

  /// Drops references to the secret material (best effort in Dart).
  void forget() {
    _words = const [];
    _qrPayload = '';
  }

  @override
  String toString() => 'RecoveryKit(vaultId: ${vaultId.value}, words: <redacted>)';
}

/// Structural parse of what the user typed/scanned on the recovery screen.
/// The definitive check (BIP-39 word list + checksum, envelope decryption)
/// happens in vault-core.
sealed class RecoveryInput {
  const RecoveryInput();

  static const qrPrefix = 'consolecrypt-recovery:v1:';

  /// Returns `null` for input that is structurally invalid.
  static RecoveryInput? parse(String raw) {
    final text = raw.trim();
    if (text.startsWith(qrPrefix)) {
      final rest = text.substring(qrPrefix.length);
      final sep = rest.lastIndexOf(':');
      if (sep <= 0) return null;
      final vaultId = rest.substring(0, sep);
      final key = rest.substring(sep + 1);
      if (!RegExp(r'^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$').hasMatch(vaultId)) {
        return null;
      }
      if (!RegExp(r'^[A-Za-z0-9_-]{43}$').hasMatch(key)) return null;
      try {
        if (base64Url.decode(base64Url.normalize(key)).length != 32) return null;
      } on FormatException {
        return null;
      }
      return RecoveryQrInput(VaultId(vaultId.toLowerCase()));
    }
    final words = text
        .toLowerCase()
        .split(RegExp(r'[\s,;]+'))
        .map((w) => w.replaceFirst(RegExp(r'^\d+[.)]'), ''))
        .where((w) => w.isNotEmpty)
        .toList();
    if (words.length != RecoveryKit.wordCount) return null;
    if (words.any((w) => !RegExp(r'^[a-z]{3,8}$').hasMatch(w))) return null;
    return RecoveryMnemonicInput(words.length);
  }

  /// Number of words recognised so far (for live feedback).
  static int countWords(String raw) {
    final text = raw.trim();
    if (text.isEmpty || text.startsWith(qrPrefix)) return 0;
    return text.split(RegExp(r'[\s,;]+')).where((w) => w.isNotEmpty).length;
  }
}

final class RecoveryQrInput extends RecoveryInput {
  const RecoveryQrInput(this.vaultId);

  final VaultId vaultId;
}

final class RecoveryMnemonicInput extends RecoveryInput {
  const RecoveryMnemonicInput(this.wordCount);

  final int wordCount;
}
