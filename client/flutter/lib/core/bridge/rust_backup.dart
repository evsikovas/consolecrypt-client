import 'dart:async';
import 'dart:io';

import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/bridge/rust_account.dart';
import 'package:consolecrypt/core/bridge/rust_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/core/util/value_stream.dart';
import 'package:consolecrypt/src/rust/api/backup.dart' as rs_backup;
import 'package:file_selector/file_selector.dart' as fs;
import 'package:flutter/services.dart';

/// Encrypted `.ccbackup` export / inspect / restore over app-core. The
/// schedule, the scheduler (runs in the core every 30 s while unlocked —
/// also with the window hidden) and the recent-backups list live in the
/// core (profile database); this service mirrors them and reloads on the
/// `backup_*` core events.
final class RustBackupService implements BackupService {
  RustBackupService(this._hub) {
    _profilesSub = _hub.profiles.stream.listen((_) => unawaited(_loadForActive()));
    _hub.eventListeners.add(_onEvent);
  }

  final RustBackend _hub;
  final _schedule = ValueStreamController<BackupSchedule>(const BackupSchedule());
  final _recent = ValueStreamController<List<BackupInfo>>(const []);
  late final StreamSubscription<ProfilesState> _profilesSub;
  String? _loadedFor;

  String? get _profileId => _hub.activeInfo?['id'] as String?;

  void _onEvent(Json e) {
    switch (e['type']) {
      case 'backup_schedule_changed' || 'backup_completed' || 'backup_failed':
        unawaited(_reload());
    }
  }

  Future<void> _loadForActive() async {
    final id = _profileId;
    if (id == _loadedFor) return;
    _loadedFor = id;
    if (id == null) {
      _schedule.value = const BackupSchedule();
      _recent.value = const [];
      return;
    }
    await _migrateLegacy(id);
    await _reload();
  }

  Future<void> _reload() async {
    if (_profileId == null) return;
    try {
      _schedule.value = backupScheduleFromJson(decodeObject(await guard(rs_backup.backupScheduleGet)));
      _recent.value = List.unmodifiable(decodeList(await guard(rs_backup.backupRecent)).map(backupInfoFromJson));
    } on AppException {
      // Profile closing: keep the last values.
    }
  }

  /// One-time move of the schedule / recent list older builds kept in the
  /// device-local UI store (`backup_schedule:<id>`, `recent_backups:<id>`).
  Future<void> _migrateLegacy(String id) async {
    try {
      final schedule = await _hub.storeGet('backup_schedule:$id');
      if (schedule != null) {
        final core = backupScheduleFromJson(decodeObject(await guard(rs_backup.backupScheduleGet)));
        if (!core.enabled && (core.folder ?? '').isEmpty) {
          await guard(() => rs_backup.backupScheduleSet(scheduleJson: schedule));
        }
        await _hub.storeSet('backup_schedule:$id', null);
      }
      final recent = await _hub.storeGet('recent_backups:$id');
      if (recent != null) {
        await guard(() => rs_backup.backupRemember(backupsJson: recent));
        await _hub.storeSet('recent_backups:$id', null);
      }
    } on Object {
      // Corrupt legacy state or core refusal: start fresh in the core.
    }
  }

  Future<BackupInfo> _info(String path, {bool automatic = false}) async {
    final h = await guard(() => rs_backup.backupInspect(path: path));
    return BackupInfo(
      path: h.path,
      vaultId: VaultId(h.vaultId),
      createdAt: fromMs(h.createdAtMs),
      objectCount: h.objects,
      sizeBytes: h.sizeBytes,
      formatVersion: backupFormatVersion(h.format),
      appVersion: h.appVersion,
      automatic: automatic,
    );
  }

  @override
  Future<BackupInfo> exportBackup({required String path}) async {
    final target = path.toLowerCase().endsWith('.$backupFileExtension') ? path : '$path.$backupFileExtension';
    // The core remembers the export in its recent list.
    await guard(() => rs_backup.backupExport(path: target));
    await _reload();
    return _info(target);
  }

  @override
  Future<BackupInfo> inspectBackup(String path) => _info(path);

  @override
  Future<Profile> restoreBackup({required String path, required BackupUnlock unlock, String? profileName}) async {
    final name = (profileName ?? '').trim().isNotEmpty
        ? profileName!.trim()
        : path.split(RegExp(r'[\\/]')).last.replaceAll(RegExp(r'\.ccbackup$', caseSensitive: false), '');
    final String json;
    switch (unlock) {
      case BackupUnlockWithPassphrase(:final passphrase):
        json = await withSecret(
          passphrase,
          (p) => guard(
            () => rs_backup.backupImport(
              path: path,
              displayName: name,
              unlockKind: rs_backup.BackupUnlockKind.passphrase,
              secret: p,
            ),
          ),
        );
      case BackupUnlockWithRecoveryKey(:final recoveryInput, :final newPassphrase):
        json = await withSecrets(
          recoveryInput,
          newPassphrase,
          (r, p) => guard(
            () => rs_backup.backupImport(
              path: path,
              displayName: name,
              unlockKind: rs_backup.BackupUnlockKind.recoveryKey,
              secret: r,
              newPassphrase: p,
            ),
          ),
        );
    }
    final created = decodeObject(json);
    _hub.draftProfile = null;
    await _hub.refreshState();
    await _hub.reloadAll();
    return profileFromJson((created['profile']! as Map).cast<String, Object?>());
  }

  @override
  Stream<BackupSchedule> watchSchedule() {
    unawaited(_loadForActive());
    return _schedule.stream;
  }

  @override
  Future<void> updateSchedule(BackupSchedule schedule) async {
    if (_profileId == null) {
      throw const AppException(AppErrorCode.notFound, 'No active profile', reason: AppErrorReason.noActiveProfile);
    }
    // The core validates (`backup_folder_required`, `keep_at_least_one`)
    // and computes the next run.
    final stored = await guard(
      () => rs_backup.backupScheduleSet(scheduleJson: encodeJson(backupScheduleToJson(schedule))),
    );
    _schedule.value = backupScheduleFromJson(decodeObject(stored));
  }

  @override
  Future<BackupInfo> backupNow() async {
    final info = backupInfoFromJson(decodeObject(await guard(rs_backup.backupNow)));
    await _reload();
    return info;
  }

  @override
  Stream<List<BackupInfo>> watchRecentBackups() {
    unawaited(_loadForActive());
    return _recent.stream;
  }

  Future<void> dispose() async {
    _hub.eventListeners.remove(_onEvent);
    await _profilesSub.cancel();
    await _schedule.close();
    await _recent.close();
  }
}

/// Native dialogs via the flutter.dev `file_selector` plugin.
final class NativeFileDialogService implements FileDialogService {
  const NativeFileDialogService();

  static List<fs.XTypeGroup> _groups(List<String> extensions) => extensions.isEmpty
      ? const []
      : [
          fs.XTypeGroup(
            label: extensions.join(', '),
            extensions: extensions,
            // iOS document pickers require UTIs; custom .ccbackup has no
            // registered UTI. The core validates the chosen file format.
            uniformTypeIdentifiers: Platform.isIOS ? const ['public.data'] : const [],
          ),
        ];

  @override
  Future<String?> chooseSaveFile({required String suggestedName, List<String> extensions = const []}) async {
    if (Platform.isAndroid || Platform.isIOS) {
      final root = await MethodChannel(Platform.isIOS ? 'consolecrypt/ios' : 'consolecrypt/android')
          .invokeMethod<String>('dataDirectory');
      if (root == null || root.isEmpty) throw StateError('Mobile private storage unavailable');
      final dir = await Directory('$root/exports').create(recursive: true);
      final name = suggestedName.replaceAll(RegExp(r'[\\/\x00-\x1f]'), '_');
      final staging = await Directory(dir.path).createTemp('export-');
      return '${staging.path}/$name';
    }
    return (await fs.getSaveLocation(suggestedName: suggestedName, acceptedTypeGroups: _groups(extensions)))?.path;
  }

  @override
  Future<bool> finishSaveFile(String path) async =>
      (!Platform.isAndroid && !Platform.isIOS) ||
      (await MethodChannel(Platform.isIOS ? 'consolecrypt/ios' : 'consolecrypt/android')
              .invokeMethod<bool>('exportFile', {'path': path}) ??
          false);

  @override
  Future<String?> chooseOpenFile({List<String> extensions = const []}) async =>
      (await fs.openFile(acceptedTypeGroups: _groups(extensions)))?.path;

  @override
  Future<String?> chooseDirectory() => fs.getDirectoryPath();

  @override
  Future<String?> chooseApplication({required String label, required String confirmButtonText}) async {
    try {
      return (await fs.openFile(
        initialDirectory: Platform.isMacOS ? '/Applications' : null,
        confirmButtonText: confirmButtonText,
        acceptedTypeGroups: [
          if (Platform.isMacOS)
            fs.XTypeGroup(label: label, uniformTypeIdentifiers: const ['com.apple.application-bundle'])
          else if (Platform.isWindows)
            fs.XTypeGroup(label: label, extensions: const ['exe']),
        ],
      ))?.path;
    } on PlatformException catch (e) {
      throw AppException(
        AppErrorCode.internal,
        'Application picker failed', // l10n-ignore: diagnostic
        reason: AppErrorReason.applicationPickerFailed,
        args: {'detail': String.fromCharCodes((e.message ?? e.code).runes.take(512))},
      );
    }
  }
}
