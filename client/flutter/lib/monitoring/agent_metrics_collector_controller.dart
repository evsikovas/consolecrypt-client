import 'dart:async';
import 'dart:math';

import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_metrics_collector_models.dart';
import 'package:consolecrypt/monitoring/agent_metrics_collector_port.dart';
import 'package:flutter/foundation.dart';

/// One explicit bounded collection session. Original owner/handle, no background renewal.
final class AgentMetricsCollectorController extends ChangeNotifier {
  AgentMetricsCollectorController({
    required this.port,
    required this.scope,
    required this.record,
    required this.original,
    this.durationSeconds = 300,
    this.elapsed,
    DateTime Function()? now,
  }) : _now = now ?? DateTime.now;
  final AgentMetricsCollectorPort port;
  final RuntimeNoticeScope scope;
  final AgentAssociationRecord record;
  final bool Function() original;
  final Duration Function()? elapsed;
  final DateTime Function() _now;
  final Stopwatch _clock = Stopwatch();
  int durationSeconds;
  AgentMetricsCollectorHandle? _handle;
  AgentMetricsCollectorIntent? _intent;
  Future<void>? _preparing;
  Future<AgentMetricsCollectorCleanup>? _releaseFuture;
  Future<AgentMetricsCollectorCleanup?>? _closing;
  Timer? _timer;
  int _generation = 0, _sessionBudget = 0, _reviewBudget = 0;
  bool _disposed = false, _retired = false, _cancelledHandle = false;
  bool busy = false,
      joining = false,
      prepareSpent = false,
      approvalSpent = false,
      stopSpent = false,
      cancelled = false,
      expired = false,
      unknown = false,
      awaitingCleanup = false;
  Object? error, cleanupError;
  AgentMetricsCollectorView? view;
  AgentMetricsCollectorCleanup? cleanup;
  bool get current => !_disposed && !_retired && original();
  bool get supported => record.canRequestMetricsCollection;
  bool get canPrepare => current && supported && !busy && !prepareSpent;
  bool get canApprove =>
      current &&
      !busy &&
      !joining &&
      !approvalSpent &&
      view?.phase == AgentMetricsCollectorPhase.collectorReview &&
      reviewRemainingMs > 0;
  bool get canStop =>
      current &&
      !busy &&
      !joining &&
      !stopSpent &&
      view?.phase == AgentMetricsCollectorPhase.running &&
      sessionRemainingMs > 0;
  int get _elapsedMs => (elapsed?.call() ?? _clock.elapsed).inMilliseconds;
  int get sessionRemainingMs => (_sessionBudget - _elapsedMs).clamp(0, _sessionBudget);
  int get reviewRemainingMs => min(sessionRemainingMs, (_reviewBudget - _elapsedMs).clamp(0, _reviewBudget));
  int get remainingSeconds =>
      ((view?.phase == AgentMetricsCollectorPhase.collectorReview ? reviewRemainingMs : sessionRemainingMs) + 999) ~/
      1000;
  void _emit() {
    if (!_disposed) notifyListeners();
  }

  bool _same(int generation) {
    if (!current) {
      retire();
      return false;
    }
    return generation == _generation;
  }

  void setDuration(int value) {
    if (!canPrepare || value < 30 || value > 300) return;
    durationSeconds = value;
    _emit();
  }

  void _cancelHandle(AgentMetricsCollectorHandle h) {
    if (_cancelledHandle) return;
    _cancelledHandle = true;
    try {
      h.cancel();
    } catch (e) {
      cleanupError = e;
      unknown = true;
    }
  }

  Future<AgentMetricsCollectorCleanup> _release(AgentMetricsCollectorHandle h) {
    final existing = _releaseFuture;
    if (existing != null) return existing;
    final done = Completer<AgentMetricsCollectorCleanup>();
    _releaseFuture = done.future;
    unawaited(_releaseOwned(h, done));
    return done.future;
  }

  Future<void> _releaseOwned(AgentMetricsCollectorHandle h, Completer<AgentMetricsCollectorCleanup> done) async {
    awaitingCleanup = true;
    _cancelHandle(h);
    _emit();
    try {
      final result = await h.joined();
      cleanup = result;
      if (result.state == AgentMetricsCleanupState.unknown || result.phase == AgentMetricsCollectorPhase.unknown) {
        unknown = true;
      }
      done.complete(result);
    } catch (e, stack) {
      cleanupError = e;
      unknown = true;
      done.completeError(e, stack);
    } finally {
      try {
        h.dispose();
      } catch (e) {
        cleanupError = e;
        unknown = true;
      }
      awaitingCleanup = false;
      _emit();
    }
  }

  void _accept(AgentMetricsCollectorView next) {
    final prior = view;
    if (prior != null &&
        (prior.profile != next.profile ||
            prior.collector.collectorId != next.collector.collectorId ||
            prior.collector.manifestSha256 != next.collector.manifestSha256 ||
            prior.ownerKind != next.ownerKind ||
            prior.ownerId != next.ownerId ||
            prior.ownerDeviceId != next.ownerDeviceId ||
            prior.ownerSigningKeySha256 != next.ownerSigningKeySha256 ||
            prior.nodePublicKey != next.nodePublicKey ||
            prior.nodeFingerprint != next.nodeFingerprint ||
            prior.nodeKeyEpoch != next.nodeKeyEpoch ||
            prior.originalExpiresUtc != next.originalExpiresUtc ||
            prior.leaseHash != null && next.leaseHash != prior.leaseHash)) {
      throw const FormatException('Original collector changed');
    }
    final observedBudget = min(
      _intent!.durationSeconds * 1000,
      max(0, next.originalExpiresUtc.difference(_now().toUtc()).inMilliseconds),
    );
    if (prior == null) {
      _sessionBudget = observedBudget;
      _reviewBudget = min(observedBudget, next.reviewRemainingMs ?? 0);
      _clock.start();
    } else {
      _sessionBudget = min(_sessionBudget, _elapsedMs + observedBudget);
      if (next.reviewRemainingMs != null) _reviewBudget = min(_reviewBudget, _elapsedMs + next.reviewRemainingMs!);
    }
    view = next;
    if (const {AgentMetricsCollectorPhase.stopped, AgentMetricsCollectorPhase.unknown}.contains(next.phase)) {
      _timer?.cancel();
      return;
    }
    if (sessionRemainingMs == 0 || !next.actorAdmission && reviewRemainingMs == 0) {
      expired = true;
      retire();
      return;
    }
    _schedule();
  }

  void _schedule() {
    _timer?.cancel();
    if (!current || view == null) return;
    final left = view!.actorAdmission ? sessionRemainingMs : reviewRemainingMs;
    _timer = Timer(Duration(milliseconds: min(1000, max(1, left))), () {
      if (!current) {
        retire();
        return;
      }
      if (sessionRemainingMs == 0 || view?.actorAdmission != true && reviewRemainingMs == 0) {
        expired = true;
        retire();
        return;
      }
      _emit();
      _schedule(); // display clock only; never Native polling/renewal.
    });
  }

  Future<void> prepare() {
    if (!canPrepare) return Future.value();
    _intent = AgentMetricsCollectorIntent(record, durationSeconds);
    prepareSpent = true;
    busy = true;
    error = null;
    final future = _prepare(_generation);
    _preparing = future;
    _emit();
    return future;
  }

  Future<void> _prepare(int generation) async {
    try {
      if (!_same(generation)) return;
      final h = await port.prepare(scope, _intent!);
      if (!_same(generation)) {
        await _release(h);
        return;
      }
      _handle = h;
      final result = await h.details();
      if (!_same(generation)) {
        await _release(h);
        return;
      }
      _accept(result);
    } catch (e) {
      if (_same(generation)) {
        error = e;
        unknown = true;
        retire();
      }
    } finally {
      if (_same(generation)) {
        busy = false;
        _emit();
      }
    }
  }

  Future<void> _action(Future<AgentMetricsCollectorView> Function(AgentMetricsCollectorHandle) action) async {
    final h = _handle;
    if (h == null || !current || busy || joining) return;
    busy = true;
    error = null;
    final generation = _generation;
    _emit();
    try {
      if (!_same(generation)) return;
      final result = await action(h);
      if (_same(generation)) _accept(result);
    } catch (e) {
      if (_same(generation)) {
        error = e;
        unknown = true;
        retire();
      }
    } finally {
      if (_same(generation)) {
        busy = false;
        _emit();
      }
    }
  }

  Future<void> approveOnce() {
    if (!canApprove) return Future.value();
    approvalSpent = true;
    return _action((h) => h.approveOnce());
  }

  Future<void> refresh() {
    if (!current || _handle == null || busy || joining) return Future.value();
    return _action((h) => h.details());
  }

  Future<void> stop() {
    if (!canStop) return Future.value();
    stopSpent = true;
    return _action((h) => h.stop());
  }

  /// Explicit observation only: does not cancel an active session or dispose.
  Future<void> observeCleanup() async {
    final h = _handle;
    if (h == null || !current || busy || joining) return;
    joining = true;
    cleanupError = null;
    final generation = _generation;
    _emit();
    try {
      if (!_same(generation)) return;
      final result = await h.joined();
      if (_same(generation)) {
        cleanup = result;
        if (result.state == AgentMetricsCleanupState.unknown || result.phase == AgentMetricsCollectorPhase.unknown) {
          unknown = true;
        }
      }
    } catch (e) {
      if (_same(generation)) cleanupError = e;
    } finally {
      if (_same(generation)) {
        joining = false;
        _emit();
      }
    }
  }

  void retire() {
    if (_retired) {
      return;
    }
    _retired = true;
    _generation++;
    cancelled = true;
    busy = false;
    joining = false;
    _timer?.cancel();
    final h = _handle;
    if (h != null) {
      _cancelHandle(h);
      unawaited(_release(h).then<void>((_) {}, onError: (Object _, StackTrace _) {}));
    }
    _emit();
  }

  void cancel() => retire();
  Future<AgentMetricsCollectorCleanup?> close() {
    final existing = _closing;
    if (existing != null) return existing;
    final done = Completer<AgentMetricsCollectorCleanup?>();
    _closing = done.future;
    unawaited(_closeOwned(done));
    return done.future;
  }

  Future<void> _closeOwned(Completer<AgentMetricsCollectorCleanup?> done) async {
    awaitingCleanup = _preparing != null || _handle != null;
    retire();
    try {
      await _preparing;
    } catch (_) {
      /* Same original cancellation remains sticky. */
    }
    try {
      final h = _handle;
      if (h != null) {
        done.complete(await _release(h));
      } else if (_releaseFuture != null) {
        done.complete(await _releaseFuture);
      } else {
        done.complete(cleanup);
      }
    } catch (_) {
      done.complete(null);
    } finally {
      awaitingCleanup = false;
      _emit();
    }
  }

  @override
  void dispose() {
    if (_disposed) {
      return;
    }
    _disposed = true;
    unawaited(close());
    super.dispose();
  }
}
