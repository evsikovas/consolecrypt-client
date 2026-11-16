import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/monitoring/job_metric_timeline_flow.dart';
import 'package:consolecrypt/monitoring/monitoring_host_context.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';

/// Adapt existing owners; this is not an RPC, permission or service registry.
/// The host captures its service/session/scope + source/binding/window lifetime
/// before constructing this flow and must invalidate it synchronously on change.
JobMetricTimelineFlow monitoringJobTimeline({
  required JobMetricTimelineSelection selection,
  required MonitoringHostContextReader monitoring,
  required AutomationJobService jobs,
  required MonitoringSource source,
  required MonitoringBinding binding,
  required bool Function() isCurrent,
  required DateTime Function() nowUtc,
}) {
  if (!monitoring.hostContextSupported ||
      source.id != selection.sourceId ||
      source.revision != selection.sourceRevision ||
      binding.id != selection.bindingId ||
      binding.revision != selection.bindingRevision ||
      binding.hostId != selection.hostId ||
      binding.sourceId != source.id ||
      binding.sourceRevision != source.revision ||
      binding.state != MonitoringBindingState.current ||
      source.readOnly) {
    throw const MonitoringHostContextException(MonitoringHostContextIssue.invalidRequest);
  }
  return JobMetricTimelineFlow(
    selection: selection,
    capture: () =>
        monitoring.captureHostContext(profileId: selection.scope.profileId, source: source, binding: binding),
    recheck: monitoring.recheckHostContext,
    readJobs: jobs.jobs,
    isCurrent: isCurrent,
    nowUtc: nowUtc,
  );
}
