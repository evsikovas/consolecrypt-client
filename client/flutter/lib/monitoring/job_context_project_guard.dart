import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/monitoring/job_context_focus.dart';
import 'package:consolecrypt/monitoring/job_context_navigation.dart';
import 'package:consolecrypt/monitoring/job_metric_correlation.dart';

/// Current metadata comparison only. No service/read/save/selection operation.
/// The caller owns the original session fence before and after its actual reads.
AutomationProject? jobContextProjectFromCurrentMetadata(
  JobContextFocus focus,
  AutomationProjectCatalog catalog,
  Object? jobs,
  DateTime observedUtc, {
  AutomationProjectRevision? files,
}) {
  if (focus.selection.destination != JobContextDestination.ansibleProject ||
      catalog.version != 1 ||
      catalog.readOnly ||
      focus.project == null ||
      !observedUtc.isUtc) {
    return null;
  }
  final expected = focus.project!, selection = focus.selection;
  final matches = catalog.projects
      .where(
        (project) =>
            project.id == expected.id &&
            project.revision == expected.revision &&
            !project.readOnly &&
            (project.importState == null || project.importState == 'ready'),
      )
      .toList();
  if (matches.length != 1 ||
      selection.projectId != expected.id ||
      selection.projectRevision != expected.revision ||
      files != null && files.digest != expected.digest) {
    return null;
  }
  final context = focus.originalContext;
  if (context == null) return null;
  final projection = JobMetricCorrelation()..bind(context);
  try {
    final ticket = projection.begin(context)!;
    if (!projection.complete(ticket, jobs, observedUtc) || projection.snapshot.failure != null) return null;
    final rows = projection.snapshot.markers
        .where(
          (job) =>
              job.jobId == selection.jobId &&
              job.notificationRevision == selection.jobRevision &&
              job.projectId == expected.id &&
              job.revisionDigest == expected.digest,
        )
        .toList();
    return rows.length == 1 ? matches.single : null;
  } finally {
    projection.dispose();
  }
}

/// A fresh existing job catalog must still name the original admitted job and
/// exact approved target. This is metadata consistency, never a metrics grant.
bool jobContextMonitoringJobMatches(JobContextFocus focus, Object? catalog) {
  if (!focus.metadataOnly || focus.selection.destination != JobContextDestination.monitoringBinding ||
      catalog is! Map || catalog['schema_version'] != 1 || catalog['read_only'] != false ||
      catalog['jobs'] is! List || (catalog['jobs'] as List).length > JobMetricCorrelation.maxCatalogJobs) { return false; }
  final selected = (catalog['jobs'] as List).whereType<Map<Object?, Object?>>().where((row) => row['job_id'] == focus.selection.jobId).toList();
  if (selected.length != 1) return false;
  final hosts = jobContextApprovedHostIds(selected.single, profileId: focus.scope.profileId,
    jobId: focus.selection.jobId!, revision: focus.selection.jobRevision!,
    projectId: focus.selection.projectId!, manifestDigest: focus.originManifestDigest!);
  return hosts?.contains(focus.metadata.hostId) == true;
} 

/// Shared fixed admission-fact projection for the chooser and target receiver.
Set<String>? jobContextApprovedHostIds(Map<Object?, Object?> row, {required String profileId, required String jobId,
  required int revision, required String projectId, required String manifestDigest}) {
  final uuid = RegExp(r'^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$');
  if (![profileId, jobId, projectId].every(uuid.hasMatch) || revision < 1 || revision > 9007199254740991 ||
      !RegExp(r'^[0-9a-f]{64}$').hasMatch(manifestDigest)) { return null; }
  final manifest = row['manifest'];
  if (row['version'] is! int || row['version'] != 1 || row['job_id'] != jobId ||
      row['notification_revision'] is! int || row['notification_revision'] != revision || row['manifest_digest'] != manifestDigest ||
      row['project_id'] != projectId ||
      !const {'running','failed','cancelled','interrupted','unknown','acknowledged'}.contains(row['state']) ||
      manifest is! Map || manifest['version'] is! int || manifest['version'] != 1 || manifest['job_id'] != jobId ||
      manifest['tenant_id'] != profileId || manifest['project_id'] != projectId ||
      row['host_ids'] is! List || manifest['targets'] is! List) { return null; }
  final hosts = row['host_ids'] as List, targets = manifest['targets'] as List;
  if (hosts.isEmpty || hosts.length > 10000 || targets.length != hosts.length ||
      hosts.any((host) => host is! String || !uuid.hasMatch(host))) { return null; }
  final declared = hosts.toSet(), approved = <String>{};
  for (final target in targets) {
    if (target is! Map || target['host_id'] is! String || !uuid.hasMatch(target['host_id'] as String) ||
        !approved.add(target['host_id'] as String)) { return null; }
  }
  return declared.length == hosts.length && declared.length == approved.length && declared.containsAll(approved)
    ? Set.unmodifiable(approved) : null;
}
