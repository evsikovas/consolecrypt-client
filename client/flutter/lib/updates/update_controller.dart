import 'dart:io';

import 'package:consolecrypt/core/bridge/rust_app_services.dart';
import 'package:consolecrypt/updates/update_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

enum UpdatePhase { idle, checking, current, available, downloading, ready, installing, permission, opened, failed }

class UpdateState {
  const UpdateState({this.phase = UpdatePhase.idle, this.release, this.progress = 0, this.error});
  final UpdatePhase phase;
  final UpdateRelease? release;
  final double progress;
  final String? error;
  bool get busy => const {UpdatePhase.checking, UpdatePhase.downloading, UpdatePhase.installing}.contains(phase);
}

final updateServiceProvider = Provider<UpdateService>((ref) => UpdateService());
final updateControllerProvider = NotifierProvider<UpdateController, UpdateState>(UpdateController.new);

class UpdateController extends Notifier<UpdateState> {
  File? _installer;
  @override
  UpdateState build() => const UpdateState();

  Future<void> check() async {
    if (state.busy) return;
    state = const UpdateState(phase: UpdatePhase.checking);
    try {
      final release = await ref.read(updateServiceProvider).check();
      if (!ref.mounted) return;
      state = UpdateState(phase: release == null ? UpdatePhase.current : UpdatePhase.available, release: release);
    } on Object catch (error) {
      if (ref.mounted) state = UpdateState(phase: UpdatePhase.failed, error: _error(error));
    }
  }

  Future<void> downloadAndInstall({String? macosSaveTitle, String? macosSavePrompt}) async {
    final release = state.release;
    if (release == null || state.busy) return;
    final service = ref.read(updateServiceProvider);
    try {
      if (_installer == null || !_installer!.existsSync() || _installer!.uri.pathSegments.last != release.fileName) {
        state = UpdateState(phase: UpdatePhase.downloading, release: release);
        _installer = await service.download(release, (value) {
          if (ref.mounted) state = UpdateState(phase: UpdatePhase.downloading, release: release, progress: value);
        });
      }
      if (!ref.mounted) return;
      state = UpdateState(phase: UpdatePhase.installing, release: release, progress: 1);
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
      );
      if (outcome == UpdateInstallResult.exitWindows) {
        await RustAppServices.current?.close();
        exit(0);
      }
    } on Object catch (error) {
      // A verified cache can change or an installer can fail to launch. A retry
      // must fetch fresh bytes rather than repeatedly reusing a bad file.
      _installer = null;
      if (ref.mounted) state = UpdateState(phase: UpdatePhase.failed, release: release, error: _error(error));
    }
  }

  String _error(Object error) => error is UpdateException ? error.code : 'network';
}
