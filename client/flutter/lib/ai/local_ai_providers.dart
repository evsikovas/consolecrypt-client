import 'dart:async';
import 'dart:ui' show AppExitResponse;

import 'package:consolecrypt/ai/device_local_ai_service.dart';
import 'package:consolecrypt/ai/local/local_ai_hardware.dart';
import 'package:consolecrypt/ai/local/local_ai_runtime.dart';
import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/core/storage/storage_providers.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

final deviceLocalAiAvailableProvider = Provider<bool>(
  (ref) =>
      (AppPlatform.isMacOS || AppPlatform.isWindows || AppPlatform.isLinux) &&
      ref.watch(appServicesProvider).ai is DeviceLocalAiBridge,
);

final localAiRuntimeProvider = Provider<LocalAiRuntime>((ref) {
  final runtime = LocalAiRuntime(supportDirectory: ref.watch(appStoragePathsProvider).localAiRoot);
  return bindLocalAiRuntimeLifecycle(ref, runtime);
});

/// Binds an already-created device process owner. AppServices replacement
/// revokes its endpoint synchronously but must never replace an unjoined child.
/// The same binding is exercised with controlled owned children in tests.
LocalAiRuntime bindLocalAiRuntimeLifecycle(Ref ref, LocalAiRuntime runtime) {
  void Function()? detachExit;
  AppLifecycleListener? lifecycle;
  void bindExit(ApprovedExitActions? approved) {
    detachExit?.call();
    lifecycle?.dispose();
    detachExit = approved?.register(runtime.stop);
    lifecycle = approved == null
        ? AppLifecycleListener(
            onExitRequested: () async {
              try {
                await runtime.stop();
                return AppExitResponse.exit;
              } catch (_) {
                return AppExitResponse.cancel;
              }
            },
          )
        : null;
  }

  bindExit(ref.read(appServicesProvider).approvedExitActions);
  ref.listen(appServicesProvider, (_, next) {
    // stop() invalidates generation/endpoint before its first await. Failure
    // remains on THIS owner, visible to the same status/Stop retry UI.
    unawaited(runtime.stop().catchError((Object _) {}));
    bindExit(next.approvedExitActions);
  });
  unawaited(runtime.initialize());
  ref.onDispose(() {
    detachExit?.call();
    lifecycle?.dispose();
    // Runtime owns its unjoined child; no success/retirement claim on failure.
    unawaited(runtime.dispose().catchError((Object _) {}));
  });
  return runtime;
}

final localAiStatusProvider = StreamProvider<LocalAiRuntimeStatus>((ref) {
  final runtime = ref.watch(localAiRuntimeProvider);
  return Stream<LocalAiRuntimeStatus>.multi((output) {
    output.add(runtime.status);
    final subscription = runtime.statuses.listen(output.add, onError: output.addError, onDone: output.close);
    output.onCancel = subscription.cancel;
  });
});

final localAiHardwareProvider = FutureProvider<LocalAiHardware>((ref) => LocalAiHardware.detect());

AiService withDeviceLocalAi(Ref ref, AiService delegate) {
  if (!ref.watch(deviceLocalAiAvailableProvider)) return delegate;
  final service = DeviceLocalAiService(
    delegate: delegate,
    bridge: delegate as DeviceLocalAiBridge,
    runtime: ref.watch(localAiRuntimeProvider),
    settings: ref.watch(settingsServiceProvider),
    profiles: ref.watch(profileServiceProvider),
    vault: ref.watch(vaultServiceProvider),
  );
  ref.onDispose(() => unawaited(service.dispose()));
  return service;
}
