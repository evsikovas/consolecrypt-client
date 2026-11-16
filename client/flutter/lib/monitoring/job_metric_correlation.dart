// Pure read-only metadata projection. No service, timer, query, lookup or route.
enum JobMetricState {
  queued,
  dispatching,
  running,
  cancelling,
  succeeded,
  failed,
  cancelled,
  interrupted,
  unknown,
  acknowledged,
}

enum JobClockOrigin { nativeClient, runner, observedClient }

enum JobTimeMeaning { admitted, latestStateUpdate, queuedOnRunner, latestRunnerState, received }

enum JobMarkerUnpositioned { missingAdmissionTime, unknownClockSkew, outsidePeriod }

enum JobCorrelationFailure implements Exception { unsupportedSchema, invalidMetadata, duplicateJob, budget }

enum JobInitiator { unknown, nativeDevice }

enum JobProvenanceStatus { legacyUnknown, available, unavailable }

enum JobRunnerObservationStatus { legacyUnknown, available, unavailable }

/// Persisted admission DATA, not a credential, signature check or navigation grant.
/// Only the existing Native-origin catalog projection constructs this value.
final class JobMetricProvenance {
  const JobMetricProvenance._({
    required this.jobId,
    required this.profileId,
    required this.deviceId,
    required this.projectId,
    required this.projectRevision,
    required this.runnerId,
    required this.revisionObjectId,
    required this.revisionDigest,
  });
  final String jobId, profileId, deviceId, projectId, runnerId, revisionObjectId, revisionDigest;
  final int projectRevision;
}

/// Trusted Runner process observations only. Neither a target execution period
/// nor proof of remote success. Duration includes launch and cleanup and must
/// not be compared to wall-clock endpoints to invent an offset/skew estimate.
final class JobRunnerObservation {
  const JobRunnerObservation._(
    this.spawnObservedUtc,
    this.exitObservedUtc,
    this.elapsedMonotonicMs,
    this.hasApplicableClockMeasurement,
  );
  final DateTime spawnObservedUtc, exitObservedUtc;
  final int elapsedMonotonicMs;
  final bool hasApplicableClockMeasurement;
  bool get wallClockReversed => exitObservedUtc.isBefore(spawnObservedUtc);
}

enum JobOutcomeScope { wholeJobNotPerHost }

enum JobCorrelationBindingState { current, staleSource, missingSource, missingHost }

enum JobMarkerPositionKind { clientAdmissionNotExecution }

final _uuid = RegExp(r'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$');
final _digest = RegExp(r'^[0-9a-f]{64}$');
bool _id(Object? v) =>
    v is String && v.length == 36 && v != '00000000-0000-0000-0000-000000000000' && _uuid.hasMatch(v);
bool _sha(Object? v) => v is String && v.length == 64 && _digest.hasMatch(v);
bool _revision(Object? v, {bool legacy = false}) => v is int && v >= (legacy ? 0 : 1) && v <= 9007199254740991;

/// Original observer lifetime, not authorization. Never store a query identity,
/// source fingerprint, manifest or a payload in an opaque Object token.
final class JobCorrelationScope {
  JobCorrelationScope(this.profileId) {
    if (!_id(profileId)) throw ArgumentError('profile');
  }
  final String profileId;
}

/// Explicit measured clock data, never inferred from receipt time or success.
/// offset = producer clock minus reference UTC; uncertainty remains an interval.
final class JobClockAlignment {
  const JobClockAlignment.unknown()
    : offset = null,
      uncertainty = null,
      validFromProducerUtc = null,
      validThroughProducerUtc = null;
  JobClockAlignment.measured(
    this.offset,
    this.uncertainty, {
    required this.validFromProducerUtc,
    required this.validThroughProducerUtc,
  }) {
    if (offset == null ||
        uncertainty == null ||
        offset!.abs() > const Duration(days: 7) ||
        uncertainty!.isNegative ||
        uncertainty! > const Duration(minutes: 5) ||
        validFromProducerUtc == null ||
        validThroughProducerUtc == null ||
        !validFromProducerUtc!.isUtc ||
        !validThroughProducerUtc!.isUtc ||
        validFromProducerUtc!.isAfter(validThroughProducerUtc!)) {
      throw ArgumentError('clock');
    }
  }
  final Duration? offset, uncertainty;

  /// Bounds are in the producer's own timestamp domain. A recent measurement
  /// must never silently align an older job across an unknown clock change.
  final DateTime? validFromProducerUtc, validThroughProducerUtc;
  bool appliesTo(DateTime producerUtc) =>
      offset != null && !producerUtc.isBefore(validFromProducerUtc!) && !producerUtc.isAfter(validThroughProducerUtc!);
}

final class JobCorrelationContext {
  JobCorrelationContext({
    required this.scope,
    required this.hostId,
    required this.hostRevision,
    required this.sourceId,
    required this.sourceRevision,
    required this.bindingId,
    required this.bindingRevision,
    required String bindingHostId,
    required String bindingSourceId,
    required int bindingSourceRevision,
    required JobCorrelationBindingState bindingState,
    required this.startUtc,
    required this.endUtc,
    this.clientClock = const JobClockAlignment.unknown(),
    this.runnerClock = const JobClockAlignment.unknown(),
  }) {
    if (!_id(hostId) ||
        !_id(sourceId) ||
        !_id(bindingId) ||
        !_revision(hostRevision) ||
        !_revision(sourceRevision) ||
        !_revision(bindingRevision) ||
        bindingState != JobCorrelationBindingState.current ||
        bindingHostId != hostId ||
        bindingSourceId != sourceId ||
        bindingSourceRevision != sourceRevision ||
        !startUtc.isUtc ||
        !endUtc.isUtc ||
        !startUtc.isBefore(endUtc)) {
      throw ArgumentError('context');
    }
  }
  final JobCorrelationScope scope;
  final String hostId, sourceId, bindingId;
  final int hostRevision, sourceRevision, bindingRevision;
  final DateTime startUtc, endUtc;
  final JobClockAlignment clientClock, runnerClock;
}

final class JobMetricStamp {
  const JobMetricStamp(this.utc, this.origin, this.meaning);
  final DateTime utc;
  final JobClockOrigin origin;
  final JobTimeMeaning meaning;
}

final class JobMarkerPosition {
  const JobMarkerPosition(this.earliestUtc, this.latestUtc, this.clipped, this.offset, this.uncertainty);
  final DateTime earliestUtc, latestUtc;
  final bool clipped;
  final Duration offset, uncertainty;
  JobMarkerPositionKind get kind => JobMarkerPositionKind.clientAdmissionNotExecution;
}

final class JobMetricMarker {
  const JobMetricMarker._({
    required this.jobId,
    required this.projectId,
    required this.revisionDigest,
    required this.notificationRevision,
    required this.state,
    required this.admitted,
    required this.updated,
    required this.runnerQueued,
    required this.runnerUpdated,
    required this.received,
    required this.position,
    required this.unpositioned,
    required this.provenanceStatus,
    required this.provenance,
    required this.runnerObservationStatus,
    required this.runnerObservation,
  });
  final String jobId, projectId, revisionDigest;
  final int notificationRevision;
  final JobMetricState state;
  final JobMetricStamp? admitted, updated, runnerQueued, runnerUpdated;
  final JobMetricStamp received;
  final JobMarkerPosition? position;
  final JobMarkerUnpositioned? unpositioned;
  final JobProvenanceStatus provenanceStatus;
  final JobMetricProvenance? provenance;
  final JobRunnerObservationStatus runnerObservationStatus;
  final JobRunnerObservation? runnerObservation;
  JobInitiator get initiator => provenance == null ? JobInitiator.unknown : JobInitiator.nativeDevice;
  JobOutcomeScope get outcomeScope => JobOutcomeScope.wholeJobNotPerHost;
  // Target execution/host-revision facts remain unknown even when a Runner
  // process observation exists. Do not relabel process time as target time.
  DateTime? get executionStartedUtc => null;
  DateTime? get executionFinishedUtc => null;
  int? get hostRevisionAtExecution => null;
}

final class JobMetricOpenTarget {
  const JobMetricOpenTarget._(this.jobId, this.projectId, this.revisionDigest);
  final String jobId, projectId, revisionDigest;
}

final class JobMetricReadTicket {
  const JobMetricReadTicket._(this.context);
  final JobCorrelationContext context;
}

final class JobMetricSnapshot {
  const JobMetricSnapshot._(this.context, this.markers, this.truncated, this.failure);
  final JobCorrelationContext? context;
  final List<JobMetricMarker> markers;
  final bool truncated;
  final JobCorrelationFailure? failure;
}

final class JobMetricCorrelation {
  static const maxCatalogJobs = 2048, maxMarkers = 128, maxTotalTargets = 20000;
  JobCorrelationContext? _context;
  JobMetricReadTicket? _pending;
  List<JobMetricMarker> _markers = const [];
  bool _truncated = false, _disposed = false;
  JobCorrelationFailure? _failure;
  final _revisions = <String, (int, JobMetricState)>{};
  JobMetricSnapshot get snapshot => JobMetricSnapshot._(_context, _markers, _truncated, _failure);
  void bind(JobCorrelationContext? context) {
    if (_disposed || identical(context, _context)) return;
    _context = context;
    _pending = null;
    _markers = const [];
    _failure = null;
    _truncated = false;
    _revisions.clear();
  }

  JobMetricReadTicket? begin(JobCorrelationContext original) {
    if (_disposed || !identical(original, _context)) return null;
    return _pending = JobMetricReadTicket._(original);
  }

  bool complete(JobMetricReadTicket ticket, Object? rawCatalog, DateTime observedUtc) {
    if (_disposed || !identical(ticket, _pending) || !identical(ticket.context, _context)) {
      return false;
    }
    _pending = null;
    try {
      if (!observedUtc.isUtc) throw JobCorrelationFailure.invalidMetadata;
      final parsed = _project(ticket.context, rawCatalog, observedUtc);
      for (final marker in parsed) {
        final previous = _revisions[marker.jobId];
        if (previous != null &&
            (marker.notificationRevision < previous.$1 ||
                (marker.notificationRevision > 0 &&
                    marker.notificationRevision == previous.$1 &&
                    marker.state != previous.$2))) {
          throw JobCorrelationFailure.invalidMetadata;
        }
      }
      final newIds = parsed.where((marker) => !_revisions.containsKey(marker.jobId)).length;
      if (_revisions.length + newIds > maxCatalogJobs) {
        throw JobCorrelationFailure.budget;
      }
      for (final marker in parsed) {
        _revisions[marker.jobId] = (marker.notificationRevision, marker.state);
      }
      _truncated = parsed.length > maxMarkers;
      _markers = List.unmodifiable(parsed.take(maxMarkers));
      _failure = null;
    } on JobCorrelationFailure catch (reason) {
      _markers = const [];
      _truncated = false;
      _failure = reason;
    }
    return true;
  }

  JobMetricOpenTarget? navigation(JobMetricSnapshot captured, JobMetricMarker marker) =>
      !_disposed &&
          identical(captured.context, _context) &&
          _context != null &&
          _markers.any((m) => identical(m, marker))
      ? JobMetricOpenTarget._(marker.jobId, marker.projectId, marker.revisionDigest)
      : null;
  void dispose() {
    _disposed = true;
    _context = null;
    _pending = null;
    _markers = const [];
    _failure = null;
    _truncated = false;
    _revisions.clear();
  }
}

DateTime? _clientTime(Object? value) {
  if (value is! int || value < 0 || value > 253402300799) return null;
  return DateTime.fromMillisecondsSinceEpoch(value * 1000, isUtc: true);
}

DateTime? _runnerTime(Object? value) {
  if (value is! String || value.length > 32) return null;
  final m = RegExp(r'^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.\d{1,6})?Z$').firstMatch(value);
  final time = DateTime.tryParse(value);
  if (m == null || m.end != value.length || time == null || !time.isUtc) {
    return null;
  }
  final parts = [time.year, time.month, time.day, time.hour, time.minute, time.second];
  for (var i = 0; i < parts.length; i++) {
    if (parts[i] != int.parse(m[i + 1]!)) return null;
  }
  return time;
}

({JobProvenanceStatus status, JobMetricProvenance? value}) _provenance(Map<dynamic, dynamic> row, Map<dynamic, dynamic> manifest) {
  if (!row.containsKey('correlation_provenance')) return (status: JobProvenanceStatus.legacyUnknown, value: null);
  final p = row['correlation_provenance'];
  const fields = {
    'version',
    'jobId',
    'profileId',
    'deviceId',
    'projectId',
    'projectRevision',
    'runnerId',
    'revisionObjectId',
    'revisionDigest',
  };
  if (p is! Map ||
      p.length != fields.length ||
      !p.keys.every(fields.contains) ||
      p['version'] is! int ||
      p['version'] != 1 ||
      !_id(p['jobId']) ||
      !_id(p['profileId']) ||
      !_id(p['deviceId']) ||
      !_id(p['projectId']) ||
      !_id(p['runnerId']) ||
      !_id(p['revisionObjectId']) ||
      !_revision(p['projectRevision']) ||
      !_sha(p['revisionDigest']) ||
      p['jobId'] != row['job_id'] ||
      p['jobId'] != manifest['job_id'] ||
      p['profileId'] != manifest['tenant_id'] ||
      p['projectId'] != row['project_id'] ||
      p['projectId'] != manifest['project_id'] ||
      p['runnerId'] != row['runner_id'] ||
      p['runnerId'] != manifest['runner_id'] ||
      p['revisionDigest'] != manifest['revision_digest']) {
    return (status: JobProvenanceStatus.unavailable, value: null);
  }
  return (
    status: JobProvenanceStatus.available,
    value: JobMetricProvenance._(
      jobId: p['jobId'] as String,
      profileId: p['profileId'] as String,
      deviceId: p['deviceId'] as String,
      projectId: p['projectId'] as String,
      projectRevision: p['projectRevision'] as int,
      runnerId: p['runnerId'] as String,
      revisionObjectId: p['revisionObjectId'] as String,
      revisionDigest: p['revisionDigest'] as String,
    ),
  );
}

/// Exactly the one known uncertainty wrapper is supported. Never recursively
/// hunt arbitrary payload/output for a plausible state or process receipt.
Map<dynamic, dynamic>? _runnerState(Map<dynamic, dynamic> row) {
  var raw = row['runner_result'];
  if (raw is Map &&
      raw.length == 2 &&
      raw['reason'] == 'native_operation_not_acknowledged' &&
      raw.containsKey('runner_result')) {
    raw = raw['runner_result'];
  }
  if (raw is! Map ||
      raw['version'] is! int ||
      raw['version'] != 1 ||
      raw['job_id'] != row['job_id'] ||
      !_sha(row['manifest_digest']) ||
      raw['manifest_digest'] != row['manifest_digest']) {
    return null;
  }
  return raw;
}

({JobRunnerObservationStatus status, JobRunnerObservation? value}) _runnerObservation(
  Map<dynamic, dynamic> row,
  Map<dynamic, dynamic>? state,
  JobClockAlignment alignment,
) {
  if (state == null) {
    return (
      status: row['runner_result'] == null
          ? JobRunnerObservationStatus.legacyUnknown
          : JobRunnerObservationStatus.unavailable,
      value: null,
    );
  }
  final result = state['result'];
  if (result == null || result is Map && !result.containsKey('runner_execution')) {
    return (status: JobRunnerObservationStatus.legacyUnknown, value: null);
  }
  final p = result is Map ? result['runner_execution'] : null;
  const fields = {'version', 'clockSource', 'scope', 'spawnObservedUtc', 'exitObservedUtc', 'elapsedMonotonicMs'};
  if (p is! Map ||
      p.length != fields.length ||
      !p.keys.every(fields.contains) ||
      p['version'] is! int ||
      p['version'] != 1 ||
      p['clockSource'] != 'runner-wall' ||
      p['scope'] != 'runner-process' ||
      p['elapsedMonotonicMs'] is! int ||
      (p['elapsedMonotonicMs'] as int) < 0 ||
      (p['elapsedMonotonicMs'] as int) > 9007199254740991) {
    return (status: JobRunnerObservationStatus.unavailable, value: null);
  }
  final spawn = _runnerTime(p['spawnObservedUtc']), exit = _runnerTime(p['exitObservedUtc']);
  if (spawn == null || exit == null || spawn.year < 1 || exit.year < 1) {
    return (status: JobRunnerObservationStatus.unavailable, value: null);
  }
  final aligned = !exit.isBefore(spawn) && alignment.appliesTo(spawn) && alignment.appliesTo(exit);
  return (
    status: JobRunnerObservationStatus.available,
    value: JobRunnerObservation._(spawn, exit, p['elapsedMonotonicMs'] as int, aligned),
  );
}

List<JobMetricMarker> _project(JobCorrelationContext c, Object? raw, DateTime observed) {
  if (raw is! Map || raw['schema_version'] is! int || raw['schema_version'] != 1 || raw['read_only'] != false) {
    throw JobCorrelationFailure.unsupportedSchema;
  }
  final jobs = raw['jobs'];
  if (jobs is! List) throw JobCorrelationFailure.invalidMetadata;
  if (jobs.length > JobMetricCorrelation.maxCatalogJobs) {
    throw JobCorrelationFailure.budget;
  }
  final ids = <String>{}, result = <JobMetricMarker>[];
  var targetCount = 0;
  for (final row in jobs) {
    if (row is! Map || row['version'] is! int || row['version'] != 1) {
      throw JobCorrelationFailure.unsupportedSchema;
    }
    final id = row['job_id'], manifest = row['manifest'], hostIds = row['host_ids'];
    final revision = row.containsKey('notification_revision') ? row['notification_revision'] : 0;
    if (!_id(id) ||
        !_revision(revision, legacy: true) ||
        manifest is! Map ||
        manifest['version'] is! int ||
        manifest['version'] != 1 ||
        manifest['job_id'] != id ||
        manifest['tenant_id'] != c.scope.profileId ||
        !_id(manifest['project_id']) ||
        row['project_id'] != manifest['project_id'] ||
        !_sha(manifest['revision_digest']) ||
        hostIds is! List ||
        hostIds.isEmpty ||
        hostIds.length > 10000) {
      throw JobCorrelationFailure.invalidMetadata;
    }
    if (!ids.add(id as String)) throw JobCorrelationFailure.duplicateJob;
    final targets = manifest['targets'];
    if (targets is! List ||
        targets.length != hostIds.length ||
        (targetCount += targets.length) > JobMetricCorrelation.maxTotalTargets) {
      throw JobCorrelationFailure.budget;
    }
    final approved = <String>{}, declared = <String>{};
    for (final host in hostIds) {
      if (!_id(host) || !declared.add(host as String)) {
        throw JobCorrelationFailure.invalidMetadata;
      }
    }
    for (final target in targets) {
      if (target is! Map || !_id(target['host_id']) || !approved.add(target['host_id'] as String)) {
        throw JobCorrelationFailure.invalidMetadata;
      }
    }
    if (approved.length != declared.length || !approved.containsAll(declared)) {
      throw JobCorrelationFailure.invalidMetadata;
    }
    if (!approved.contains(c.hostId)) continue;
    final state = JobMetricState.values.where((s) => s.name == row['state']).firstOrNull ?? JobMetricState.unknown;
    final admissionUtc = _clientTime(row['accepted_at']), updatedUtc = _clientTime(row['updated_at']);
    JobMetricStamp? runnerQueued, runnerUpdated;
    final runner = _runnerState(row);
    if (runner != null) {
      final queued = _runnerTime(runner['accepted_at']), latest = _runnerTime(runner['updated_at']);
      if (queued != null) runnerQueued = JobMetricStamp(queued, JobClockOrigin.runner, JobTimeMeaning.queuedOnRunner);
      if (latest != null) {
        runnerUpdated = JobMetricStamp(latest, JobClockOrigin.runner, JobTimeMeaning.latestRunnerState);
      }
    }
    final provenance = _provenance(row, manifest);
    final process = _runnerObservation(row, runner, c.runnerClock);
    JobMarkerPosition? position;
    JobMarkerUnpositioned? reason;
    if (admissionUtc == null) {
      reason = JobMarkerUnpositioned.missingAdmissionTime;
    } else if (!c.clientClock.appliesTo(admissionUtc)) {
      reason = JobMarkerUnpositioned.unknownClockSkew;
    } else {
      final aligned = admissionUtc.subtract(c.clientClock.offset!), uncertainty = c.clientClock.uncertainty!;
      final low = aligned.subtract(uncertainty), high = aligned.add(uncertainty);
      if (high.isBefore(c.startUtc) || low.isAfter(c.endUtc)) {
        reason = JobMarkerUnpositioned.outsidePeriod;
      } else {
        position = JobMarkerPosition(
          low.isBefore(c.startUtc) ? c.startUtc : low,
          high.isAfter(c.endUtc) ? c.endUtc : high,
          low.isBefore(c.startUtc) || high.isAfter(c.endUtc),
          c.clientClock.offset!,
          uncertainty,
        );
      }
    }
    result.add(
      JobMetricMarker._(
        jobId: id,
        projectId: manifest['project_id'] as String,
        revisionDigest: manifest['revision_digest'] as String,
        notificationRevision: revision as int,
        state: state,
        admitted: admissionUtc == null
            ? null
            : JobMetricStamp(admissionUtc, JobClockOrigin.nativeClient, JobTimeMeaning.admitted),
        updated: updatedUtc == null
            ? null
            : JobMetricStamp(updatedUtc, JobClockOrigin.nativeClient, JobTimeMeaning.latestStateUpdate),
        runnerQueued: runnerQueued,
        runnerUpdated: runnerUpdated,
        received: JobMetricStamp(observed, JobClockOrigin.observedClient, JobTimeMeaning.received),
        position: position,
        unpositioned: reason,
        provenanceStatus: provenance.status,
        provenance: provenance.value,
        runnerObservationStatus: process.status,
        runnerObservation: process.value,
      ),
    );
  }
  result.sort((a, b) {
    final at = a.admitted?.utc, bt = b.admitted?.utc;
    final compared = at == null
        ? (bt == null ? 0 : 1)
        : bt == null
        ? -1
        : bt.compareTo(at);
    return compared == 0 ? a.jobId.compareTo(b.jobId) : compared;
  });
  return result;
}
