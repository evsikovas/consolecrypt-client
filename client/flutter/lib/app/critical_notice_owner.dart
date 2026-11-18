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

enum CriticalRefreshError { failed, invalidResponse, contextChanged }

sealed class CriticalNoticeTarget {
  const CriticalNoticeTarget();
}

final class CriticalJobTarget extends CriticalNoticeTarget {
  const CriticalJobTarget(this.jobId);
  final String jobId;
}

final class CriticalRuntimeTarget extends CriticalNoticeTarget {
  const CriticalRuntimeTarget(this.domain);
  final RuntimeNoticeDomain domain;
}

final class CriticalNoticeRow {
  const CriticalNoticeRow.job(CriticalJobNotice value) : job = value, runtime = null;
  const CriticalNoticeRow.runtime(CriticalRuntimeNotice value) : runtime = value, job = null;
  final CriticalRuntimeNotice? runtime;
  final CriticalJobNotice? job;
  String get id => job?.id ?? runtime!.id;
  DateTime get observedUtc => job?.observedUtc ?? runtime!.observedUtc;
  bool get unread => job?.unread ?? runtime!.unread;
  CriticalNoticeSeverity get severity =>
      job?.severity ??
      switch (runtime!.value.severity) {
        RuntimeCriticalNoticeSeverity.info => CriticalNoticeSeverity.info,
        RuntimeCriticalNoticeSeverity.warning => CriticalNoticeSeverity.warning,
        RuntimeCriticalNoticeSeverity.error => CriticalNoticeSeverity.danger,
      };
  CriticalNoticeTarget get target =>
      job != null ? CriticalJobTarget(job!.change.jobId) : CriticalRuntimeTarget(runtime!.value.domain);
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
    _publish();
  }
  final Object scope;
  final bool Function() isCurrent;
  final AutomationJobService? jobs;
  final DateTime Function() now;
  final CriticalJobNoticeRing ring;
  final CriticalRuntimeNotices runtime;
  final RuntimeNoticeEvents? _runtimeEvents;
  StreamSubscription<RuntimeNoticeFrame>? _runtimeSubscription;
  final _state = ValueStreamController<CriticalNoticeSnapshot>(const CriticalNoticeSnapshot());
  CriticalNoticeSnapshot get state => _state.value;
  Stream<CriticalNoticeSnapshot> get changes => _state.stream;
  StreamSubscription<JobNoticeFrame>? _eventSubscription;
  int _request = 0;
  bool _disposed = false, _scopeGone = false, _refreshing = false;
  CriticalRefreshError? _error;
  bool get current => !_disposed && !_scopeGone && isCurrent();
  List<CriticalNoticeRow> get _rows => [
    for (final row in ring.rows) CriticalNoticeRow.job(row),
    for (final row in runtime.rows) CriticalNoticeRow.runtime(row),
  ]..sort((a, b) => b.observedUtc.compareTo(a.observedUtc));

  void _publish() {
    if (!current) return;
    var rows = _rows;
    while (rows.length > CriticalJobNoticeRing.maxRows) {
      final last = rows.last;
      if (last.job != null) {
        ring.dismiss(scope, last.id);
      } else {
        runtime.dismiss(last.runtime!);
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
    }
    ring.ingest(scope, frame, now().toUtc());
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
    ring.markRead(scope, id);
    _publish();
  }

  void dismiss(String id) {
    if (!current) return;
    ring.dismiss(scope, id);

    _publish();
  }

  void clear() {
    if (!current) return;
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
    _state.value = const CriticalNoticeSnapshot();
    await _eventSubscription?.cancel();
    await _runtimeSubscription?.cancel();
    await _state.close();
  }
}
