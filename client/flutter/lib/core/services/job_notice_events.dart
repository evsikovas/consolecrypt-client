import 'dart:async';

enum JobNoticeState {
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

final class JobNoticeChange {
  const JobNoticeChange({required this.profileId, required this.jobId, required this.revision, required this.state});
  final String profileId, jobId;
  final int revision;
  final JobNoticeState state;
  static JobNoticeChange? parse(Map<String, dynamic> value, {bool legacyRecord = false}) {
    if (value.length != 5 || value['type'] != 'automation_job_changed') return null;
    final profile = value['profile_id'], job = value['job_id'], revision = value['revision'], state = value['state'];
    final uuid = RegExp(r'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$');
    const nil = '00000000-0000-0000-0000-000000000000';
    if (profile is! String ||
        job is! String ||
        !uuid.hasMatch(profile) ||
        !uuid.hasMatch(job) ||
        profile == nil ||
        job == nil ||
        revision is! int ||
        revision < (legacyRecord ? 0 : 1) ||
        revision > 9007199254740991 ||
        state is! String) {
      return null;
    }
    final parsed = JobNoticeState.values.where((candidate) => candidate.name == state).firstOrNull;
    return parsed == null ? null : JobNoticeChange(profileId: profile, jobId: job, revision: revision, state: parsed);
  }

  /// Explicit reconciliation reads the existing Native jobs() catalog. Strip
  /// every journal/manifest field immediately; retain only this DTO's metadata.
  static List<JobNoticeChange>? catalog(String profileId, Object? payload) {
    if (payload is! Map ||
        payload['schema_version'] is! int ||
        payload['schema_version'] != 1 ||
        payload['read_only'] != false) {
      return null;
    }
    final rows = payload['jobs'];
    if (rows is! List || rows.length > 2048) return null;
    final result = <JobNoticeChange>[], ids = <String>{};
    for (final row in rows) {
      if (row is! Map || row['version'] != 1) return null;
      final parsed = parse({
        'type': 'automation_job_changed',
        'profile_id': profileId,
        'job_id': row['job_id'],
        'revision': row.containsKey('notification_revision') ? row['notification_revision'] : 0,
        'state': row['state'],
      }, legacyRecord: true);
      if (parsed == null || !ids.add(parsed.jobId)) return null;
      result.add(parsed);
    }
    return List.unmodifiable(result);
  }
}

/// Metadata from the SINGLE existing core_events subscription. `reset` clears
/// all profile-owned notices; `refreshNeeded` never fabricates a job state.
final class JobNoticeFrame {
  const JobNoticeFrame({
    required this.epoch,
    this.profileId,
    this.change,
    this.reset = false,
    this.refreshNeeded = false,
    this.streamUnavailable = false,
  });
  final int epoch;
  final String? profileId;
  final JobNoticeChange? change;
  final bool reset, refreshNeeded, streamUnavailable;
}

abstract interface class JobNoticeEvents {
  JobNoticeFrame get currentScope;
  Stream<JobNoticeFrame> watchJobNotices();
}

/// Pure adapter owned by RustBackend. No service/timer/Native API is available.
/// The hub calls boundary() synchronously BEFORE awaiting lock/profile refresh;
/// only an original-epoch refresh may establish a fresh unlocked scope.
final class JobNoticeEventAdapter implements JobNoticeEvents {
  final _frames = StreamController<JobNoticeFrame>.broadcast();
  int _epoch = 0;
  String? _profile;
  bool _disposed = false, _streamUnavailable = false, _refreshNeeded = false;
  int get epoch => _epoch;
  @override
  JobNoticeFrame get currentScope => JobNoticeFrame(
    epoch: _epoch,
    profileId: _profile,
    reset: true,
    refreshNeeded: _refreshNeeded,
    streamUnavailable: _streamUnavailable,
  );
  @override
  Stream<JobNoticeFrame> watchJobNotices() => _frames.stream;

  void boundary() {
    if (_disposed) return;
    _epoch++;
    _profile = null;
    _refreshNeeded = _streamUnavailable;
    _frames.add(currentScope);
  }

  /// A global feed loss may have hidden an unlock boundary. Clear the old
  /// observer atomically and preserve the explicit reconciliation requirement.
  void retireGlobalStream({bool closed = false}) {
    if (_disposed) return;
    _streamUnavailable |= closed;
    _refreshNeeded = true;
    _epoch++;
    _profile = null;
    _frames.add(currentScope);
  }

  void acceptScope({required int originalEpoch, required String? profileId, required bool unlocked}) {
    if (_disposed || originalEpoch != _epoch) return;
    final next = unlocked ? profileId : null;
    if (_profile == next) return;
    // A direct profile operation may refresh state before its core event is
    // delivered. Clear first even on that path; never carry a ring across it.
    _epoch++;
    _profile = next;
    _frames.add(currentScope);
  }

  void streamLost({bool closed = false}) {
    if (_disposed) return;
    _streamUnavailable = _streamUnavailable || closed;
    _refreshNeeded = true;
    if (_profile == null) return;
    _frames.add(
      JobNoticeFrame(epoch: _epoch, profileId: _profile, refreshNeeded: true, streamUnavailable: _streamUnavailable),
    );
  }

  void ingest(Map<String, dynamic> value) {
    if (_disposed || _profile == null) return;
    if (value['type'] == 'lagged') {
      _refreshNeeded = true;
      _frames.add(JobNoticeFrame(epoch: _epoch, profileId: _profile, refreshNeeded: true));
      return;
    }
    if (value['type'] != 'automation_job_changed') return;
    // Late other-profile events are normal during shutdown, not a reason to
    // refresh the current profile or retain the old payload.
    if (value['profile_id'] != _profile) return;
    final change = JobNoticeChange.parse(value);
    if (change == null) _refreshNeeded = true;
    _frames.add(JobNoticeFrame(epoch: _epoch, profileId: _profile, change: change, refreshNeeded: change == null));
  }

  Future<void> dispose() async {
    if (_disposed) return;
    boundary();
    _disposed = true;
    await _frames.close();
  }
}
