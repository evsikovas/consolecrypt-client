import 'package:consolecrypt/core/models/models.dart';

/// Encrypted `.ccbackup` export/import and scheduled auto-backup
/// (storage-core, ADR-0106). Backups contain envelopes + object
/// ciphertexts only; they are restorable with the passphrase or Recovery Key.
abstract interface class BackupService {
  /// Writes a backup of the active (unlocked) profile's vault to [path].
  Future<BackupInfo> exportBackup({required String path});

  /// Reads the header of a backup file without decrypting anything.
  Future<BackupInfo> inspectBackup(String path);

  /// Restores into a **new local profile** (never overwrites an existing
  /// one), unlocks it and creates a device envelope for this installation.
  Future<Profile> restoreBackup({required String path, required BackupUnlock unlock, String? profileName});

  /// Auto-backup settings of the active profile.
  Stream<BackupSchedule> watchSchedule();

  Future<void> updateSchedule(BackupSchedule schedule);

  /// Runs the scheduled backup immediately (to the configured folder).
  Future<BackupInfo> backupNow();

  /// Backups of the active profile known to this device, newest first.
  Stream<List<BackupInfo>> watchRecentBackups();
}
