import 'dart:async';
import 'dart:math';

import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_route_models.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_route_port.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_saved_route_port.dart';
import 'package:flutter/foundation.dart';

/// One original UI owner and issued handle. No prepare/open/poll on construction.
final class AgentTunnelRouteController extends ChangeNotifier {
  AgentTunnelRouteController({
    required this.port,
    required this.scope,
    required this.intent,
    required this.original,
    DateTime Function()? now,
    this.elapsed,
  }) : _now = now ?? DateTime.now {
    if (port case final AgentTunnelSavedRoutePort saved) {
      _cachedMetadataVersion = saved.savedIntents.metadataVersion(scope, intent);
    }
  }
  final AgentTunnelRoutePort port;
  final RuntimeNoticeScope scope;
  final AgentTunnelRouteIntent intent;
  final bool Function() original;
  final DateTime Function() _now;
  final Duration Function()? elapsed;
  final Stopwatch _clock = Stopwatch();
  AgentTunnelRouteHandle? _handle;
  Future<void>? _preparing;
  Future<void>? _detaching;
  Future<AgentTunnelRouteCleanup?>? _closing;
  Future<AgentTunnelRouteCleanup>? _released;
  Timer? _timer;
  bool _disposed = false, _retired = false, _handleCancelled = false;
  int _generation = 0, _budgetMs = 0;
  bool busy = false,
      prepareSpent = false,
      approvalSpent = false,
      stopSpent = false,
      cancelRequested = false,
      expired = false,
      unknown = false,
      awaitingCleanup = false,
      auditLoading = false;
  Object? error, cleanupError, auditError;
  AgentTunnelRouteView? view;
  AgentTunnelRouteCleanup? cleanup;
  AgentTunnelAccessAudit? audit;
  bool attachSpent = false, noSavedOriginal = false, detachSpent = false;
  int? _cachedMetadataVersion;
  AgentTunnelDetachReceipt? detachedReceipt;
  bool get detached => detachedReceipt != null;
  bool get current => !_disposed && !_retired && original();
  bool get retired => _retired;
  bool get canPrepare => current && !busy && !prepareSpent && (!attachSpent || noSavedOriginal);
  bool get canAttachSaved => current && !busy && !prepareSpent && !attachSpent &&
      intent.resumeOriginal && port is AgentTunnelSavedRoutePort;
  bool get canDetach => current && !busy && !detachSpent && !expired && remainingMs > 0 &&
      _released == null && _handle is AgentTunnelDetachableRouteHandle &&
      view?.resumeOriginalSignedOptIn == true && view?.resumeTransportImplemented == true &&
      const {AgentTunnelRoutePhase.listening, AgentTunnelRoutePhase.connecting, AgentTunnelRoutePhase.connected}
          .contains(view?.phase);
  bool get canApprove =>
      current &&
      !busy &&
      !approvalSpent &&
      !expired &&
      remainingMs > 0 &&
      view?.phase == AgentTunnelRoutePhase.routeReview;
  bool get canStop =>
      current &&
      !detached &&
      _handle != null &&
      !busy &&
      !stopSpent &&
      remainingMs > 0 &&
      const {
        AgentTunnelRoutePhase.listening,
        AgentTunnelRoutePhase.connecting,
        AgentTunnelRoutePhase.connected,
      }.contains(view?.phase);
  int get remainingMs => (_budgetMs - (elapsed?.call() ?? _clock.elapsed).inMilliseconds).clamp(0, _budgetMs);
  int get remainingSeconds => (remainingMs + 999) ~/ 1000;
  bool _same(int generation) {
    if (!current) {
      retire();
      return false;
    }
    return generation == _generation;
  }

  void _emit() {
    if (!_disposed) notifyListeners();
  }

  void _cancelHandle(AgentTunnelRouteHandle h) {
    if (_handleCancelled) return;
    _handleCancelled = true;
    try {
      h.cancel();
    } catch (e) {
      cleanupError = e;
      unknown = true;
    }
  }

  Future<AgentTunnelRouteCleanup> _release(AgentTunnelRouteHandle h) {
    final existing = _released;
    if (existing != null) return existing;
    final done = Completer<AgentTunnelRouteCleanup>();
    _released = done.future;
    unawaited(_releaseOwned(h, done));
    return done.future;
  }

  Future<void> _releaseOwned(AgentTunnelRouteHandle h, Completer<AgentTunnelRouteCleanup> done) async {
    awaitingCleanup = true;
    _cancelHandle(h);
    _emit();
    try {
      final observed = await h.joined();
      cleanup = observed;
      if (observed.state == AgentTunnelCleanupState.unknown) unknown = true;
      done.complete(observed);
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

  void _accept(AgentTunnelRouteView next) {
    final elapsedMs = (elapsed?.call() ?? _clock.elapsed).inMilliseconds;
    final nativeBudget = min(
      next.remainingMs,
      min(next.limits.maxTtlSeconds * 1000, max(0, next.grantExpiresUtc.difference(_now().toUtc()).inMilliseconds)),
    );
    _budgetMs = view == null ? nativeBudget : min(_budgetMs, max(0, elapsedMs) + nativeBudget);
    if (view == null) _clock.start();
    view = next;
    if (remainingMs == 0) {
      expired = true;
      retire();
      return;
    }
    _schedule();
  }

  void _schedule() {
    _timer?.cancel();
    if (!current || view == null) return;
    _timer = Timer(Duration(milliseconds: min(1000, max(1, remainingMs))), () {
      if (!current) {
        retire();
        return;
      }
      if (remainingMs == 0) {
        expired = true;
        retire();
        return;
      }
      _emit();
      _schedule(); // local countdown only, never a Native details poll.
    });
  }

  Future<void> prepare() {
    if (!canPrepare) return Future.value();
    prepareSpent = true;
    busy = true;
    error = null;
    final generation = _generation;
    final pending = _prepare(generation);
    _preparing = pending;
    _emit();
    return pending;
  }

  /// Explicit lookup only. None/error never invokes prepare or approve.
  Future<void> attachSaved() {
    if (!canAttachSaved) return Future.value();
    attachSpent = true;
    busy = true;
    error = null;
    final generation = _generation;
    final pending = _attachSaved(generation);
    _preparing = pending;
    _emit();
    return pending;
  }

  Future<void> _attachSaved(int generation) async {
    try {
      if (!_same(generation)) return;
      final h = await (port as AgentTunnelSavedRoutePort).attachSaved(scope, intent);
      if (h == null) {
        (port as AgentTunnelSavedRoutePort).savedIntents.forget(scope, intent, expectedVersion: _cachedMetadataVersion);
        if (_same(generation)) noSavedOriginal = true;
        return;
      }
      if (!_same(generation)) {
        await _release(h);
        return;
      }
      _handle = h;
      prepareSpent = true;
      approvalSpent = true; // Some is the already-approved SAME original.
      final observed = await h.details();
      if (!_same(generation)) {
        await _release(h);
        return;
      }
      if (!const {AgentTunnelRoutePhase.listening, AgentTunnelRoutePhase.connecting, AgentTunnelRoutePhase.connected}
          .contains(observed.phase) || !observed.resumeOriginalSignedOptIn) {
        throw const FormatException('Saved original tunnel is no longer active');
      }
      _accept(observed);
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

  /// Only a validated explicit Native handoff can bypass ordinary cancel/join.
  Future<void> detachAutoOpen() {
    if (!canDetach) return Future.value();
    detachSpent = true;
    busy = true;
    error = null;
    final generation = _generation;
    final pending = _detachAutoOpen(generation);
    _detaching = pending;
    _emit();
    return pending;
  }

  Future<void> _detachAutoOpen(int generation) async {
    final h = _handle! as AgentTunnelDetachableRouteHandle, observed = view!;
    try {
      if (!_same(generation)) return;
      final receipt = await h.detachAutoOpen(observed);
      if (!_same(generation)) {
        await _release(h);
        return;
      }
      final elapsedMs = (elapsed?.call() ?? _clock.elapsed).inMilliseconds;
      _budgetMs = min(_budgetMs, max(0, elapsedMs) + receipt.remainingMs);
      if (!identical(receipt.original, observed) || remainingMs == 0 ||
          !_now().toUtc().isBefore(observed.grantExpiresUtc)) {
        throw const FormatException('Original tunnel handoff expired or changed');
      }
      // No await between final guards and release of this exact UI proxy.
      h.dispose();
      _handle = null;
      detachedReceipt = receipt;
      if (port case final AgentTunnelSavedRoutePort saved) {
        _cachedMetadataVersion = saved.savedIntents.retainValidated(scope, intent, receipt, remainingMs);
      }
      _timer?.cancel();
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

  Future<void> _prepare(int generation) async {
    try {
      if (!_same(generation)) return;
      final h = await port.prepare(scope, intent);
      if (!_same(generation)) {
        await _release(h);
        return;
      }
      _handle = h;
      final observed = await h.details();
      if (!_same(generation)) {
        await _release(h);
        return;
      }
      _accept(observed);
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

  Future<void> _action(Future<AgentTunnelRouteView> Function(AgentTunnelRouteHandle) action) async {
    final h = _handle;
    if (h == null || !current || busy) return;
    busy = true;
    error = null;
    final generation = _generation;
    _emit();
    try {
      if (!_same(generation)) return;
      final observed = await action(h);
      if (_same(generation)) _accept(observed);
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
    if (!current || busy || _handle == null) return Future.value();
    return _action((h) => h.details());
  }

  Future<void> stop() {
    if (!canStop) return Future.value();
    if (port case final AgentTunnelSavedRoutePort saved) {
      saved.savedIntents.forget(scope, intent, expectedVersion: _cachedMetadataVersion);
    }
    stopSpent = true;
    return _action((h) => h.stop());
  }

  Future<void> loadAudit() async {
    if (!current || busy || auditLoading) return;
    auditLoading = true;
    auditError = null;
    final generation = _generation;
    _emit();
    try {
      if (!_same(generation)) return;
      final observed = await port.audit(scope);
      if (_same(generation)) audit = observed;
    } catch (e) {
      if (_same(generation)) auditError = e;
    } finally {
      if (_same(generation)) {
        auditLoading = false;
        _emit();
      }
    }
  }

  /// Abort is separate from explicit authenticated owner Stop.
  void retire() {
    if (_retired) return;
    if (port case final AgentTunnelSavedRoutePort saved) {
      saved.savedIntents.forget(scope, intent, expectedVersion: _cachedMetadataVersion);
    }
    _retired = true;
    _generation++;
    cancelRequested = true;
    busy = false;
    auditLoading = false;
    _timer?.cancel();
    final h = _handle;
    if (h != null) {
      _cancelHandle(h);
      unawaited(_release(h).then<void>((_) {}, onError: (Object _, StackTrace _) {}));
    }
    _emit();
  }

  void cancel() => retire();

  /// A late prepare ACK is still retained and retired before this completes.
  Future<AgentTunnelRouteCleanup?> close() {
    final existing = _closing;
    if (existing != null) return existing;
    final done = Completer<AgentTunnelRouteCleanup?>();
    _closing = done.future;
    unawaited(_closeOwned(done));
    return done.future;
  }

  Future<void> _closeOwned(Completer<AgentTunnelRouteCleanup?> done) async {
    if (detached) {
      // The explicit validated handoff already disposed this UI proxy. No join
      // was observed and no cancellation is implied by dismissing that review.
      _timer?.cancel();
      done.complete(null);
      return;
    }
    awaitingCleanup = _preparing != null || _handle != null;
    retire();
    try {
      await _preparing;
      await _detaching;
    } catch (_) {
      /* Original cancellation already sticky. */
    }
    final h = _handle;
    try {
      if (h != null) {
        done.complete(await _release(h));
      } else if (_released != null) {
        done.complete(await _released);
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
    if (_disposed) return;
    _disposed = true;
    unawaited(close());
    super.dispose();
  }
}
