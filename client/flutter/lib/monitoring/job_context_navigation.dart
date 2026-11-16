import 'package:consolecrypt/monitoring/job_metric_correlation.dart';

enum JobContextDestination { sshHost, dockerHost, ansibleProject, monitoringBinding }

enum JobContextHostProtocol { ssh, rdp, unknown }

enum JobContextNavigationFailure {
  contextUnavailable,
  metadataUnavailable,
  profileChanged,
  scopeChanged,
  hostChanged,
  sourceChanged,
  bindingChanged,
  unsupportedProtocol,
  missingJob,
  jobChanged,
  missingProject,
  projectChanged,
}

final _uuid = RegExp(r'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$');
bool _id(Object? value) =>
    value is String && value.length == 36 && value != '00000000-0000-0000-0000-000000000000' && _uuid.hasMatch(value);
bool _revision(Object? value) => value is int && value > 0 && value <= 9007199254740991;
bool _sha(String value) => value.length == 64 && RegExp(r'^[0-9a-f]{64}$').hasMatch(value);

/// Data from the proposed Native metadata-only read, not authorization. No
/// updatedAt/UI counter can substitute for the actual WorkingSet revision.
final class JobContextMetadata {
  const JobContextMetadata._(
    this.profileId,
    this.notificationScopeId,
    this.hostId,
    this.hostRevision,
    this.protocol,
    this.sourceId,
    this.sourceRevision,
    this.bindingId,
    this.bindingRevision,
  );
  final String profileId, notificationScopeId, hostId, sourceId, bindingId;
  final int hostRevision, sourceRevision, bindingRevision;
  final JobContextHostProtocol protocol;
  static JobContextMetadata? parse(Object? raw) {
    const keys = {
      'version',
      'profileId',
      'notificationScopeId',
      'hostId',
      'hostRevision',
      'protocol',
      'sourceId',
      'sourceRevision',
      'bindingId',
      'bindingRevision',
    };
    if (raw is! Map ||
        raw.length != keys.length ||
        raw.keys.any((key) => !keys.contains(key)) ||
        raw['version'] is! int ||
        raw['version'] != 1 ||
        !['profileId', 'notificationScopeId', 'hostId', 'sourceId', 'bindingId'].every((key) => _id(raw[key])) ||
        !['hostRevision', 'sourceRevision', 'bindingRevision'].every((key) => _revision(raw[key]))) {
      return null;
    }
    final protocol = switch (raw['protocol']) {
      'ssh' => JobContextHostProtocol.ssh,
      'rdp' => JobContextHostProtocol.rdp,
      _ => JobContextHostProtocol.unknown,
    };
    return JobContextMetadata._(
      raw['profileId'] as String,
      raw['notificationScopeId'] as String,
      raw['hostId'] as String,
      raw['hostRevision'] as int,
      protocol,
      raw['sourceId'] as String,
      raw['sourceRevision'] as int,
      raw['bindingId'] as String,
      raw['bindingRevision'] as int,
    );
  }
}

/// A current project read selected explicitly for this proposal. `digest` is
/// actual source revision data, not the project's SQL metadata revision.
final class JobContextProject {
  JobContextProject(this.id, this.revision, this.digest) {
    if (!_id(id) || !_revision(revision) || !_sha(digest)) {
      throw ArgumentError('project');
    }
  }
  final String id, digest;
  final int revision;
}

final class JobContextNavigationFrame {
  JobContextNavigationFrame({
    required this.context,
    required this.notificationScopeId,
    required this.metadata,
    this.jobs,
    this.project,
  }) {
    if (!_id(notificationScopeId)) throw ArgumentError('scope');
  }
  final JobCorrelationContext context;
  final String notificationScopeId;
  final JobContextMetadata? metadata;
  final JobMetricSnapshot? jobs;
  final JobContextProject? project;
}

/// Pending read-only navigation. Created only by its original projection and
/// consumed once after the original workspace checks current Native context.
final class JobContextNavigationIntent {
  const JobContextNavigationIntent._(this._owner, this._frame, this.destination, this._job);
  final JobContextNavigation _owner;
  final JobContextNavigationFrame _frame;
  final JobContextDestination destination;
  final JobMetricMarker? _job;
}

final class JobContextNavigationDecision {
  const JobContextNavigationDecision._(this.intent, this.failure);
  final JobContextNavigationIntent? intent;
  final JobContextNavigationFailure? failure;
}

/// Selection proposal only. No route, callback, address, credential, command,
/// session, connect/start/retry flag or arbitrary payload can be carried.
final class JobContextNavigationSelection {
  const JobContextNavigationSelection._(
    this.destination,
    this.profileId,
    this.hostId,
    this.hostRevision,
    this.sourceId,
    this.sourceRevision,
    this.bindingId,
    this.bindingRevision,
    this.projectId,
    this.projectRevision,
    this.jobId,
    this.jobRevision,
  );
  /// Metadata-only reverse focus. No graph/window, query or execution grant.
  static JobContextNavigationSelection? monitoring(JobContextMetadata metadata, {
    required String jobId, required int jobRevision, required String projectId,
  }) {
    if (!_id(jobId) || !_revision(jobRevision) || !_id(projectId) ||
        metadata.protocol == JobContextHostProtocol.unknown) { return null; }
    return JobContextNavigationSelection._(JobContextDestination.monitoringBinding,
      metadata.profileId, metadata.hostId, metadata.hostRevision, metadata.sourceId,
      metadata.sourceRevision, metadata.bindingId, metadata.bindingRevision,
      projectId, null, jobId, jobRevision);
  }
  final JobContextDestination destination;
  final String profileId, hostId, sourceId, bindingId;
  final int hostRevision, sourceRevision, bindingRevision;
  final String? projectId, jobId;
  final int? projectRevision, jobRevision;
}

/// Pure presentation projection, owned by the EXISTING workspace lifecycle.
/// No listener, RPC, service, timer, deferred task or automatic navigation.
final class JobContextNavigation {
  JobContextNavigationFrame? _frame;
  JobContextNavigationIntent? _pending;
  bool _disposed = false;
  void bind(JobContextNavigationFrame? frame) {
    if (_disposed || identical(frame, _frame)) return;
    _frame = frame;
    _pending = null;
  }

  JobContextNavigationDecision propose(
    JobContextNavigationFrame original,
    JobContextDestination destination, {
    JobMetricMarker? job,
  }) {
    _pending = null;
    final failure = _check(original, destination, job);
    if (failure != null) return JobContextNavigationDecision._(null, failure);
    final intent = JobContextNavigationIntent._(this, original, destination, job);
    _pending = intent;
    return JobContextNavigationDecision._(intent, null);
  }

  JobContextNavigationSelection? take(JobContextNavigationIntent original) {
    if (!identical(original._owner, this) || !identical(original, _pending)) {
      return null;
    }
    _pending = null;
    if (_check(original._frame, original.destination, original._job) != null) {
      return null;
    }
    final c = original._frame.context, job = original._job;
    final project = original.destination == JobContextDestination.ansibleProject ? original._frame.project : null;
    return JobContextNavigationSelection._(
      original.destination,
      c.scope.profileId,
      c.hostId,
      c.hostRevision,
      c.sourceId,
      c.sourceRevision,
      c.bindingId,
      c.bindingRevision,
      project?.id,
      project?.revision,
      job?.jobId,
      job?.notificationRevision,
    );
  }

  JobContextNavigationFailure? _check(
    JobContextNavigationFrame original,
    JobContextDestination destination,
    JobMetricMarker? job,
  ) {
    if (_disposed || !identical(original, _frame)) {
      return JobContextNavigationFailure.contextUnavailable;
    }
    final c = original.context, m = original.metadata;
    if (m == null) return JobContextNavigationFailure.metadataUnavailable;
    if (m.profileId != c.scope.profileId) {
      return JobContextNavigationFailure.profileChanged;
    }
    if (m.notificationScopeId != original.notificationScopeId) {
      return JobContextNavigationFailure.scopeChanged;
    }
    if (m.hostId != c.hostId || m.hostRevision != c.hostRevision) {
      return JobContextNavigationFailure.hostChanged;
    }
    if (m.sourceId != c.sourceId || m.sourceRevision != c.sourceRevision) {
      return JobContextNavigationFailure.sourceChanged;
    }
    if (m.bindingId != c.bindingId || m.bindingRevision != c.bindingRevision) {
      return JobContextNavigationFailure.bindingChanged;
    }
    if (m.protocol != JobContextHostProtocol.ssh) {
      return JobContextNavigationFailure.unsupportedProtocol;
    }
    if (destination == JobContextDestination.monitoringBinding) {
      return JobContextNavigationFailure.contextUnavailable;
    }
    if (job != null) {
      final jobs = original.jobs;
      if (jobs == null ||
          jobs.failure != null ||
          !identical(jobs.context, c) ||
          job.notificationRevision < 1 ||
          !jobs.markers.any((row) => identical(row, job))) {
        return JobContextNavigationFailure.jobChanged;
      }
    }
    if (destination == JobContextDestination.ansibleProject) {
      if (job == null) return JobContextNavigationFailure.missingJob;
      final project = original.project;
      if (project == null) return JobContextNavigationFailure.missingProject;
      if (project.id != job.projectId || project.digest != job.revisionDigest) {
        return JobContextNavigationFailure.projectChanged;
      }
    }
    return null;
  }

  void dispose() {
    _disposed = true;
    _frame = null;
    _pending = null;
  }
}
