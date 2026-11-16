import 'dart:async';
import 'dart:math';

import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_association_port.dart';
import 'package:flutter/foundation.dart';

/// One original issued revocation review. No status refresh/retry or grants.
final class AgentRevocationController extends ChangeNotifier {
  AgentRevocationController({
    required this.port,
    required this.scope,
    required this.record,
    required this.original,
    this.elapsedForReview,
  });
  final AgentAssociationRevocationPort port;
  final RuntimeNoticeScope scope;
  final AgentAssociationRecord record;
  final bool Function() original;
  final Duration Function()? elapsedForReview;
  AgentRevocationHandle? _handle;
  AgentRevocationDetails? details, result;
  AgentRevocationCleanup? cleanup;
  Object? error;
  bool busy = false,
      spent = false,
      unknown = false,
      cancelRequested = false,
      expired = false,
      awaitingCleanup = false,
      cleanupUnknown = false;
  bool _disposed = false, _retired = false, _started = false;
  int _generation = 0, _budget = 0;
  Stopwatch? _clock;
  Timer? _timer;
  bool get current => !_disposed && !_retired && original();
  int get remainingMs =>
      (_budget - ((elapsedForReview?.call() ?? _clock?.elapsed ?? Duration.zero).inMilliseconds)).clamp(0, _budget);
  int get remainingSeconds => (remainingMs + 999) ~/ 1000;
  bool get canConfirm =>
      current &&
      !busy &&
      !spent &&
      !unknown &&
      !cancelRequested &&
      !expired &&
      remainingMs > 0 &&
      details?.phase == 'revocationReview' &&
      _handle != null;
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

  void _stop(AgentRevocationHandle h) {
    try {
      h.cancel();
    } catch (_) {
      /* No closure claim. */
    }
  }

  Future<void> _join(AgentRevocationHandle h) async {
    final g = _generation;
    awaitingCleanup = true;
    try {
      final value = await h.joined();
      if (_same(g)) {
        cleanup = value;
        cleanupUnknown = value.cleanupState == 'unknown';
      }
    } catch (_) {
      if (_same(g)) cleanupUnknown = true;
    } finally {
      if (_same(g)) {
        awaitingCleanup = false;
        _emit();
      }
    }
  }

  void _release() {
    final h = _handle;
    _handle = null;
    if (h != null) {
      _stop(h);
      unawaited(_join(h));
    }
  }

  void _tick() {
    _timer?.cancel();
    if (!current) {
      retire();
      return;
    }
    if (details == null || result != null || cancelRequested) return;
    if (remainingMs == 0) {
      if (!busy) {
        expired = true;
        unknown = spent;
        _release();
        _emit();
      }
      return;
    }
    _timer = Timer(Duration(milliseconds: min(1000, remainingMs)), () {
      if (!current) {
        retire();
        return;
      }
      // Native owns an already-dispatched confirmation. A display timer must
      // not cancel it or turn it into a fresh attempt.
      if (remainingMs == 0 && !busy) {
        expired = true;
        unknown = spent;
        _release();
      }
      _emit();
      _tick();
    });
  }

  void _validate(AgentRevocationDetails v) {
    if (!scope.same(v.scope) || !identical(v.record, record)) {
      throw const FormatException('Changed original revocation');
    }
  }

  Future<void> prepare() async {
    if (!current || busy || _started || cancelRequested || expired || spent) {
      return;
    }
    _started = true;
    final g = ++_generation;
    busy = true;
    _emit();
    try {
      final issued = await port.prepareRevocation(scope, record);
      if (!_same(g)) {
        _stop(issued);
        unawaited(_join(issued));
        return;
      }
      _handle = issued; // Retain BEFORE async details so cancel is immediate.
      final value = await issued.details();
      if (!_same(g)) return;
      _validate(value);
      if (value.phase != 'revocationReview' || value.localBlocked != false || value.remoteRevocationObserved) {
        throw const FormatException('Revocation review unavailable');
      }
      details = value;
      _budget = value.remainingMs;
      _clock = Stopwatch()..start();
    } catch (failure) {
      if (_same(g)) {
        error = failure;
        _release();
      }
    } finally {
      if (_same(g)) {
        busy = false;
        _tick();
        _emit();
      }
    }
  }

  bool _samePeer(AgentRevocationDetails a, AgentRevocationDetails b) =>
      a.nodePublicKey == b.nodePublicKey &&
      a.nodeKeyEpoch == b.nodeKeyEpoch &&
      a.ownerKind == b.ownerKind &&
      a.ownerId == b.ownerId &&
      a.ownerDeviceId == b.ownerDeviceId &&
      a.ownerSigningKeySha256 == b.ownerSigningKeySha256 &&
      a.nodeContextKind == b.nodeContextKind &&
      a.nodeContextId == b.nodeContextId &&
      a.nodeDeploymentId == b.nodeDeploymentId;
  Future<void> confirm() async {
    final h = _handle, review = details;
    if (!canConfirm || h == null || review == null) return;
    final g = _generation;
    spent = true;
    busy = true;
    error = null;
    _emit();
    try {
      final value = await h.confirmOnce();
      if (!_same(g)) return;
      _validate(value);
      if (!_samePeer(review, value) ||
          value.phase != 'revoked' ||
          value.localBlocked != true ||
          !value.remoteRevocationObserved ||
          BigInt.parse(value.actorRevision) != BigInt.parse(review.actorRevision) + BigInt.one ||
          BigInt.parse(value.checkpoint) != BigInt.parse(review.checkpoint) + BigInt.one) {
        throw const FormatException('Revocation outcome unknown');
      }
      result = value;
      _timer?.cancel();
      _handle = null;
      unawaited(_join(h));
    } catch (failure) {
      if (_same(g)) {
        error = failure;
        unknown = true;
        _release();
      }
    } finally {
      if (_same(g)) {
        busy = false;
        _tick();
        _emit();
      }
    }
  }

  void cancel() {
    if (_disposed) return;
    _generation++;
    cancelRequested = true;
    unknown = spent;
    busy = false;
    _timer?.cancel();
    _clock?.stop();
    _release();
    _emit();
  }

  void retire() {
    if (_retired) return;
    _retired = true;
    cancel();
    details = null;
    result = null;
  }

  @override
  void dispose() {
    if (_disposed) return;
    _retired = true;
    _timer?.cancel();
    _release();
    _disposed = true;
    super.dispose();
  }
}
