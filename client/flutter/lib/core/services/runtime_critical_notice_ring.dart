// Pure bounded presentation state for the existing Native metadata observer.
import 'package:consolecrypt/core/services/runtime_notice_events.dart';

enum RuntimeCriticalNoticeSeverity { info, warning, error }

final class RuntimeCriticalNoticeRow {
  const RuntimeCriticalNoticeRow._(this.domain, this.recordId, this.revision, this.state, this.severity, this.isRead);
  final RuntimeNoticeDomain domain;
  final String recordId;
  final int revision;
  final RuntimeNoticeState state;
  final RuntimeCriticalNoticeSeverity severity;
  final bool isRead;

  RuntimeCriticalNoticeRow _read() => RuntimeCriticalNoticeRow._(domain, recordId, revision, state, severity, true);
}

/// Observed metadata only. A false incomplete flag never proves complete
/// historical coverage, remote recovery, command success or delivery.
final class RuntimeCriticalNoticeSnapshot {
  const RuntimeCriticalNoticeSnapshot._(
    this._ring,
    this.epoch,
    this.scope,
    this.rows,
    this.incomplete,
    this.unsupported,
    this.feedUnavailable,
    this.disposed,
  );
  final Object _ring;
  final int epoch;
  final RuntimeNoticeScope? scope;
  final List<RuntimeCriticalNoticeRow> rows;
  final bool incomplete, unsupported, feedUnavailable, disposed;
}

final class _Key {
  const _Key(this.domain, this.record);
  final RuntimeNoticeDomain domain;
  final String record;
  @override
  bool operator ==(Object other) => other is _Key && domain == other.domain && record == other.record;
  @override
  int get hashCode => Object.hash(domain, record);
}

final class _Revision {
  const _Revision(this.revision, this.state);
  final int revision;
  final RuntimeNoticeState state;
}

/// No subscription, clock, timer, RPC, filesystem, resolver or executable
/// destination. Only explicit reset frames may establish a captured scope.
/// A fence hides rows synchronously; the same Native nonce retains its bounded
/// rows/revision fences/loss. A genuinely new owner discards that old metadata.
final class RuntimeCriticalNoticeRing {
  static const maxRows = 64;
  static const maxTombstones = 2048;
  static const _maxEpoch = 9007199254740991;
  final Object _identity = Object();
  final Map<_Key, _Revision> _revisions = {};
  final Map<_Key, RuntimeCriticalNoticeRow> _rows = {};
  RuntimeNoticeScope? _scope, _retainedScope;
  int _epoch = 0;
  bool _incomplete = false, _unsupported = false, _feedUnavailable = false, _disposed = false;

  RuntimeCriticalNoticeSnapshot get snapshot => RuntimeCriticalNoticeSnapshot._(
    _identity,
    _epoch,
    _scope,
    List<RuntimeCriticalNoticeRow>.unmodifiable(
      _scope == null ? <RuntimeCriticalNoticeRow>[] : _rows.values.toList().reversed,
    ),
    _incomplete,
    _unsupported,
    _feedUnavailable,
    _disposed,
  );

  bool apply(RuntimeNoticeFrame frame) {
    if (_disposed || frame.epoch < 0 || frame.epoch > _maxEpoch) return false;
    if (frame.reset) return _reset(frame);
    final scope = _scope;
    if (scope == null || frame.epoch != _epoch || !identical(frame.scope, scope)) {
      return false;
    }
    final change = frame.change;
    if (change != null && (change.profileId != scope.profileId || change.scopeId != scope.nonce)) return false;
    final priorIncomplete = _incomplete;
    final priorUnavailable = _feedUnavailable;
    _incomplete |= frame.incomplete;
    _feedUnavailable |= frame.streamUnavailable;
    if (_feedUnavailable || change == null) {
      return priorIncomplete != _incomplete || priorUnavailable != _feedUnavailable;
    }
    final key = _Key(change.domain, change.recordId);
    final old = _revisions[key];
    if (old != null && change.revision <= old.revision) {
      if (change.revision == old.revision && change.state != old.state) _incomplete = true;
      return priorIncomplete != _incomplete || priorUnavailable != _feedUnavailable;
    }
    if (old == null && _revisions.length == maxTombstones) {
      _incomplete = true;
      return !priorIncomplete;
    }
    _revisions[key] = _Revision(change.revision, change.state);
    if (old == null && _healthyFirst(change)) return priorIncomplete != _incomplete;
    _rows.remove(key);
    if (_rows.length == maxRows) {
      _rows.remove(_rows.keys.first);
      _incomplete = true;
    }
    _rows[key] = RuntimeCriticalNoticeRow._(
      change.domain,
      change.recordId,
      change.revision,
      change.state,
      _severity(change),
      false,
    );
    return true;
  }

  bool _reset(RuntimeNoticeFrame frame) {
    if (frame.change != null || frame.epoch < _epoch) return false;
    final next = frame.scope;
    // A same-epoch ordinary refresh must not replace an established object.
    if (frame.epoch == _epoch && _scope != null && next != null && !identical(_scope, next)) return false;
    if (next != null && frame.unsupported) return false;
    if (next != null && !next.same(_retainedScope)) {
      _rows.clear();
      _revisions.clear();
      _incomplete = false;
      _retainedScope = next;
    }
    _scope = next;
    _epoch = frame.epoch;
    _unsupported = frame.unsupported;
    _incomplete |= frame.incomplete;
    _feedUnavailable |= frame.streamUnavailable;
    return true;
  }

  bool _current(RuntimeCriticalNoticeSnapshot captured) =>
      !_disposed &&
      _scope != null &&
      identical(captured._ring, _identity) &&
      captured.epoch == _epoch &&
      identical(captured.scope, _scope);

  bool markRead(RuntimeCriticalNoticeSnapshot captured, RuntimeCriticalNoticeRow row) {
    if (!_current(captured)) return false;
    final key = _Key(row.domain, row.recordId);
    final current = _rows[key];
    if (!identical(current, row) || current == null || current.isRead) return false;
    _rows[key] = current._read();
    return true;
  }

  bool dismiss(RuntimeCriticalNoticeSnapshot captured, RuntimeCriticalNoticeRow row) {
    if (!_current(captured)) return false;
    final key = _Key(row.domain, row.recordId);
    if (!identical(_rows[key], row)) return false;
    _rows.remove(key);
    return true;
  }

  bool clear(RuntimeCriticalNoticeSnapshot captured) {
    if (!_current(captured) || _rows.isEmpty) return false;
    var changed = false;
    for (final row in captured.rows) {
      final key = _Key(row.domain, row.recordId);
      if (identical(_rows[key], row)) {
        _rows.remove(key);
        changed = true;
      }
    }
    return changed;
  }

  void dispose() {
    _disposed = true;
    _scope = null;
    _retainedScope = null;
    _rows.clear();
    _revisions.clear();
  }

  static bool _healthyFirst(RuntimeNoticeChange change) => switch ((change.domain, change.state)) {
    (RuntimeNoticeDomain.sync, RuntimeNoticeState.idle) ||
    (RuntimeNoticeDomain.tunnel, RuntimeNoticeState.running) ||
    (RuntimeNoticeDomain.tunnel, RuntimeNoticeState.stopped) ||
    (RuntimeNoticeDomain.terminal, RuntimeNoticeState.connected) ||
    (RuntimeNoticeDomain.backup, RuntimeNoticeState.completed) ||
    (RuntimeNoticeDomain.transfer, RuntimeNoticeState.completed) ||
    (RuntimeNoticeDomain.edit, RuntimeNoticeState.synced) => true,
    _ => false,
  };

  static RuntimeCriticalNoticeSeverity _severity(RuntimeNoticeChange change) => switch (change.state) {
    RuntimeNoticeState.error ||
    RuntimeNoticeState.deviceRevoked ||
    RuntimeNoticeState.reauthRequired ||
    RuntimeNoticeState.accountDisabled ||
    RuntimeNoticeState.notTrusted ||
    RuntimeNoticeState.vaultGone ||
    RuntimeNoticeState.upgradeRequired ||
    RuntimeNoticeState.failed || RuntimeNoticeState.integrityWarning || RuntimeNoticeState.serverRollback ||
    RuntimeNoticeState.secretConflict || RuntimeNoticeState.revoked || RuntimeNoticeState.keyConflict ||
    RuntimeNoticeState.invalidLog || RuntimeNoticeState.logRollback || RuntimeNoticeState.removed => RuntimeCriticalNoticeSeverity.error,
    RuntimeNoticeState.conflictCopy || RuntimeNoticeState.registered || RuntimeNoticeState.approvalRequested ||
    RuntimeNoticeState.recoveryChanged || RuntimeNoticeState.conflict || RuntimeNoticeState.recoveryAvailable =>
      RuntimeCriticalNoticeSeverity.warning,
    RuntimeNoticeState.closed || RuntimeNoticeState.offline => RuntimeCriticalNoticeSeverity.warning,
    RuntimeNoticeState.stopped when change.domain == RuntimeNoticeDomain.sync => RuntimeCriticalNoticeSeverity.warning,
    _ => RuntimeCriticalNoticeSeverity.info,
  };
}
