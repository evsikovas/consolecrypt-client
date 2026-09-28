import 'dart:async';

import 'package:consolecrypt/core/mock/mock_cloud.dart';
import 'package:consolecrypt/core/mock/mock_config.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/passphrase_strength.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/core/util/value_stream.dart';

/// Simulated `.ccbackup` files (kept in memory, keyed by path).
final class MockBackupService implements BackupService {
  MockBackupService(this._cloud) {
    _sub = _cloud.changed.listen((_) => _recompute());
    _recompute();
  }

  final MockCloud _cloud;

  final ValueStreamController<BackupSchedule> _schedule = ValueStreamController(const BackupSchedule());
  final ValueStreamController<List<BackupInfo>> _recent = ValueStreamController(const []);
  late final StreamSubscription<void> _sub;
  Object? _signature;
  int _version = 0;

  static const _appVersion = '0.1.0';

  void _recompute() {
    final r = _cloud.activeRecord;
    final signature = (r?.profile.id, _version);
    if (signature == _signature) return;
    _signature = signature;
    _schedule.value = r?.schedule ?? const BackupSchedule();
    _recent.value = List.unmodifiable(r?.backups ?? const <BackupInfo>[]);
  }

  void _bump() {
    _version++;
    _recompute();
  }

  MockProfileRecord _unlocked() {
    final r = _cloud.activeRecord;
    if (r == null || r.vault == null || r.phase != VaultPhase.unlocked) {
      throw const AppException(
        AppErrorCode.notFound,
        'Unlock the vault to back it up',
        reason: AppErrorReason.vaultLocked,
      );
    }
    return r;
  }

  BackupInfo _write(MockProfileRecord r, String path, {required bool automatic}) {
    final keys = r.vault!;
    final objects = _cloud.dataFor(keys.vaultId, keys.name).objectCount;
    final info = BackupInfo(
      path: path,
      vaultId: keys.vaultId,
      createdAt: DateTime.now().toUtc(),
      objectCount: objects,
      sizeBytes: 4096 + objects * 1400,
      formatVersion: 1,
      appVersion: _appVersion,
      automatic: automatic,
    );
    _cloud.backupFiles[path] = MockBackupFile(info: info, keys: keys.copy());
    r.backups.insert(0, info);
    return info;
  }

  @override
  Future<BackupInfo> exportBackup({required String path}) async {
    final r = _unlocked();
    if (!path.endsWith('.$backupFileExtension')) {
      throw const AppException(
        AppErrorCode.validation,
        'Backup files must end with .$backupFileExtension',
        reason: AppErrorReason.backupExtension,
        args: {'extension': backupFileExtension},
      );
    }
    await mockDelay(_cloud.config.kdfLatency);
    final info = _write(r, path, automatic: false);
    _bump();
    return info;
  }

  @override
  Future<BackupInfo> inspectBackup(String path) async {
    await mockDelay(_cloud.config.latency);
    final file = _cloud.backupFiles[path.trim()];
    if (file == null) {
      throw const AppException(
        AppErrorCode.notFound,
        'Not a ConsoleCrypt backup (or the file is missing)',
        reason: AppErrorReason.notABackup,
      );
    }
    return file.info;
  }

  @override
  Future<Profile> restoreBackup({required String path, required BackupUnlock unlock, String? profileName}) async {
    final file = _cloud.backupFiles[path.trim()];
    if (file == null) {
      throw const AppException(
        AppErrorCode.notFound,
        'Not a ConsoleCrypt backup (or the file is missing)',
        reason: AppErrorReason.notABackup,
      );
    }
    final keys = file.keys.copy();
    switch (unlock) {
      case BackupUnlockWithPassphrase(:final passphrase):
        await mockDelay(_cloud.config.kdfLatency);
        if (passphrase.expose() != keys.passphrase) {
          throw const AppException(AppErrorCode.wrongPassphrase, 'Wrong passphrase for this backup');
        }
      case BackupUnlockWithRecoveryKey(:final recoveryInput, :final newPassphrase):
        if (!PassphraseStrength.estimate(newPassphrase.expose()).acceptable) {
          throw const AppException(
            AppErrorCode.validation,
            'Choose a stronger new passphrase',
            reason: AppErrorReason.weakPassphrase,
          );
        }
        await mockDelay(_cloud.config.kdfLatency);
        if (!keys.matchesRecovery(recoveryInput.expose())) {
          throw const AppException(AppErrorCode.invalidRecoveryKey, 'This Recovery Key does not open the backup');
        }
        keys.passphrase = newPassphrase.expose();
    }
    final profile = Profile(
      id: ProfileId.generate(),
      name: (profileName ?? '').trim().isEmpty ? '${keys.name} (restored)' : profileName!.trim(),
      kind: ProfileKind.local,
      vaultId: keys.vaultId,
      createdAt: DateTime.now().toUtc(),
    );
    _cloud.lockActive();
    _cloud.records[profile.id] = MockProfileRecord(profile)
      ..vault = keys
      ..phase = VaultPhase.unlocked
      ..deviceTrusted =
          true // new device envelope for this installation
      ..recoveryKitConfirmed = true;
    _cloud.publishProfiles(activeId: profile.id);
    return profile;
  }

  @override
  Stream<BackupSchedule> watchSchedule() => _schedule.stream;

  @override
  Future<void> updateSchedule(BackupSchedule schedule) async {
    final r = _cloud.activeRecord;
    if (r == null) {
      throw const AppException(AppErrorCode.notFound, 'No active profile', reason: AppErrorReason.noActiveProfile);
    }
    if (schedule.enabled && (schedule.folder ?? '').trim().isEmpty) {
      throw const AppException(
        AppErrorCode.validation,
        'Choose a folder for automatic backups',
        reason: AppErrorReason.backupFolderRequired,
      );
    }
    if (schedule.keepLast < 1) {
      throw const AppException(
        AppErrorCode.validation,
        'Keep at least one backup',
        reason: AppErrorReason.keepAtLeastOne,
      );
    }
    r.schedule = schedule.copyWith(
      nextRunAt: schedule.enabled ? DateTime.now().add(schedule.frequency.interval) : null,
    );
    _bump();
  }

  @override
  Future<BackupInfo> backupNow() async {
    final r = _unlocked();
    final folder = r.schedule.folder;
    if (folder == null || folder.trim().isEmpty) {
      throw const AppException(
        AppErrorCode.validation,
        'Choose a backup folder first',
        reason: AppErrorReason.backupFolderRequired,
      );
    }
    await mockDelay(_cloud.config.kdfLatency);
    final stamp = formatIsoDateTime(DateTime.now()).replaceAll(RegExp('[: ]'), '-');
    final info = _write(r, '$folder/${r.profile.name}-$stamp.$backupFileExtension', automatic: true);
    // Retention: keep the newest `keepLast` automatic backups.
    final automatic = r.backups.where((b) => b.automatic).toList();
    for (final old in automatic.skip(r.schedule.keepLast)) {
      r.backups.remove(old);
      _cloud.backupFiles.remove(old.path);
    }
    r.schedule = r.schedule.copyWith(
      lastRunAt: DateTime.now(),
      nextRunAt: r.schedule.enabled ? DateTime.now().add(r.schedule.frequency.interval) : null,
      clearError: true,
    );
    _bump();
    return info;
  }

  @override
  Stream<List<BackupInfo>> watchRecentBackups() => _recent.stream;

  Future<void> dispose() async {
    await _sub.cancel();
    await _schedule.close();
    await _recent.close();
  }
}

/// Demo paths instead of native dialogs.
final class MockFileDialogService implements FileDialogService {
  MockFileDialogService(this._cloud);

  final MockCloud _cloud;

  /// Simulated native application choice; null means Cancel.
  String? applicationPath = '/Applications/Visual Studio Code.app';
  int applicationChoices = 0;

  @override
  Future<String?> chooseSaveFile({required String suggestedName, List<String> extensions = const []}) async =>
      '/Users/demo/Documents/$suggestedName';

  @override
  Future<bool> finishSaveFile(String path) async => true;

  @override
  Future<String?> chooseOpenFile({List<String> extensions = const []}) async {
    final files = _cloud.backupFiles.keys.toList();
    return files.isEmpty ? null : files.last;
  }

  @override
  Future<String?> chooseDirectory() async => '/Users/demo/Backups';

  @override
  Future<String?> chooseApplication({required String label, required String confirmButtonText}) async {
    applicationChoices++;
    return applicationPath;
  }
}
