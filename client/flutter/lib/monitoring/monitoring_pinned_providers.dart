import 'dart:ui' show AppExitResponse;

import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/monitoring/monitoring_pinned_observation.dart';
import 'package:consolecrypt/monitoring/monitoring_pinned_transport.dart';
import 'package:consolecrypt/monitoring/monitoring_providers.dart';

import 'package:flutter/widgets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// Watching this owner does not allocate a transport or issue a request. The
/// immutable unlocked profile identity bounds every await and all state.
final monitoringPinnedObservationProvider = Provider<MonitoringPinnedObservation?>((ref) {
  final scope = ref.watch(monitoringScopeProvider), backend = ref.watch(monitoringBackendProvider);
  if (!scope.current || scope.profileId == null || backend == null) return null;
  final owner = MonitoringPinnedObservation(
    createTransport: () => ScopedMonitoringPinTransport(backend),
    isCurrent: () => scope.current && identical(ref.read(monitoringScopeProvider), scope),
    classify: (error) {
      if (error is AppException) {
        if (error.reason == AppErrorReason.vaultLocked || error.reason == AppErrorReason.noActiveProfile) {
          return MonitoringPinDisposition.scopeChanged;
        }
        return MonitoringPinDisposition.stop;
      }
      return MonitoringPinnedObservation.classifyFailure(error);
    },
  );
  MonitoringPinLifecycle projectLifecycle(AppLifecycleState state) => switch (state) {
    AppLifecycleState.resumed => MonitoringPinLifecycle.resumed,
    AppLifecycleState.inactive => MonitoringPinLifecycle.inactive,
    AppLifecycleState.hidden => MonitoringPinLifecycle.hidden,
    AppLifecycleState.paused => MonitoringPinLifecycle.paused,
    AppLifecycleState.detached => MonitoringPinLifecycle.detached,
  };
  if (WidgetsBinding.instance.lifecycleState case final state?) owner.lifecycle(projectLifecycle(state));
  final lifecycle = AppLifecycleListener(
    onStateChange: (state) => owner.lifecycle(projectLifecycle(state)),
    onExitRequested: () {
      owner.lifecycle(MonitoringPinLifecycle.exit);
      return Future.value(AppExitResponse.exit);
    },
  );
  ref.onDispose(() {
    lifecycle.dispose();
    owner.dispose();
  });
  return owner;
});
