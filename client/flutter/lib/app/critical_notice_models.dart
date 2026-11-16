import 'package:consolecrypt/core/services/job_notice_events.dart';

enum CriticalNoticeSeverity { info, success, warning, danger }

final class CriticalJobNotice {
  const CriticalJobNotice({required this.change, required this.observedUtc, this.unread = true});
  final JobNoticeChange change;
  final DateTime observedUtc;
  final bool unread;
  String get id => 'job:${change.jobId}';
  CriticalNoticeSeverity get severity => switch (change.state) {
    JobNoticeState.succeeded => CriticalNoticeSeverity.success,
    JobNoticeState.failed || JobNoticeState.unknown => CriticalNoticeSeverity.danger,
    JobNoticeState.cancelled || JobNoticeState.interrupted => CriticalNoticeSeverity.warning,
    _ => CriticalNoticeSeverity.info,
  };
  CriticalJobNotice read() => CriticalJobNotice(change: change, observedUtc: observedUtc, unread: false);
}

/// Pure profile-memory projection. It cannot fetch, execute, cancel or retry.
/// Per-record revisions remain bounded by Native's2048-record catalog limit;
/// unlike an evicted tombstone, they prevent a late old event becoming "new".
final class CriticalJobNoticeRing {
  CriticalJobNoticeRing({required this.scope, required this.profileId});
  final Object scope;
  final String profileId;
  static const maxRows = 64, maxRecords = 2048;
  final _records = <String, JobNoticeChange>{};
  final _rows = <String, CriticalJobNotice>{};
  int? _epoch;
  int _gapVersion = 0;
  bool _blocked = false, _disposed = false, refreshNeeded = false, feedUnavailable = false;
  int? get epoch => _epoch;
  int get gapVersion => _gapVersion;
  int get rememberedRecords => _records.length;
  bool get accepting => !_disposed && !_blocked;
  List<CriticalJobNotice> get rows => List.unmodifiable(_rows.values.toList().reversed);
  int get unread => _rows.values.where((row) => row.unread).length;
  bool current(Object capturedScope) => !_disposed && identical(scope, capturedScope);

  void ingest(Object capturedScope, JobNoticeFrame frame, DateTime observed) {
    if (!current(capturedScope) || _epoch != null && frame.epoch < _epoch!) return;
    if (frame.reset) {
      _epoch = frame.epoch;
      _blocked = frame.profileId != profileId;
      _records.clear();
      _rows.clear();
      refreshNeeded = frame.refreshNeeded;
      feedUnavailable = frame.streamUnavailable;
      _gapVersion++;
      return;
    }
    if (_blocked || frame.profileId != profileId) return;
    _epoch ??= frame.epoch;
    if (frame.epoch != _epoch) {
      // Missing a boundary is an observation gap. Do not adopt state from a
      // new epoch until the explicit reset/current profile is established.
      _gap();
      return;
    }
    if (frame.streamUnavailable) feedUnavailable = true;
    if (frame.refreshNeeded) _gap();
    if (frame.change case final change?) _accept(change, observed, fromRefresh: false);
  }

  void _gap() {
    refreshNeeded = true;
    _gapVersion++;
  }

  void _accept(JobNoticeChange change, DateTime observed, {required bool fromRefresh}) {
    if (change.profileId != profileId) return;
    final old = _records[change.jobId];
    if (old != null) {
      if (change.revision < old.revision) return;
      if (change.revision == old.revision) {
        if (change.state != old.state) _gap();
        return;
      }
    } else if (_records.length >= maxRecords) {
      _gap();
      return;
    }
    _records[change.jobId] = change;
    final key = 'job:${change.jobId}', existing = _rows[key];
    final terminal = const {
      JobNoticeState.succeeded,
      JobNoticeState.failed,
      JobNoticeState.cancelled,
      JobNoticeState.interrupted,
      JobNoticeState.unknown,
    }.contains(change.state);
    if (!terminal && existing == null) return;
    _rows.remove(key);
    _rows[key] = CriticalJobNotice(
      change: change,
      observedUtc: observed.toUtc(),
      unread: change.state == JobNoticeState.acknowledged
          ? false
          : fromRefresh
          ? existing?.unread ?? false
          : true,
    );
    while (_rows.length > maxRows) {
      _rows.remove(_rows.keys.first);
    }
  }

  /// One explicit existing jobs() result, under its ORIGINAL scope/epoch/gap
  /// generation. No polling, authority or inferred completion timestamp.
  bool reconcile(
    Object capturedScope, {
    required int? originalEpoch,
    required int originalGapVersion,
    required List<JobNoticeChange> changes,
    required DateTime observed,
  }) {
    if (!current(capturedScope) || _blocked || originalEpoch != _epoch || changes.length > maxRecords) return false;
    for (final change in changes) {
      if (change.profileId != profileId) return false;
    }
    for (final change in changes) {
      _accept(change, observed, fromRefresh: true);
    }
    if (originalGapVersion == _gapVersion && !feedUnavailable) refreshNeeded = false;
    return true;
  }

  void markRead(Object capturedScope, String id) {
    if (!current(capturedScope)) return;
    if (_rows[id] case final row?) _rows[id] = row.read();
  }

  void dismiss(Object capturedScope, String id) {
    if (current(capturedScope)) _rows.remove(id);
  }

  void clearRows(Object capturedScope) {
    if (current(capturedScope)) _rows.clear();
  }

  void dispose() {
    _disposed = true;
    _records.clear();
    _rows.clear();
    refreshNeeded = false;
  }
}
