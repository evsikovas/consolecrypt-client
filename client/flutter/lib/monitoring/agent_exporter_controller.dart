import 'dart:async';

import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_exporter.dart';
import 'package:flutter/foundation.dart';

/// One explicit original operation. Timers only reduce the captured budget.
final class AgentExporterController extends ChangeNotifier {
  AgentExporterController({
    required this.port,
    required this.scope,
    required this.record,
    required this.original,
    DateTime Function()? now,
    this.elapsed,
  }) : _now = now ?? DateTime.now {
    _clock.start();
  }
  final AgentExporterPort port;
  final RuntimeNoticeScope scope;
  final AgentAssociationRecord record;
  final bool Function() original;
  final DateTime Function() _now;
  final Duration Function()? elapsed;
  final Stopwatch _clock = Stopwatch();
  AgentExporterCatalog? catalog;
  AgentExporterView? view;
  AgentExporterCleanup? cleanup;
  Object? error, cleanupError;
  AgentExporterHandle? _handle;
  Future<void>? _pending;
  Future<AgentExporterCleanup?>? _closing;
  Timer? _tick;
  bool busy = false, querySpent = false, prepareSpent = false, approvalSpent = false, stopSpent = false;
  bool retired = false, _disposed = false, unknown = false;
  int? _reviewEnd, _leaseEnd;
  int get _ms => (elapsed?.call() ?? _clock.elapsed).inMilliseconds;
  bool get current => !retired && original();
  int get remainingMs {
    final v = view;
    if (v == null) {
      return 0;
    }
    final end = approvalSpent ? _leaseEnd : _reviewEnd;
    final wall = v.originalExpiresUtc.difference(_now().toUtc()).inMilliseconds;
    return [end == null ? 0 : end - _ms, wall].reduce((a, b) => a < b ? a : b).clamp(0, 300000).toInt();
  }

  bool get closedObserved => cleanup?.closedObserved == true;
  bool get canRefresh => current && !busy && !closedObserved && _handle != null;
  bool get canQuery => !closedObserved && current && !busy && !querySpent && canRequestAgentExporter(record);
  bool get canPrepare => !closedObserved && current && !busy && !prepareSpent && !unknown && catalog != null;
  bool get canApprove =>
      !closedObserved &&
      current &&
      !busy &&
      !approvalSpent &&
      !unknown &&
      view?.phase == AgentExporterPhase.exporterReview &&
      remainingMs > 0;
  bool get canStop => !closedObserved && current && !busy && !stopSpent && view?.phase == AgentExporterPhase.running;
  void _notify() {
    if (!_disposed) {
      notifyListeners();
    }
  }

  void checkOriginal() {
    if (!original()) {
      retire();
    }
  }

  void retire() {
    if (retired) {
      return;
    }
    retired = true;
    try {
      if (!closedObserved) {
        _handle?.cancel();
      }
    } catch (e) {
      cleanupError = e;
      unknown = true;
    }
    _notify();
  }

  Future<void> _work(Future<void> Function() body) {
    busy = true;
    _notify();
    final task = Future<void>(() async {
      try {
        await body();
      } catch (e) {
        error = e;
        unknown = true;
        try {
          _handle?.cancel();
        } catch (c) {
          cleanupError = c;
        }
      } finally {
        busy = false;
        _notify();
      }
    });
    _pending = task;
    return task;
  }

  Future<void> readCatalog() async {
    if (!canQuery) {
      return;
    }
    querySpent = true;
    await _work(() async {
      final value = await port.catalog(scope, record);
      if (current) {
        catalog = value;
      }
    });
  }

  Future<void> prepare(int seconds) async {
    if (!canPrepare || seconds < 30 || seconds > 300 || seconds > catalog!.policy.maxSeconds) {
      return;
    }
    prepareSpent = true;
    final started = _ms;
    await _work(() async {
      final handle = await port.prepare(scope, record, catalog!, seconds);
      if (!current) {
        await _release(handle);
        return;
      }
      _handle = handle;
      try {
        final value = await handle.details();
        if (!current) {
          await _release(handle);
          _handle = null;
          return;
        }
        if (value.phase != AgentExporterPhase.exporterReview) {
          throw const FormatException('Exporter review required');
        }
        _leaseEnd = started + seconds * 1000;
        _accept(value, started + 60000);
        _tick = Timer.periodic(const Duration(milliseconds: 250), (_) {
          if (!current) {
            retire();
            return;
          }
          if (remainingMs == 0) {
            unknown = true;
            retire();
          }
          _notify();
        });
      } catch (_) {
        await _release(handle);
        _handle = null;
        rethrow;
      }
    });
  }

  void _accept(AgentExporterView value, [int? initialReviewEnd]) {
    final old = view;
    if (old != null &&
        (value.originalExpiresUtc != old.originalExpiresUtc || value.manifestSha256 != old.manifestSha256)) {
      throw const FormatException('Original exporter observation changed');
    }
    final remaining = value.reviewRemainingMs;
    if (remaining != null) {
      final candidate = _ms + remaining;
      final currentEnd = _reviewEnd ?? initialReviewEnd ?? candidate;
      _reviewEnd = candidate < currentEnd ? candidate : currentEnd;
    }
    view = value;
  }

  Future<void> approveOnce() async {
    if (!canApprove) {
      return;
    }
    approvalSpent = true;
    await _work(() async {
      try {
        final v = await _handle!.approveOnce();
        if (current) {
          _accept(v);
        }
      } catch (_) {
        try {
          _handle?.cancel();
        } catch (e) {
          cleanupError = e;
        }
        rethrow;
      }
    });
  }

  Future<void> refresh() async {
    if (!canRefresh) {
      return;
    }
    await _work(() async {
      final v = await _handle!.details();
      if (current) {
        _accept(v);
      }
    });
  }

  Future<void> stop() async {
    if (!canStop) {
      return;
    }
    stopSpent = true;
    await _work(() async {
      final v = await _handle!.stop();
      if (current) {
        _accept(v);
      }
    });
    // Stop is a request. Only joined/terminal facts can establish closure.
  }

  Future<void> observeCleanup() async {
    if (!current || busy || closedObserved || _handle == null) {
      return;
    }
    await _work(() async {
      final value = await _handle!.joined();
      cleanup = value;
      if (!value.closedObserved) {
        unknown = true;
      } else {
        _tick?.cancel();
      }
    });
  }

  Future<void> _release(AgentExporterHandle handle) async {
    try {
      handle.cancel();
    } catch (e) {
      cleanupError = e;
      unknown = true;
    }
    try {
      cleanup = await handle.joined();
      if (!cleanup!.closedObserved) {
        unknown = true;
      }
    } catch (e) {
      cleanupError = e;
      unknown = true;
    }
    try {
      handle.dispose();
    } catch (e) {
      cleanupError = e;
      unknown = true;
    }
  }

  Future<AgentExporterCleanup?> close() {
    final pending = _closing;
    if (pending != null) {
      return pending;
    }
    final done = Completer<AgentExporterCleanup?>();
    _closing = done.future;
    unawaited(_close().then(done.complete, onError: done.completeError));
    return done.future;
  }

  Future<AgentExporterCleanup?> _close() async {
    retire();
    _tick?.cancel();
    await _pending;
    final handle = _handle;
    _handle = null;
    if (handle != null) {
      if (closedObserved) {
        try {
          handle.dispose();
        } catch (e) {
          cleanupError = e;
          unknown = true;
        }
      } else {
        await _release(handle);
      }
    }
    _notify();
    return cleanup;
  }

  @override
  void dispose() {
    if (_disposed) {
      return;
    }
    retire();
    _tick?.cancel();
    unawaited(close());
    _disposed = true;
    super.dispose();
  }
}
