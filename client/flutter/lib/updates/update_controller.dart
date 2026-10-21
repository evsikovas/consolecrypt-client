import 'dart:async';
import 'dart:io';

import 'package:consolecrypt/core/bridge/rust_app_services.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/settings_service.dart';
import 'package:consolecrypt/updates/update_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:url_launcher/url_launcher.dart';

enum UpdatePhase { idle, checking, current, available, downloading, ready, installing, permission, opened, failed }

class UpdateState {
  const UpdateState({
    this.phase = UpdatePhase.idle,
    this.release,
    this.progress = 0,
    this.error,
    this.errorDetail,
    this.inPlace = true,
  });
  final UpdatePhase phase;
  final UpdateRelease? release;
  final double progress;
  final String? error;

  /// Non-sensitive detail for [error], e.g. the OS error number of a launch.
  final String? errorDetail;

  /// False for a portable Windows copy: it is sent to the download page.
  final bool inPlace;
  bool get busy => const {UpdatePhase.checking, UpdatePhase.downloading, UpdatePhase.installing}.contains(phase);

  UpdateState copyWith({UpdatePhase? phase, double? progress}) =>
      UpdateState(phase: phase ?? this.phase, release: release, progress: progress ?? this.progress, inPlace: inPlace);
}

/// Persists the newest accepted feed `issuedAt` in device-local settings.
final class SettingsFeedHighWater implements UpdateFeedHighWater {
  const SettingsFeedHighWater(this._settings);
  final SettingsService _settings;
  @override
  Future<DateTime?> read() async => _settings.currentLocal.updateFeedIssuedAt;
  @override
  Future<void> write(DateTime issuedAt) =>
      _settings.updateLocal(_settings.currentLocal.copyWith(updateFeedIssuedAt: issuedAt.toUtc()));
}

final updateServiceProvider = Provider<UpdateService>(
  (ref) => UpdateService(feedHighWater: SettingsFeedHighWater(ref.read(settingsServiceProvider))),
);
final updateControllerProvider = NotifierProvider<UpdateController, UpdateState>(UpdateController.new);

/// Opens the public download page (portable copies, failed updates).
final updateOpenUrlProvider = Provider<Future<bool> Function(Uri)>(
  (ref) =>
      (uri) => launchUrl(uri, mode: LaunchMode.externalApplication),
);

/// Leaves the app so Setup (waiting for this process) can replace its files.
final updateAppExitProvider = Provider<Future<void> Function()>(
  (ref) => () async {
    try {
      await RustAppServices.current?.close().timeout(const Duration(seconds: 5));
    } on Object {
      // Setup is already running; a slow shutdown must not keep files open.
    }
    exit(0);
  },
);

Uri updateDownloadPage(String languageCode) => Uri.https('consolecrypt.dev', '/download', {'lang': languageCode});

class UpdateController extends Notifier<UpdateState> {
  File? _installer;
  @override
  UpdateState build() => const UpdateState();

  Future<void> check() async {
    if (state.busy) return;
    state = const UpdateState(phase: UpdatePhase.checking);
    try {
      final service = ref.read(updateServiceProvider);
      final release = await service.check();
      final inPlace = release == null || await service.canInstallInPlace();
      if (!ref.mounted) return;
      state = UpdateState(
        phase: release == null ? UpdatePhase.current : UpdatePhase.available,
        release: release,
        inPlace: inPlace,
      );
    } on Object catch (error) {
      if (ref.mounted) state = _failed(error, null, inPlace: state.inPlace);
    }
  }

  Future<void> downloadAndInstall({String? macosSaveTitle, String? macosSavePrompt}) async {
    final release = state.release;
    if (release == null || state.busy) return;
    final inPlace = state.inPlace;
    final service = ref.read(updateServiceProvider);
    try {
      if (!inPlace) throw const UpdateException('portable');
      if (_installer == null || !_installer!.existsSync() || _installer!.uri.pathSegments.last != release.fileName) {
        state = UpdateState(phase: UpdatePhase.downloading, release: release, inPlace: inPlace);
        _installer = await service.download(release, (value) {
          if (ref.mounted && state.phase == UpdatePhase.downloading) state = state.copyWith(progress: value);
        });
      }
      if (!ref.mounted) return;
      state = UpdateState(phase: UpdatePhase.installing, release: release, progress: 1, inPlace: inPlace);
      final outcome = await service.install(
        _installer!,
        release,
        macosSaveTitle: macosSaveTitle,
        macosSavePrompt: macosSavePrompt,
      );
      if (!ref.mounted) return;
      state = UpdateState(
        phase: switch (outcome) {
          UpdateInstallResult.androidPermission => UpdatePhase.permission,
          UpdateInstallResult.cancelled => UpdatePhase.ready,
          _ => UpdatePhase.opened,
        },
        release: release,
        progress: 1,
        inPlace: inPlace,
      );
      if (outcome == UpdateInstallResult.exitWindows) await ref.read(updateAppExitProvider)();
    } on Object catch (error) {
      // A verified cache can change or an installer can fail to launch. A retry
      // must fetch fresh bytes rather than repeatedly reusing a bad file.
      _installer = null;
      if (ref.mounted) state = _failed(error, release, inPlace: inPlace);
    }
  }

  /// Records that the download page could not be opened (portable copies).
  void downloadPageFailed() {
    state = UpdateState(phase: UpdatePhase.failed, release: state.release, error: 'browser', inPlace: state.inPlace);
  }

  static UpdateState _failed(Object error, UpdateRelease? release, {required bool inPlace}) => UpdateState(
    phase: UpdatePhase.failed,
    release: release,
    error: error is UpdateException ? error.code : (error is FileSystemException ? 'storage' : 'network'),
    errorDetail: error is UpdateException ? error.detail : null,
    inPlace: inPlace,
  );
}
