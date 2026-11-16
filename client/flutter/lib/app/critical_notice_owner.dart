import 'dart:async';

import 'package:consolecrypt/app/critical_notice_models.dart';
import 'package:consolecrypt/app/critical_runtime_notices.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/services/job_notice_events.dart';
import 'package:consolecrypt/core/services/runtime_critical_notice_ring.dart';
import 'package:consolecrypt/core/services/runtime_notice_adapter.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/core/util/value_stream.dart';
import 'package:consolecrypt/monitoring/monitoring_pinned_observation.dart';

enum CriticalPinStatus { retrying, stopped, recovered, manualStopped }

enum CriticalPinReason { readFailure, sourceChanged, retryLimit, paused, closed, manual, recovered }

enum CriticalRefreshError { failed, invalidResponse, contextChanged }

sealed class CriticalNoticeTarget {
  const CriticalNoticeTarget();
}

final class CriticalJobTarget extends CriticalNoticeTarget {
  const CriticalJobTarget(this.jobId);
  final String jobId;
}

final class CriticalMonitoringTarget extends CriticalNoticeTarget {
  const CriticalMonitoringTarget({this.sourceId, this.sourceRevision});
  final String? sourceId;
  final int? sourceRevision;
}

final class CriticalRuntimeTarget extends CriticalNoticeTarget {
  const CriticalRuntimeTarget(this.domain);
  final RuntimeNoticeDomain domain;
}

final class CriticalPinNotice {
  const CriticalPinNotice({
    required this.id,
    required this.status,
    required this.reason,
    required this.observedUtc,
    this.sourceId,
    this.sourceRevision,
    this.unread = true,
  });
  final String id;
  final String? sourceId;
  final int? sourceRevision;
  final CriticalPinStatus status;
  final CriticalPinReason reason;
  final DateTime observedUtc;
  final bool unread;
  CriticalPinNotice read() => CriticalPinNotice(
    id: id,
    status: status,
    reason: reason,
    observedUtc: observedUtc,
    sourceId: sourceId,
    sourceRevision: sourceRevision,
    unread: false,
  );
}

final class CriticalNoticeRow {
  const CriticalNoticeRow.job(CriticalJobNotice value) : job = value, pin = null, runtime = null;
  const CriticalNoticeRow.pin(CriticalPinNotice value) : pin = value, job = null, runtime = null;
  const CriticalNoticeRow.runtime(CriticalRuntimeNotice value) : runtime = value, job = null, pin = null;
  final CriticalRuntimeNotice? runtime;
  final CriticalJobNotice? job;
  final CriticalPinNotice? pin;
  String get id => job?.id ?? pin?.id ?? runtime!.id;
  DateTime get observedUtc => job?.observedUtc ?? pin?.observedUtc ?? runtime!.observedUtc;
  bool get unread => job?.unread ?? pin?.unread ?? runtime!.unread;
  CriticalNoticeSeverity get severity =>
      job?.severity ??
      (runtime != null
          ? switch (runtime!.value.severity) {
              RuntimeCriticalNoticeSeverity.info => CriticalNoticeSeverity.info,
              RuntimeCriticalNoticeSeverity.warning => CriticalNoticeSeverity.warning,
              RuntimeCriticalNoticeSeverity.error => CriticalNoticeSeverity.danger,
            }
          : switch (pin!.status) {
              CriticalPinStatus.retrying => CriticalNoticeSeverity.warning,
              CriticalPinStatus.stopped => switch (pin!.reason) {
                CriticalPinReason.paused || CriticalPinReason.closed => CriticalNoticeSeverity.warning,
                _ => CriticalNoticeSeverity.danger,
              },
              CriticalPinStatus.recovered || CriticalPinStatus.manualStopped => CriticalNoticeSeverity.info,
            });
  CriticalNoticeTarget get target => job != null
      ? CriticalJobTarget(job!.change.jobId)
      : runtime != null
      ? CriticalRuntimeTarget(runtime!.value.domain)
      : CriticalMonitoringTarget(sourceId: pin!.sourceId, sourceRevision: pin!.sourceRevision);
}

final class CriticalNoticeSnapshot {
  const CriticalNoticeSnapshot({
    this.rows = const [],
    this.refreshing = false,
    this.refreshNeeded = false,
    this.feedUnavailable = false,
    this.runtimeUnsupported = false,
    this.runtimeIncomplete = false,
    this.runtimeFeedUnavailable = false,
    this.canRefresh = false,
    this.error,
  });
  final List<CriticalNoticeRow> rows;
  final bool refreshing,
      refreshNeeded,
      feedUnavailable,
      canRefresh,
      runtimeUnsupported,
      runtimeIncomplete,
      runtimeFeedUnavailable;
  final CriticalRefreshError? error;
  int get unread => rows.where((row) => row.unread).length;
}

/// Profile-owned UI state only: observes existing owners and the ONE existing
/// event stream. No timer/service factory/native mutation/persistence exists.
final class CriticalNoticeOwner {
  CriticalNoticeOwner({
    required this.scope,
    required String profileId,
    required this.isCurrent,
    this.jobs,
    JobNoticeEvents? events,
    RuntimeNoticeEvents? runtimeEvents,
    this.pin,
    DateTime Function()? now,
  }) : _runtimeEvents = runtimeEvents,
       ring = CriticalJobNoticeRing(scope: scope, profileId: profileId),
       now = now ?? DateTime.now,
       runtime = CriticalRuntimeNotices(profileId) {
    if (events != null) {
      ring.ingest(scope, events.currentScope, this.now().toUtc());
      _eventSubscription = events.watchJobNotices().listen(_event);
    }
    if (runtimeEvents != null) {
      final initial = runtimeEvents.currentScope;
      runtime.ingest(initial, this.now().toUtc());
      _drainUnlockReplay(initial);
      _runtimeSubscription = runtimeEvents.watchRuntimeNotices().listen(_runtime);
    }
    if (pin != null) {
      _pinSubscription = pin!.changes.listen(_pin);
      _pin(pin!.state);
    }
    _publish();
  }
  final Object scope;
  final bool Function() isCurrent;
  final AutomationJobService? jobs;
  final MonitoringPinnedObservation? pin;
  final DateTime Function() now;
  final CriticalJobNoticeRing ring;
  final CriticalRuntimeNotices runtime;
  final RuntimeNoticeEvents? _runtimeEvents;
  StreamSubscription<RuntimeNoticeFrame>? _runtimeSubscription;
  final _state = ValueStreamController<CriticalNoticeSnapshot>(const CriticalNoticeSnapshot());
  CriticalNoticeSnapshot get state => _state.value;
  Stream<CriticalNoticeSnapshot> get changes => _state.stream;
  StreamSubscription<JobNoticeFrame>? _eventSubscription;
  StreamSubscription<MonitoringPinState>? _pinSubscription;
  final _pins = <String, CriticalPinNotice>{};
  Expando<int> _runIds = Expando<int>();
  (String?, int?, int)? _lastPin;
  int _nextRun = 0, _request = 0;
  bool _disposed = false, _scopeGone = false, _refreshing = false;
  CriticalRefreshError? _error;
  bool get current => !_disposed && !_scopeGone && isCurrent();
  List<CriticalNoticeRow> get _rows => [
    for (final row in ring.rows) CriticalNoticeRow.job(row),
    for (final row in _pins.values) CriticalNoticeRow.pin(row),
    for (final row in runtime.rows) CriticalNoticeRow.runtime(row),
  ]..sort((a, b) => b.observedUtc.compareTo(a.observedUtc));

  void _publish() {
    if (!current) return;
    var rows = _rows;
    while (rows.length > CriticalJobNoticeRing.maxRows) {
      final last = rows.last;
      if (last.job != null) {
        ring.dismiss(scope, last.id);
      } else if (last.runtime != null) {
        runtime.dismiss(last.runtime!);
      } else {
        _pins.remove(last.id);
      }
      rows = _rows;
    }
    _state.value = CriticalNoticeSnapshot(
      rows: List.unmodifiable(rows),
      refreshing: _refreshing,
      refreshNeeded: ring.refreshNeeded,
      feedUnavailable: ring.feedUnavailable,
      runtimeIncomplete: runtime.snapshot.incomplete,
      runtimeFeedUnavailable: runtime.snapshot.feedUnavailable,
      runtimeUnsupported: runtime.snapshot.unsupported,
      canRefresh: jobs != null && ring.accepting,
      error: _error,
    );
  }

  void _drainUnlockReplay(RuntimeNoticeFrame frame) {
    if (!current || !frame.reset || frame.scope == null) return;
    if (_runtimeEvents case final RuntimeNoticeEventAdapter adapter) {
      for (final replay in adapter.takeUnlockReplay(frame, ring.profileId)) {
        runtime.ingest(replay, now().toUtc());
      }
    }
  }

  void _runtime(RuntimeNoticeFrame frame) {
    if (!current) return;
    runtime.ingest(frame, now().toUtc());
    _drainUnlockReplay(frame);
    _publish();
  }

  void _event(JobNoticeFrame frame) {
    if (!current || ring.epoch != null && frame.epoch < ring.epoch!) return;
    if (frame.reset) {
      runtime.fence();
      _request++;
      _refreshing = false;
      _error = null;
      _pins.clear();
      _lastPin = null;
      _runIds = Expando<int>();
    }
    ring.ingest(scope, frame, now().toUtc());
    _publish();
  }

  void _pin(MonitoringPinState value) {
    if (!current || !ring.accepting) return;
    if (value.stopReason == MonitoringPinStop.scopeChanged) {
      _pins.clear();
      _lastPin = null;
      _publish();
      return;
    }
    if (value.visibility == MonitoringObservationVisibility.foregroundOnly) {
      _lastPin = null;
      return;
    }
    if (value.plan case final plan?) {
      final serial = _runIds[plan] ?? ++_nextRun;
      _runIds[plan] = serial;
      _lastPin = (plan.source.id, plan.source.revision, serial);
    }
    final ref = _lastPin;
    if (ref == null) return;
    final key = 'pin:${ref.$3}', old = _pins[key];
    CriticalPinStatus? status;
    CriticalPinReason? reason;
    if (value.phase == MonitoringPinPhase.backoff) {
      status = CriticalPinStatus.retrying;
      reason = CriticalPinReason.readFailure;
    } else if (value.phase == MonitoringPinPhase.stopped) {
      switch (value.stopReason) {
        case MonitoringPinStop.scopeChanged:
          _pins.clear();
          _lastPin = null;
          _publish();
          return;
        case MonitoringPinStop.manual:
          _lastPin = null;
          if (old != null) {
            status = CriticalPinStatus.manualStopped;
            reason = CriticalPinReason.manual;
          }
        case MonitoringPinStop.sourceChanged:
          status = CriticalPinStatus.stopped;
          reason = CriticalPinReason.sourceChanged;
        case MonitoringPinStop.retryLimit:
          status = CriticalPinStatus.stopped;
          reason = CriticalPinReason.retryLimit;
        case MonitoringPinStop.appPaused:
          status = CriticalPinStatus.stopped;
          reason = CriticalPinReason.paused;
        case MonitoringPinStop.appClosed:
          status = CriticalPinStatus.stopped;
          reason = CriticalPinReason.closed;
        default:
          status = CriticalPinStatus.stopped;
          reason = CriticalPinReason.readFailure;
      }
    } else if (value.phase == MonitoringPinPhase.waiting &&
        value.hasAcceptedResult &&
        value.error == null &&
        old?.status == CriticalPinStatus.retrying) {
      status = CriticalPinStatus.recovered;
      reason = CriticalPinReason.recovered;
    }
    if (status == null || reason == null) return;
    if (old?.status == status && old?.reason == reason) return;
    _pins[key] = CriticalPinNotice(
      id: key,
      status: status,
      reason: reason,
      observedUtc: now().toUtc(),
      sourceId: ref.$1,
      sourceRevision: ref.$2,
      unread: status != CriticalPinStatus.manualStopped,
    );
    _publish();
  }

  Future<void> refreshJobs() async {
    final service = jobs;
    if (!current || service == null || _refreshing || !ring.accepting) return;
    final token = ++_request, epoch = ring.epoch, gap = ring.gapVersion;
    _refreshing = true;
    _error = null;
    _publish();
    try {
      final payload = await service.jobs();
      if (!current || token != _request) return;
      final changes = JobNoticeChange.catalog(ring.profileId, payload);
      if (changes == null) {
        _error = CriticalRefreshError.invalidResponse;
        return;
      }
      if (!ring.reconcile(
        scope,
        originalEpoch: epoch,
        originalGapVersion: gap,
        changes: changes,
        observed: now().toUtc(),
      )) {
        _error = CriticalRefreshError.contextChanged;
      }
    } catch (error) {
      if (!current || token != _request) return;
      if (error is AppException &&
          (error.reason == AppErrorReason.vaultLocked || error.reason == AppErrorReason.noActiveProfile)) {
        _scopeGone = true;
        ring.dispose();
        runtime.dispose();
        _pins.clear();
        _state.value = const CriticalNoticeSnapshot();
        return;
      }
      _error = CriticalRefreshError.failed;
    } finally {
      if (current && token == _request) {
        _refreshing = false;
        _publish();
      }
    }
  }

  CriticalNoticeTarget? target(String id) {
    if (!current) return null;
    return state.rows.where((row) => row.id == id).firstOrNull?.target;
  }

  bool rowCurrent(CriticalNoticeRow row) =>
      current && (row.runtime == null ? target(row.id) != null : runtime.isCurrent(row.runtime!));

  void markReadRow(CriticalNoticeRow row) {
    if (!rowCurrent(row)) return;
    if (row.runtime case final value?) {
      runtime.markRead(value);
      _publish();
    } else {
      markRead(row.id);
    }
  }

  void dismissRow(CriticalNoticeRow row) {
    if (!rowCurrent(row)) return;
    if (row.runtime case final value?) {
      runtime.dismiss(value);
      _publish();
    } else {
      dismiss(row.id);
    }
  }

  void markRead(String id) {
    if (!current) return;
    if (_pins[id] case final row?) {
      _pins[id] = row.read();
    } else {
      ring.markRead(scope, id);
    }
    _publish();
  }

  void dismiss(String id) {
    if (!current) return;
    _pins.remove(id);
    ring.dismiss(scope, id);

    _publish();
  }

  void clear() {
    if (!current) return;
    _pins.clear();
    ring.clearRows(scope);
    runtime.clear();
    _publish();
  }

  Future<void> dispose() async {
    if (_disposed) return;
    _disposed = true;
    _request++;
    ring.dispose();
    runtime.dispose();
    _pins.clear();
    _lastPin = null;
    _runIds = Expando<int>();
    _state.value = const CriticalNoticeSnapshot();
    await _eventSubscription?.cancel();
    await _runtimeSubscription?.cancel();
    await _pinSubscription?.cancel();
    await _state.close();
  }
}
