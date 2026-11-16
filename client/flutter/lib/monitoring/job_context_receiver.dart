import 'package:consolecrypt/monitoring/job_context_focus.dart';
import 'package:consolecrypt/monitoring/job_context_navigation.dart';
import 'package:consolecrypt/monitoring/monitoring_host_context.dart';
import 'package:consolecrypt/monitoring/monitoring_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// Fresh original-metadata check at the user's separate Connect/Open action.
/// This does not consume/recreate a focus packet or adopt the latest nonce.
Future<bool> recheckJobContextFocus(WidgetRef ref, JobContextFocus focus, {
  required bool Function() receiverCurrent,
}) async {
  if (!receiverCurrent() || !ref.read(monitoringScopeProvider).current) return false;
  final original = ref.read(monitoringSessionProvider), service = ref.read(monitoringSessionProvider)?.service;
  if (!identical(original?.correlationScope, focus.scope) || service is! MonitoringHostContextReader) return false;
  final reader = service as MonitoringHostContextReader;
  if (!reader.hostContextSupported) return false;
  final checked = await reader.recheckHostContext(focus.metadata);
  return receiverCurrent() && ref.read(monitoringScopeProvider).current && identical(ref.read(monitoringSessionProvider), original) &&
    sameJobContextMetadata(focus.metadata, checked);
}

/// Called by the destination's existing State, not by a new global owner. The
/// packet contains no service. The existing ORIGINAL session is captured before
/// await, and subsequent provider reads only compare it, never adopt a new one.
Future<JobContextFocus?> receiveJobContextFocus(
  WidgetRef ref,
  JobContextFocus focus, {
  required JobContextDestination destination,
  required bool Function() receiverCurrent,
}) async {
  if (!receiverCurrent() || !ref.read(monitoringScopeProvider).current) return null;
  final session = ref.read(monitoringSessionProvider), original = focus.scope;
  final service = session?.service;
  if (session == null || !identical(session.correlationScope, original) || service is! MonitoringHostContextReader) {
    return null;
  }
  final reader = service as MonitoringHostContextReader;
  if (!reader.hostContextSupported) return null;
  bool current() => receiverCurrent() && ref.read(monitoringScopeProvider).current && identical(ref.read(monitoringSessionProvider), session);
  return admitJobContextFocus(
    focus,
    destination: destination,
    originalSessionScope: original,
    currentSessionScope: () => current() ? session.correlationScope : null,
    targetCurrent: current,
    recheck: reader.recheckHostContext,
  );
}
