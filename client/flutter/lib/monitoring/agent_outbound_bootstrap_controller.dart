import 'dart:async';
import 'dart:math';

import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_outbound_bootstrap.dart';
import 'package:flutter/foundation.dart';

/// One preparation and one approval on the exact issued Native review.
final class AgentOutboundBootstrapController extends ChangeNotifier {
  AgentOutboundBootstrapController({
    required this.port,
    required this.scope,
    required this.intent,
    required this.original,
    this.elapsed,
  });
  final AgentOutboundBootstrapPort port;
  final RuntimeNoticeScope scope;
  final AgentOutboundBootstrapIntent intent;
  final bool Function() original;
  final Duration Function()? elapsed;
  AgentOutboundBootstrapHandle? _handle;
  AgentOutboundBootstrapView? view;
  AgentOutboundBootstrapCleanup? cleanup;
  Object? error;
  bool prepared = false,
      approvalSpent = false,
      busy = false,
      cancelRequested = false,
      unknown = false,
      expired = false,
      awaitingCleanup = false;
  bool _retired = false, _disposed = false;
  int _generation = 0, _budget = 0;
  Stopwatch? _clock;
  Timer? _timer;
  Future<void>? _closing;
  Completer<void>? _pendingPreparation;
  bool get current => !_disposed && !_retired && original();
  int get remainingMs =>
      (_budget - (elapsed?.call() ?? _clock?.elapsed ?? Duration.zero).inMilliseconds).clamp(0, _budget);
  int get remainingSeconds => (remainingMs + 999) ~/ 1000;
  bool get canPrepare => current && !prepared && !busy && !cancelRequested;
  bool get canApprove =>
      current &&
      !busy &&
      !approvalSpent &&
      !cancelRequested &&
      !expired &&
      remainingMs > 0 &&
      view?.phase == 'bootstrapReview';
  void _emit() {
    if (!_disposed) notifyListeners();
  }

  bool _same(int g) {
    if (!current) {
      retire();
      return false;
    }
    return g == _generation;
  }

  Future<void> _release(AgentOutboundBootstrapHandle handle) async {
    try {
      handle.cancel();
    } catch (_) {
      /* Request failure is not a close witness. */
    }
    awaitingCleanup = true;
    try {
      cleanup = await handle.joined();
      unknown |= cleanup!.state == 'unknown';
    } catch (_) {
      unknown = true;
    } finally {
      awaitingCleanup = false;
      try {
        handle.dispose();
      } catch (_) {
        unknown = true;
      }
      _emit();
    }
  }

  Future<void> _close() {
    final existing = _closing;
    if (existing != null) return existing;
    final h = _handle;
    _handle = null;
    return _closing = h == null ? Future.value() : _release(h);
  }

  void _tick() {
    _timer?.cancel();
    if (!current) {
      retire();
      return;
    }
    if (approvalSpent || cancelRequested || view == null) return;
    if (remainingMs == 0) {
      expired = true;
      unawaited(_close());
      _emit();
      return;
    }
    _timer = Timer(Duration(milliseconds: min(1000, remainingMs)), () {
      _emit();
      _tick();
    });
  }

  Future<void> prepare() async {
    if (!canPrepare) return;
    final pending = Completer<void>();
    _pendingPreparation = pending;
    prepared = true;
    busy = true;
    final g = ++_generation;
    _emit();
    try {
      final issued = await port.prepare(scope, intent);
      if (!_same(g)) {
        await _release(issued);
        return;
      }
      _handle = issued; // Cancellation reaches Native even while async details is pending.
      final value = await issued.details();
      if (!_same(g)) return;
      if (!scope.same(value.scope) || !identical(value.intent, intent) || value.phase != 'bootstrapReview') {
        throw const FormatException('Original outbound review changed');
      }
      view = value;
      _budget = value.remainingMs;
      _clock = Stopwatch()..start();
      _tick();
    } catch (failure) {
      if (_same(g)) {
        error = failure;
        await _close();
      }
    } finally {
      pending.complete();
      if (_same(g)) {
        busy = false;
        _emit();
      }
    }
  }

  Future<void> approveOnce() async {
    final h = _handle, before = view;
    if (!canApprove || h == null || before == null) return;
    approvalSpent = true;
    busy = true;
    _timer?.cancel();
    final g = _generation;
    _emit();
    try {
      final result = await h.approveOnce();
      if (!_same(g)) return;
      if (!before.sameOriginal(result) || result.phase != 'configured') {
        throw const FormatException('Outbound registration result unknown');
      }
      view = result;
      // Approval's configured result is not an observed future carrier or Node admission.
      await _close();
    } catch (failure) {
      if (_same(g)) {
        error = failure;
        unknown = true;
        await _close();
      }
    } finally {
      if (_same(g)) {
        busy = false;
        _emit();
      }
    }
  }

  void cancel() {
    if (cancelRequested) return;
    cancelRequested = true;
    _generation++;
    busy = false;
    _timer?.cancel();
    unawaited(_close());
    _emit();
  }

  void retire() {
    if (_retired) return;
    _retired = true;
    cancel();
  }

  Future<void> close() async {
    cancel();
    await _pendingPreparation?.future;
    await _close();
  }

  @override
  void dispose() {
    if (_disposed) return;
    retire();
    _disposed = true;
    _timer?.cancel();
    super.dispose();
  }
}
