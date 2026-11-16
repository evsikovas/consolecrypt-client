import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/job_metric_correlation.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

final class MonitoringScope {
  const MonitoringScope(this.profileId, this.unlocked, {this.isCurrent, this.nativeScope});
  final Object? profileId;
  final bool unlocked;
  final bool Function()? isCurrent;
  final RuntimeNoticeScope? nativeScope;

  /// Synchronous local epoch fence, not a Native identity or a read grant.
  bool get current => unlocked && (isCurrent?.call() ?? true);
}

final monitoringScopeProvider = Provider<MonitoringScope>((ref) {
  final profile = ref.watch(activeProfileProvider.select((profile) => profile?.id));
  final unlocked = ref.watch(appStageProvider) == AppStage.unlocked;
  final events = ref.watch(appServicesProvider).runtimeNotices;
  final originalEpoch = events?.currentScope.epoch;
  final originalUnavailable = events?.currentScope.streamUnavailable ?? false;
  final originalNative = events?.currentScope.unsupported == false && !originalUnavailable ? events?.currentScope.scope : null;
  final nativeMatchesProfile = originalNative == null || originalNative.profileId == profile?.value;
  bool sameNative(RuntimeNoticeScope? current) => originalNative == null ? current == null : originalNative.same(current);
  // Reuse the existing hub fan-out. Its synchronous boundary precedes the
  // profiles refresh, including a lock/reopen with the same profile ID.
  final subscription = events?.watchRuntimeNotices().listen((frame) {
    if (frame.epoch != originalEpoch || frame.streamUnavailable != originalUnavailable || !sameNative(frame.scope)) ref.invalidateSelf();
  });
  ref.onDispose(() => subscription?.cancel());
  return MonitoringScope(profile, unlocked, nativeScope: nativeMatchesProfile ? originalNative : null,
    isCurrent: () => nativeMatchesProfile && (events == null || events.currentScope.epoch == originalEpoch && !events.currentScope.streamUnavailable && sameNative(events.currentScope.scope)));
});

final class MonitoringSession {
  const MonitoringSession(this.service, this.sources, {this.correlationScope});
  final JobCorrelationScope? correlationScope;
  final MonitoringService service;
  final MonitoringSourceStore sources;
}

final monitoringBackendProvider = Provider<MonitoringBackend?>((ref) => ref.watch(appServicesProvider).monitoring);
final monitoringSessionProvider = Provider<MonitoringSession?>((ref) {
  final scope = ref.watch(monitoringScopeProvider);
  final backend = ref.watch(monitoringBackendProvider);
  if (!scope.current || backend == null) return null;
  final service = backend.createService();
  ref.onDispose(service.cancel);
  final profileId = scope.profileId;
  return MonitoringSession(service, backend.createSourceStore(), correlationScope:
    profileId is ObjectId ? JobCorrelationScope(profileId.value) : null);
});
