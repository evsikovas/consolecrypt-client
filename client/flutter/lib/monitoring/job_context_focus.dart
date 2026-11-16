import 'package:consolecrypt/monitoring/job_context_navigation.dart';
import 'package:consolecrypt/monitoring/job_metric_correlation.dart';
import 'package:consolecrypt/monitoring/monitoring_host_context.dart';

/// Typed in-memory router extra. Only fixed metadata + original session-scope
/// identity are carried: no Host, service, callback, address, endpoint or command.
/// This is a focus proposal, never connection/read/execution authorization.
final class JobContextFocus {
  JobContextFocus._(this.selection, this.metadata, JobCorrelationContext original, this.project)
      : originalContext = original, scope = original.scope, originManifestDigest = null;
  JobContextFocus._monitoring(this.selection, this.metadata, this.scope, this.originManifestDigest)
      : originalContext = null, project = null;
  final JobContextNavigationSelection selection;
  final JobContextMetadata metadata;
  /// Original accepted UTC domain + fixed refs only, never a query or route.
  /// The destination reuses it to validate the original selected job projection.
  final JobCorrelationContext? originalContext;
  final JobCorrelationScope scope;
  /// The admitted job manifest hash, not a project revision/snapshot identifier.
  final String? originManifestDigest;
  bool get metadataOnly => originalContext == null;

  static JobContextFocus? forMonitoring({required JobCorrelationScope scope,
    required JobContextMetadata metadata, required String jobId, required int jobRevision,
    required String projectId, required String manifestDigest}) {
    if (scope.profileId != metadata.profileId || !RegExp(r'^[0-9a-f]{64}$').hasMatch(manifestDigest)) return null;
    final selection = JobContextNavigationSelection.monitoring(metadata,
      jobId: jobId, jobRevision: jobRevision, projectId: projectId);
    return selection == null ? null : JobContextFocus._monitoring(selection, metadata, scope, manifestDigest);
  }
  final JobContextProject? project;
  bool _claimed = false;

  static JobContextFocus? fromTaken(JobContextNavigationSelection selection, JobContextNavigationFrame original) {
    if (selection.destination == JobContextDestination.monitoringBinding) return null;
    final metadata = original.metadata, c = original.context;
    if (metadata == null ||
        metadata.hostId != c.hostId ||
        metadata.hostRevision != c.hostRevision ||
        metadata.sourceId != c.sourceId ||
        metadata.sourceRevision != c.sourceRevision ||
        metadata.bindingId != c.bindingId ||
        metadata.bindingRevision != c.bindingRevision ||
        metadata.notificationScopeId != original.notificationScopeId ||
        metadata.protocol != JobContextHostProtocol.ssh ||
        metadata.profileId != c.scope.profileId ||
        selection.profileId != metadata.profileId ||
        selection.hostId != metadata.hostId ||
        selection.hostRevision != metadata.hostRevision ||
        selection.sourceId != metadata.sourceId ||
        selection.sourceRevision != metadata.sourceRevision ||
        selection.bindingId != metadata.bindingId ||
        selection.bindingRevision != metadata.bindingRevision) {
      return null;
    }
    JobContextProject? project;
    if (selection.destination == JobContextDestination.ansibleProject) {
      project = original.project;
      final matchingJob = original.jobs?.markers
          .where(
            (job) =>
                job.jobId == selection.jobId &&
                job.notificationRevision == selection.jobRevision &&
                job.projectId == selection.projectId,
          )
          .firstOrNull;
      if (project == null ||
          project.id != selection.projectId ||
          project.revision != selection.projectRevision ||
          matchingJob == null ||
          matchingJob.revisionDigest != project.digest) {
        return null;
      }
    }
    return JobContextFocus._(selection, metadata, c, project);
  }

  JobContextFocusAdmission? claim(JobCorrelationScope originalSessionScope, JobContextDestination destination) {
    if (_claimed) return null;
    _claimed = true;
    if (metadataOnly != (destination == JobContextDestination.monitoringBinding)) return null;
    if (!identical(scope, originalSessionScope) || selection.destination != destination) return null;
    return JobContextFocusAdmission._(this);
  }

  @override
  String toString() => 'JobContextFocus(<read-only fixed selection>)';
}

final class JobContextFocusAdmission {
  JobContextFocusAdmission._(this._focus);
  final JobContextFocus _focus;
  bool _finished = false;
  JobContextFocus? finish(JobContextMetadata checked, JobCorrelationScope currentSessionScope) {
    if (_finished) return null;
    _finished = true;
    if (!identical(_focus.scope, currentSessionScope) || !sameJobContextMetadata(_focus.metadata, checked)) return null;
    return _focus;
  }
}

/// Target's existing State passes its captured service callback and validity
/// check. The packet never owns either. A source-page hide is intentionally not
/// a session lifetime change; target hide, profile lock/reopen and new props are.
Future<JobContextFocus?> admitJobContextFocus(
  JobContextFocus focus, {
  required JobContextDestination destination,
  required JobCorrelationScope originalSessionScope,
  required JobCorrelationScope? Function() currentSessionScope,
  required bool Function() targetCurrent,
  required Future<JobContextMetadata> Function(JobContextMetadata original) recheck,
}) async {
  if (!targetCurrent() || !identical(originalSessionScope, currentSessionScope())) return null;
  final admission = focus.claim(originalSessionScope, destination);
  if (admission == null) return null;
  final checked = await recheck(focus.metadata);
  if (!targetCurrent()) return null;
  final current = currentSessionScope();
  if (current == null) return null;
  return admission.finish(checked, current);
}
