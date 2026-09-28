import 'package:consolecrypt/core/models/ids.dart';
import 'package:consolecrypt/core/security/secret_text.dart';

/// Encrypted backup file extension (ADR-0106).
const String backupFileExtension = 'ccbackup';

/// Metadata of a `.ccbackup` file: format version, vault id, envelopes and
/// object ciphertexts inside — never plaintext, never device keys.
final class BackupInfo {
  const BackupInfo({
    required this.path,
    required this.vaultId,
    required this.createdAt,
    required this.objectCount,
    required this.sizeBytes,
    required this.formatVersion,
    required this.appVersion,
    this.automatic = false,
  });

  final String path;
  final VaultId vaultId;
  final DateTime createdAt;
  final int objectCount;
  final int sizeBytes;
  final int formatVersion;
  final String appVersion;

  /// Written by the scheduled auto-backup.
  final bool automatic;

  String get fileName => path.split(RegExp(r'[\\/]')).last;
}

enum BackupFrequency {
  daily(Duration(days: 1)),
  weekly(Duration(days: 7));

  const BackupFrequency(this.interval);

  final Duration interval;
}

/// Scheduled auto-backup to a folder, with retention (per profile).
final class BackupSchedule {
  const BackupSchedule({
    this.enabled = false,
    this.folder,
    this.frequency = BackupFrequency.daily,
    this.keepLast = 7,
    this.lastRunAt,
    this.nextRunAt,
    this.lastError,
  });

  final bool enabled;
  final String? folder;
  final BackupFrequency frequency;

  /// Retention: older automatic backups beyond this count are deleted.
  final int keepLast;
  final DateTime? lastRunAt;
  final DateTime? nextRunAt;
  final String? lastError;

  BackupSchedule copyWith({
    bool? enabled,
    String? folder,
    BackupFrequency? frequency,
    int? keepLast,
    DateTime? lastRunAt,
    DateTime? nextRunAt,
    String? lastError,
    bool clearError = false,
  }) => BackupSchedule(
    enabled: enabled ?? this.enabled,
    folder: folder ?? this.folder,
    frequency: frequency ?? this.frequency,
    keepLast: keepLast ?? this.keepLast,
    lastRunAt: lastRunAt ?? this.lastRunAt,
    nextRunAt: nextRunAt ?? this.nextRunAt,
    lastError: clearError ? null : (lastError ?? this.lastError),
  );
}

/// How to open a backup being restored.
sealed class BackupUnlock {
  const BackupUnlock();
}

final class BackupUnlockWithPassphrase extends BackupUnlock {
  const BackupUnlockWithPassphrase(this.passphrase);

  final SecretText passphrase;

  @override
  String toString() => 'BackupUnlockWithPassphrase(<redacted>)';
}

/// Recovery Key (24 words or QR text); a new passphrase must be set.
final class BackupUnlockWithRecoveryKey extends BackupUnlock {
  const BackupUnlockWithRecoveryKey({required this.recoveryInput, required this.newPassphrase});

  final SecretText recoveryInput;
  final SecretText newPassphrase;

  @override
  String toString() => 'BackupUnlockWithRecoveryKey(<redacted>)';
}
