import 'dart:async';
import 'dart:ui' show AppExitResponse;

import 'package:consolecrypt/ai/device_local_ai_service.dart';
import 'package:consolecrypt/ai/local/local_ai_runtime.dart';
import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

final deviceLocalAiAvailableProvider = Provider<bool>(
  (ref) => (AppPlatform.isMacOS || AppPlatform.isWindows) && ref.watch(appServicesProvider).ai is DeviceLocalAiBridge,
);

final localAiRuntimeProvider = Provider<LocalAiRuntime>((ref) {
  final runtime = LocalAiRuntime();
  final lifecycle = AppLifecycleListener(
    onExitRequested: () async {
      await runtime.stop();
      return AppExitResponse.exit;
    },
  );
  unawaited(runtime.initialize());
  ref.onDispose(() {
    lifecycle.dispose();
    unawaited(runtime.dispose());
  });
  return runtime;
});

final localAiStatusProvider = StreamProvider<LocalAiRuntimeStatus>((ref) {
  final runtime = ref.watch(localAiRuntimeProvider);
  return Stream<LocalAiRuntimeStatus>.multi((output) {
    output.add(runtime.status);
    final subscription = runtime.statuses.listen(output.add, onError: output.addError, onDone: output.close);
    output.onCancel = subscription.cancel;
  });
});

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
