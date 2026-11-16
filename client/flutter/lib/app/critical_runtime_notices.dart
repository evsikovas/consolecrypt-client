import 'package:consolecrypt/core/services/runtime_critical_notice_ring.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';

/// A UI observation, never a command destination or server completion time.
final class CriticalRuntimeNotice {
  const CriticalRuntimeNotice._(this.value, this.observedUtc, this._captured);
  final RuntimeCriticalNoticeRow value;
  final DateTime observedUtc;
  final RuntimeCriticalNoticeSnapshot _captured;
  String get id => 'runtime:${value.domain.name}:${value.recordId}';
  bool get unread => !value.isRead;
}

/// Adds a receipt clock to the bounded pure ring. The original profile identity
/// is fixed by the common owner; a different Native profile fences its rows.
/// No metadata lookup, polling, service factory, persistence or mutation exists.
final class CriticalRuntimeNotices {
  CriticalRuntimeNotices(this.profileId);
  final String profileId;
  final _ring = RuntimeCriticalNoticeRing();
  final _observed = <String, (int, DateTime)>{};
  RuntimeNoticeScope? _retainedScope;
  bool _fenced = false;
  int _fenceEpoch = -1;
  RuntimeCriticalNoticeSnapshot get snapshot => _ring.snapshot;
  List<CriticalRuntimeNotice> get rows {
    final captured = snapshot;
    return [
      for (final row in (_fenced ? <RuntimeCriticalNoticeRow>[] : captured.rows))
        if (_observed[_id(row)] case final observation?) CriticalRuntimeNotice._(row, observation.$2, captured),
    ];
  }

  static String _id(RuntimeCriticalNoticeRow row) => 'runtime:${row.domain.name}:${row.recordId}';

  void ingest(RuntimeNoticeFrame frame, DateTime observedUtc) {
    final scope = frame.scope;
    if (scope != null && scope.profileId != profileId) {
      if (frame.reset) {
        _ring.apply(
          RuntimeNoticeFrame(
            epoch: frame.epoch,
            reset: true,
            incomplete: frame.incomplete,
            streamUnavailable: frame.streamUnavailable,
          ),
        );
      }
      return;
    }
    if (!_ring.apply(frame)) return;
    if (frame.reset && frame.epoch > _fenceEpoch) _fenced = false;
    final current = snapshot;
    if (current.scope == null) return;
    if (!current.scope!.same(_retainedScope)) {
      _observed.clear();
      _retainedScope = current.scope;
    }
    final visible = <String>{};
    for (final row in current.rows) {
      final id = _id(row);
      visible.add(id);
      if (_observed[id]?.$1 != row.revision) {
        _observed[id] = (row.revision, observedUtc.toUtc());
      }
    }
    _observed.removeWhere((key, _) => !visible.contains(key));
  }

  /// The common owner's other event stream saw a session boundary first.
  /// Hide immediately without inventing a Native runtime epoch or recovery.
  void fence() {
    _fenced = true;
    // Remember an observed epoch, never manufacture a future Native epoch.
    // A delayed same-epoch reset from the other broadcast queue cannot unhide
    // the old owner after this boundary was already seen by the common owner.
    _fenceEpoch = snapshot.epoch;
  }

  bool isCurrent(CriticalRuntimeNotice notice) {
    final current = snapshot;
    return !_fenced &&
        !current.disposed &&
        identical(current.scope, notice._captured.scope) &&
        current.epoch == notice._captured.epoch &&
        current.rows.any((row) => identical(row, notice.value));
  }

  void markRead(CriticalRuntimeNotice notice) {
    if (isCurrent(notice)) _ring.markRead(notice._captured, notice.value);
  }

  void dismiss(CriticalRuntimeNotice notice) {
    if (isCurrent(notice) && _ring.dismiss(notice._captured, notice.value)) {
      _observed.remove(notice.id);
    }
  }

  void clear() {
    if (_fenced) return;
    _ring.clear(snapshot);
    _observed.clear();
  }

  void dispose() {
    _fenced = true;
    _ring.dispose();
    _observed.clear();
    _retainedScope = null;
  }
}
