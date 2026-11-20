import 'dart:convert';

// Pure staged projection of Native metadata. No subscription, service, timer,
// filesystem, RPC, resolver or executable destination exists in this module.

const _maxRevision = 9007199254740991;
final _uuid = RegExp(r'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$');
bool _id(Object? value) => value is String && value != '00000000-0000-0000-0000-000000000000' && _uuid.hasMatch(value);

// Native notification nonces are opaque UTF-8 strings, not object UUIDs.
// Preserve the exact value; it is matched to the original profiles snapshot.
bool _nonce(Object? value) {
  if (value is! String || value.isEmpty ||
      value.codeUnits.any((unit) => unit <= 31 || unit >= 127 && unit <= 159)) {
    return false;
  }
  try {
    final bytes = utf8.encode(value);
    return bytes.length <= 256 && utf8.decode(bytes) == value;
  } on FormatException {
    return false;
  }
}

enum RuntimeNoticeDomain { sync, tunnel, terminal, security, device, backup, team, edit, transfer }

enum RuntimeNoticeState {
  offline('offline'),
  error('error'),
  stopped('stopped'),
  deviceRevoked('device_revoked'),
  reauthRequired('reauth_required'),
  accountDisabled('account_disabled'),
  notTrusted('not_trusted'),
  vaultGone('vault_gone'),
  upgradeRequired('upgrade_required'),
  idle('idle'),
  running('running'),
  failed('failed'),
  connected('connected'),
  closed('closed'),
  integrityWarning('integrity_warning'),
  serverRollback('server_rollback'),
  secretConflict('secret_conflict'),
  conflictCopy('conflict_copy'),
  registered('registered'),
  approvalRequested('approval_requested'),
  revoked('revoked'),
  recoveryChanged('recovery_changed'),
  keyConflict('key_conflict'),
  completed('completed'),
  invalidLog('invalid_log'),
  logRollback('log_rollback'),
  removed('removed'),
  conflict('conflict'),
  recoveryAvailable('recovery_available'),
  synced('synced');

  const RuntimeNoticeState(this.wire);
  final String wire;
}

const _states = <RuntimeNoticeDomain, Set<RuntimeNoticeState>>{
  RuntimeNoticeDomain.sync: {
    RuntimeNoticeState.offline,
    RuntimeNoticeState.error,
    RuntimeNoticeState.stopped,
    RuntimeNoticeState.deviceRevoked,
    RuntimeNoticeState.reauthRequired,
    RuntimeNoticeState.accountDisabled,
    RuntimeNoticeState.notTrusted,
    RuntimeNoticeState.vaultGone,
    RuntimeNoticeState.upgradeRequired,
    RuntimeNoticeState.idle,
  },
  RuntimeNoticeDomain.tunnel: {RuntimeNoticeState.running, RuntimeNoticeState.failed, RuntimeNoticeState.stopped},
  RuntimeNoticeDomain.terminal: {RuntimeNoticeState.connected, RuntimeNoticeState.closed, RuntimeNoticeState.failed},
  RuntimeNoticeDomain.security: {
    RuntimeNoticeState.integrityWarning, RuntimeNoticeState.serverRollback,
    RuntimeNoticeState.secretConflict, RuntimeNoticeState.conflictCopy,
  },
  RuntimeNoticeDomain.device: {
    RuntimeNoticeState.registered, RuntimeNoticeState.approvalRequested, RuntimeNoticeState.revoked,
    RuntimeNoticeState.recoveryChanged, RuntimeNoticeState.keyConflict,
  },
  RuntimeNoticeDomain.backup: {RuntimeNoticeState.failed, RuntimeNoticeState.completed},
  RuntimeNoticeDomain.team: {RuntimeNoticeState.invalidLog, RuntimeNoticeState.logRollback, RuntimeNoticeState.removed},
  RuntimeNoticeDomain.edit: {
    RuntimeNoticeState.error, RuntimeNoticeState.conflict, RuntimeNoticeState.recoveryAvailable, RuntimeNoticeState.synced,
  },
  RuntimeNoticeDomain.transfer: {RuntimeNoticeState.failed, RuntimeNoticeState.completed},
};

/// Ephemeral observer identity, never a grant or execution capability. Capture
/// only from the ORIGINAL completed Native profiles refresh at the same epoch.
final class RuntimeNoticeScope {
  const RuntimeNoticeScope._(this.profileId, this.nonce);
  final String profileId, nonce;
  static RuntimeNoticeScope? fromProfile(Map<String, dynamic> value) {
    if (value['active'] != true ||
        value['vault_state'] != 'unlocked' ||
        !_id(value['id']) ||
        !_nonce(value['notification_scope_id'])) {
      return null;
    }
    return RuntimeNoticeScope._(value['id'] as String, value['notification_scope_id'] as String);
  }

  bool same(RuntimeNoticeScope? other) => other != null && profileId == other.profileId && nonce == other.nonce;
}

final class RuntimeNoticeChange {
  const RuntimeNoticeChange._(this.profileId, this.scopeId, this.recordId, this.revision, this.domain, this.state);
  final String profileId, scopeId, recordId;
  final int revision;
  final RuntimeNoticeDomain domain;
  final RuntimeNoticeState state;

  /// Fixed six-field envelope and two-field change. Never retain source maps,
  /// labels, status payloads, raw error strings, paths, hosts or credentials.
  static RuntimeNoticeChange? parse(Map<String, dynamic> value) {
    if (value.length != 6 || value['type'] != 'critical_state_changed') {
      return null;
    }
    final profile = value['profile_id'], scope = value['scope_id'], record = value['record_id'];
    final revision = value['revision'], change = value['change'];
    if (!_id(profile) ||
        !_nonce(scope) ||
        !_id(record) ||
        revision is! int ||
        revision < 1 ||
        revision > _maxRevision ||
        change is! Map ||
        change.length != 2) {
      return null;
    }
    final domainValue = change['domain'], stateValue = change['state'];
    if (domainValue is! String || stateValue is! String || domainValue.length > 8 || stateValue.length > 32) {
      return null;
    }
    final domains = RuntimeNoticeDomain.values.where((candidate) => candidate.name == domainValue);
    final states = RuntimeNoticeState.values.where((candidate) => candidate.wire == stateValue);
    if (domains.length != 1 || states.length != 1) return null;
    final domain = domains.single, state = states.single;
    if (!_states[domain]!.contains(state) || domain == RuntimeNoticeDomain.sync && record != profile) {
      return null;
    }
    return RuntimeNoticeChange._(profile as String, scope as String, record as String, revision, domain, state);
  }
}

/// Existing backend event hub will publish these through its one Native stream;
/// this value alone starts nothing. UTC receipt time belongs to the UI owner,
/// and must never be advertised as a server completion timestamp.
final class RuntimeNoticeFrame {
  const RuntimeNoticeFrame({
    required this.epoch,
    this.scope,
    this.change,
    this.reset = false,
    this.incomplete = false,
    this.streamUnavailable = false,
    this.unsupported = false,
  });
  final int epoch;
  final RuntimeNoticeScope? scope;
  final RuntimeNoticeChange? change;
  final bool reset, incomplete, streamUnavailable, unsupported;
}

/// Exact metadata snapshot bound by the SAME Native profile-read emitter lock.
/// This projection never obtains scope/permissions from a record or event.
final class _NativeNoticeSnapshot {
  const _NativeNoticeSnapshot(this.records, this.incomplete);
  final List<RuntimeNoticeChange> records;
  final bool incomplete;
  static _NativeNoticeSnapshot? parse(Object? value, RuntimeNoticeScope owner) {
    if (value is! Map<String,dynamic> || value.length != 5 ||
        !value.keys.every(const {'version','profile_id','scope_id','records','incomplete'}.contains) ||
        value['version'] is! int || value['version'] != 1 ||
        value['profile_id'] != owner.profileId || value['scope_id'] != owner.nonce ||
        value['incomplete'] is! bool) {
      return null;
    }
    final rows=value['records'];
    if (rows is! List || rows.length > 64) return null;
    final changes=<RuntimeNoticeChange>[];
    final keys=<(RuntimeNoticeDomain,String)>{};
    int? previous;
    for (final row in rows) {
      if (row is! Map<String,dynamic>) return null;
      final change=RuntimeNoticeChange.parse(row);
      if (change == null || change.profileId != owner.profileId || change.scopeId != owner.nonce ||
          !keys.add((change.domain,change.recordId)) || previous != null && change.revision > previous) {
        return null;
      }
      changes.add(change);
      previous=change.revision;
    }
    return _NativeNoticeSnapshot(List.unmodifiable(changes),value['incomplete'] as bool);
  }
}

/// Typed metadata waiting for one captured unlock observer, never an authority.
final class _UnlockNotice {
  const _UnlockNotice(this.profileId, this.scopeId, this.change);
  final String profileId, scopeId;
  final RuntimeNoticeChange? change; // null is incompleteness, not raw diagnostics
}

/// Synchronous scope fence for the EXISTING subscriber. Only a Native unlock
/// boundary permits bounded startup metadata; actual profiles read admits it.
final class RuntimeNoticeProjection {
  static const maxUnlockNotices = 64;
  int _epoch = 0;
  RuntimeNoticeScope? _scope;
  RuntimeNoticeScope? _lostScope;
  RuntimeNoticeScope? _snapshotOwnerSeen;
  bool _disposed = false, _incomplete = false, _unavailable = false, _unsupported = false;
  int? _unlockEpoch;
  final _pending = <_UnlockNotice>[];
  bool _pendingOverflow = false;
  final _replay = <RuntimeNoticeChange>[];
  bool _awaitingStartupOwner = false;
  int get epoch => _epoch;
  RuntimeNoticeFrame get currentScope => RuntimeNoticeFrame(
    epoch: _epoch,
    scope: _scope,
    reset: true,
    incomplete: _incomplete,
    streamUnavailable: _unavailable,
    unsupported: _unsupported,
  );

  void _discardStartup() {
    _unlockEpoch = null;
    _pending.clear();
    _pendingOverflow = false;
    _replay.clear();
    _awaitingStartupOwner = false;
  }

  /// The hub sets unlockPending ONLY for its actual vault_unlocked event.
  /// All other boundaries/global loss/dispose discard pending data by default.
  RuntimeNoticeFrame boundary({bool unlockPending = false}) {
    _discardStartup();
    if (!_disposed) {
      if (_epoch == _maxRevision) {
        _disposed = true;
        _unavailable = true;
      } else {
        _epoch++;
      }
    }
    _scope = null;
    _incomplete = false;
    _unsupported = false;
    if (unlockPending && !_disposed && !_unavailable) _unlockEpoch = _epoch;
    return currentScope;
  }

  RuntimeNoticeFrame? retireGlobalStream({bool closed = false}) {
    if (_disposed) return null;
    final original = _scope;
    _unavailable |= closed;
    boundary();
    _incomplete = true;
    _lostScope = original ?? _lostScope;
    return currentScope;
  }

  RuntimeNoticeFrame? establish(int originalEpoch, Map<String, dynamic>? nativeProfile) {
    if (_disposed || originalEpoch != _epoch) return null;
    final next = nativeProfile == null ? null : RuntimeNoticeScope.fromProfile(nativeProfile);
    final startup = _unlockEpoch == originalEpoch;
    final accepted = <RuntimeNoticeChange>[];
    var startupGap = false;
    if (startup && next != null && !_unavailable) {
      startupGap = _pendingOverflow;
      for (final pending in _pending) {
        if (pending.profileId != next.profileId || pending.scopeId != next.nonce) continue;
        if (pending.change case final change?) {
          accepted.add(change);
        } else {
          startupGap = true;
        }
      }
    }
    if (startup) _discardStartup();

    // Optional on old backends. A present malformed snapshot is a gap, never
    // a partial authorization or a guessed scope. Locked/foreign profile data
    // cannot seed any records; only this actual original-epoch read may do so.
    final rawSnapshot = nativeProfile?['critical_notice_snapshot'];
    final snapshot = next != null && rawSnapshot != null && !_unavailable
        ? _NativeNoticeSnapshot.parse(rawSnapshot, next) : null;
    final snapshotGap = next != null && rawSnapshot != null &&
        (snapshot == null || snapshot.incomplete);
    final snapshotNew = snapshot != null && !next!.same(_snapshotOwnerSeen);
    if (snapshotNew) _snapshotOwnerSeen = next;

    final unsupported = nativeProfile != null && nativeProfile['active'] == true &&
        nativeProfile['vault_state'] == 'unlocked' && _id(nativeProfile['id']) && next == null;
    if ((_scope == null && next == null || _scope?.same(next) == true) && unsupported == _unsupported) {
      if (next != null && (snapshotNew || snapshotGap && !_incomplete)) {
        _incomplete |= snapshotGap;
        if (_incomplete) _lostScope = _scope;
        if (snapshotNew) {
          _awaitingStartupOwner = true;
          for (final change in snapshot.records.reversed) {
            _rememberStartup(change);
          }
        }
        return currentScope;
      }
      return null;
    }
    if (_epoch == _maxRevision) return boundary();
    if (!startup) _discardStartup();
    _epoch++;
    _scope = next;
    _unsupported = unsupported;
    _incomplete = next != null && (_lostScope?.same(next) == true || startupGap || snapshotGap);
    if (next != null && !_incomplete) _lostScope = null;
    if (next != null && _incomplete) _lostScope = next;
    if ((startup || snapshotNew) && next != null && !_unavailable) {
      _awaitingStartupOwner = true;
      if (snapshotNew) {
        for (final change in snapshot.records.reversed) {
          _rememberStartup(change);
        }
      }
      for (final change in accepted) {
        _rememberStartup(change);
      }
    } else if (next == null || !_awaitingStartupOwner) {
      _replay.clear();
    }
    return currentScope;
  }

  List<RuntimeNoticeFrame> get unlockReplay => [
    for (final change in _replay)
      RuntimeNoticeFrame(epoch: _epoch, scope: _scope, change: change, incomplete: _incomplete),
  ];

  /// Only the existing common owner drains startup delivery. Generic lifetime
  /// observers reading currentScope must not steal it before Shell mounts.
  List<RuntimeNoticeFrame> takeUnlockReplay(RuntimeNoticeFrame original, String profileId) {
    if (!_awaitingStartupOwner || _disposed || _unavailable || _scope == null ||
        original.epoch != _epoch || !identical(original.scope, _scope) || _scope!.profileId != profileId) {
      return const [];
    }
    final delivery = unlockReplay;
    _replay.clear();
    _awaitingStartupOwner = false;
    return delivery;
  }

  void _rememberStartup(RuntimeNoticeChange change) {
    if (!_awaitingStartupOwner) return;
    final index = _replay.indexWhere((old) => old.domain == change.domain && old.recordId == change.recordId);
    if (index >= 0) {
      final previous = _replay[index];
      if (change.revision <= previous.revision) {
        if (change.revision == previous.revision && change.state != previous.state) {
          _incomplete = true;
          _lostScope = _scope;
        }
        return;
      }
      _replay.removeAt(index);
    } else if (_replay.length >= maxUnlockNotices) {
      _incomplete = true;
      _lostScope = _scope;
      // The Native snapshot defines newest-first revision ordering. Merging
      // live delivery retains at most the same64 metadata records, not payloads.
      var oldest = 0;
      for (var i = 1; i < _replay.length; i++) {
        if (_replay[i].revision < _replay[oldest].revision) oldest = i;
      }
      if (change.revision <= _replay[oldest].revision) return;
      _replay.removeAt(oldest);
    }
    _replay.add(change);
  }

  void _queueUnlock(Map<String, dynamic> value) {
    if (_unlockEpoch != _epoch || _unavailable) return;
    final profile = value['profile_id'], scope = value['scope_id'];
    if (!_id(profile) || !_nonce(scope)) return;
    final parsed = value['type'] == 'critical_state_changed' ? RuntimeNoticeChange.parse(value) : null;
    if (_pending.length >= maxUnlockNotices) {
      _pendingOverflow = true;
      return;
    }
    // Strict parsing retains only bounded IDs/opaque nonce/enums/revision. Extra payloads
    // become a gap for that exact origin; names/errors/paths are never copied.
    _pending.add(_UnlockNotice(profile as String, scope as String, parsed));
  }

  RuntimeNoticeFrame? gap({bool streamUnavailable = false}) {
    if (_disposed) return null;
    _unavailable |= streamUnavailable;
    if (streamUnavailable) _discardStartup();
    if (_scope == null) return null;
    _incomplete = true;
    _lostScope = _scope;
    return RuntimeNoticeFrame(epoch: _epoch, scope: _scope, incomplete: true, streamUnavailable: _unavailable);
  }

  RuntimeNoticeFrame? consume(Map<String, dynamic> value) {
    if (_disposed || _unavailable) return null;
    final type = value['type'];
    if (type != 'critical_state_changed' && type != 'critical_notice_gap') return null;
    final source = _scope;
    if (source == null) {
      _queueUnlock(value);
      return null; // no event-derived scope or early delivery
    }
    if (value['profile_id'] != source.profileId || value['scope_id'] != source.nonce) return null;
    if (type == 'critical_notice_gap') return gap();
    final change = RuntimeNoticeChange.parse(value);
    if (change == null) return gap();
    _rememberStartup(change);
    return RuntimeNoticeFrame(epoch: _epoch, scope: source, change: change, incomplete: _incomplete);
  }

  void dispose() {
    _disposed = true;
    _scope = null;
    _lostScope = null;
    _snapshotOwnerSeen = null;
    _incomplete = false;
    _unsupported = false;
    _discardStartup();
  }
}
