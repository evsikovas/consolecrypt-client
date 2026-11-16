import 'package:consolecrypt/monitoring/job_context_navigation.dart';
import 'package:consolecrypt/monitoring/job_metric_correlation.dart';

enum JobMetricTimelineFailure { changed, readFailed, invalidCatalog }

/// Original explicit selection only. No route, endpoint, credential or query.
final class JobMetricTimelineSelection {
  JobMetricTimelineSelection({
    required this.scope,
    required this.hostId,
    required this.sourceId,
    required this.sourceRevision,
    required this.bindingId,
    required this.bindingRevision,
    required this.startUtc,
    required this.endUtc,
  }) {
    final uuid = RegExp(r'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$');
    for (final id in [hostId, sourceId, bindingId]) {
      if (id.length != 36 || id == '00000000-0000-0000-0000-000000000000' || !uuid.hasMatch(id)) {
        throw ArgumentError('selection');
      }
    }
    if (sourceRevision <= 0 ||
        sourceRevision > 9007199254740991 ||
        bindingRevision <= 0 ||
        bindingRevision > 9007199254740991 ||
        !startUtc.isUtc ||
        !endUtc.isUtc ||
        !startUtc.isBefore(endUtc)) {
      throw ArgumentError('selection');
    }
  }
  final JobCorrelationScope scope;
  final String hostId, sourceId, bindingId;
  final int sourceRevision, bindingRevision;
  final DateTime startUtc, endUtc;
}

final class JobMetricTimelineSnapshot {
  const JobMetricTimelineSnapshot._(this.selection, this.metadata, this.jobs);
  final JobMetricTimelineSelection selection;
  final JobContextMetadata metadata;
  final JobMetricSnapshot jobs;
}

/// Read-only captured selection for COR02. The receiver still checks the actual
/// current project and the ORIGINAL Native context before accepting navigation.
final class JobMetricTimelineChoice {
  const JobMetricTimelineChoice._(this.snapshot, this.marker);
  final JobMetricTimelineSnapshot snapshot;
  final JobMetricMarker marker;
}

/// Dialog/workspace-owned explicit read, with no poller or second service owner.
/// Callback adapters use the existing metadata reader and jobs() method ONLY.
final class JobMetricTimelineFlow {
  JobMetricTimelineFlow({
    required this.selection,
    required this._capture,
    required this._recheck,
    required this._readJobs,
    required this._isCurrent,
    required this._nowUtc,
  });

  final JobMetricTimelineSelection selection;
  final Future<JobContextMetadata> Function() _capture;
  final Future<JobContextMetadata> Function(JobContextMetadata) _recheck;
  final Future<Object?> Function() _readJobs;
  final bool Function() _isCurrent;
  final DateTime Function() _nowUtc;
  final _projection = JobMetricCorrelation();
  JobContextMetadata? _original;
  JobCorrelationContext? _context;
  JobMetricTimelineSnapshot? _snapshot;
  JobMetricTimelineFailure? _failure;
  int _generation = 0;
  bool _disposed = false, _busy = false;
  bool get busy => _busy;
  JobMetricTimelineFailure? get failure => _failure;
  JobMetricTimelineSnapshot? get snapshot => _live ? _snapshot : null;
  bool get _live => !_disposed && _isCurrent();
  bool _current(int generation) => _live && generation == _generation;

  Future<bool> load() async {
    if (!_live) return false;
    final generation = ++_generation;
    _busy = true;
    _snapshot = null;
    _failure = null;
    try {
      final captured = await _capture();
      if (!_current(generation)) return false;
      if (!_matches(captured) || (_original != null && !_same(_original!, captured))) {
        _failure = JobMetricTimelineFailure.changed;
        return false;
      }
      _original ??= captured;
      final catalog = await _readJobs();
      if (!_current(generation)) return false;
      final checked = await _recheck(captured);
      if (!_current(generation)) return false;
      if (!_same(captured, checked)) {
        _failure = JobMetricTimelineFailure.changed;
        return false;
      }
      // Reuse the original context across reloads, preserving revision
      // tombstones; a current Native host revision is never a UI timestamp.
      final context = _context ??= JobCorrelationContext(
        scope: selection.scope,
        hostId: captured.hostId,
        hostRevision: captured.hostRevision,
        sourceId: captured.sourceId,
        sourceRevision: captured.sourceRevision,
        bindingId: captured.bindingId,
        bindingRevision: captured.bindingRevision,
        bindingHostId: selection.hostId,
        bindingSourceId: selection.sourceId,
        bindingSourceRevision: selection.sourceRevision,
        bindingState: JobCorrelationBindingState.current,
        startUtc: selection.startUtc,
        endUtc: selection.endUtc,
      );
      _projection.bind(context);
      final ticket = _projection.begin(context)!;
      if (!_projection.complete(ticket, catalog, _nowUtc()) || _projection.snapshot.failure != null) {
        _failure = JobMetricTimelineFailure.invalidCatalog;
        return false;
      }
      _snapshot = JobMetricTimelineSnapshot._(selection, captured, _projection.snapshot);
      return true;
    } catch (_) {
      if (_current(generation)) _failure = JobMetricTimelineFailure.readFailed;
      return false;
    } finally {
      if (generation == _generation) _busy = false;
    }
  }

  JobMetricTimelineChoice? choose(JobMetricTimelineSnapshot original, JobMetricMarker marker) {
    if (!_live ||
        _busy ||
        !identical(_snapshot, original) ||
        _projection.navigation(original.jobs, marker) == null ||
        marker.notificationRevision == 0) {
      return null;
    }
    return JobMetricTimelineChoice._(original, marker);
  }

  /// Hide synchronously on any original owner/selection boundary. Cancellation
  /// stays local; no global service cancel can affect another workspace child.
  void invalidate() {
    _generation++;
    _busy = false;
    _snapshot = null;
    _failure = null;
  }

  void dispose() {
    invalidate();
    _disposed = true;
    _projection.dispose();
  }

  bool _matches(JobContextMetadata m) =>
      m.profileId == selection.scope.profileId &&
      m.protocol == JobContextHostProtocol.ssh &&
      m.hostId == selection.hostId &&
      m.sourceId == selection.sourceId &&
      m.sourceRevision == selection.sourceRevision &&
      m.bindingId == selection.bindingId &&
      m.bindingRevision == selection.bindingRevision;
}

bool _same(JobContextMetadata a, JobContextMetadata b) =>
    a.profileId == b.profileId &&
    a.notificationScopeId == b.notificationScopeId &&
    a.hostId == b.hostId &&
    a.hostRevision == b.hostRevision &&
    a.protocol == b.protocol &&
    a.sourceId == b.sourceId &&
    a.sourceRevision == b.sourceRevision &&
    a.bindingId == b.bindingId &&
    a.bindingRevision == b.bindingRevision;
