import 'dart:async';
import 'dart:math';

import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_rotation.dart';
import 'package:flutter/foundation.dart';

/// One original Native handle, one candidate and one confirmation; no retries.
final class AgentRotationController extends ChangeNotifier {
  AgentRotationController({
    required this.port,
    required this.scope,
    required this.record,
    required this.mode,
    required this.original,
    this.elapsedForReview,
  });
  final AgentAssociationRotationPort port;
  final RuntimeNoticeScope scope;
  final AgentAssociationRecord record;
  final AgentRotationMode mode;
  final bool Function() original;
  final Duration Function()? elapsedForReview;
  AgentRotationHandle? _handle;
  AgentRotationDetails? details, result;
  AgentRotationCleanup? cleanup;
  Object? error;
  bool busy = false,
      started = false,
      candidateSpent = false,
      confirmSpent = false,
      unknown = false,
      cancelRequested = false,
      expired = false,
      awaitingCleanup = false,
      cleanupUnknown = false;
  bool _disposed = false, _retired = false;
  int _generation = 0, _budget = 0;
  Stopwatch? _clock;
  Timer? _timer;
  bool get current => !_disposed && !_retired && original();
  int get _elapsed => (elapsedForReview?.call() ?? _clock?.elapsed ?? Duration.zero).inMilliseconds;
  int get remainingMs => (_budget - _elapsed).clamp(0, _budget);
  int get remainingSeconds => (remainingMs + 999) ~/ 1000;
  bool get available =>
      current && !busy && !unknown && !cancelRequested && !expired && remainingMs > 0 && _handle != null;
  bool get canPrepareCandidate =>
      available &&
      !candidateSpent &&
      !confirmSpent &&
      mode == AgentRotationMode.rotate &&
      details?.phase == 'rotationPreparationReview';
  bool get canConfirm =>
      available && !confirmSpent && {'rotationCertificateReview', 'rotationReconcileReview'}.contains(details?.phase);
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

  void _stop(AgentRotationHandle handle) {
    try {
      handle.cancel();
    } catch (_) {
      /* No close claim. */
    }
  }

  Future<void> _join(AgentRotationHandle handle) async {
    final g = _generation;
    awaitingCleanup = true;
    try {
      final value = await handle.joined();
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
        unknown = mode == AgentRotationMode.reconcile || candidateSpent || confirmSpent;
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
      if (remainingMs == 0 && !busy) {
        expired = true;
        unknown = mode == AgentRotationMode.reconcile || candidateSpent || confirmSpent;
        _release();
      }
      _emit();
      _tick();
    });
  }

  void _validate(AgentRotationDetails value) {
    if (!scope.same(value.scope) || !identical(value.record, record)) {
      throw const FormatException('Changed original rotation');
    }
  }

  void _cap(AgentRotationDetails value) {
    _budget = min(_budget, _elapsed + value.remainingMs);
  }

  Future<void> prepare() async {
    if (!current || busy || started || cancelRequested || expired) return;
    started = true;
    busy = true;
    final g = ++_generation;
    _emit();
    try {
      final issued = await (mode == AgentRotationMode.rotate
          ? port.prepareRotation(scope, record)
          : port.prepareRotationReconcile(scope, record));
      if (!_same(g)) {
        _stop(issued);
        unawaited(_join(issued));
        return;
      }
      _handle = issued; // Retain before pending async details; cancel is immediate.
      final value = await issued.details();
      if (!_same(g)) return;
      _validate(value);
      if (value.phase != (mode == AgentRotationMode.rotate ? 'rotationPreparationReview' : 'rotationReconcileReview')) {
        throw const FormatException('Rotation review unavailable');
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

  Future<void> prepareCandidate() async {
    final h = _handle, before = details;
    if (!canPrepareCandidate || h == null || before == null) return;
    candidateSpent = true;
    busy = true;
    final g = _generation;
    _emit();
    try {
      final value = await h.prepareCandidateOnce();
      if (!_same(g)) return;
      _validate(value);
      if (value.phase != 'rotationCertificateReview' ||
          !before.sameOwnerAndOriginal(value) ||
          value.actorRevision != before.actorRevision ||
          value.checkpoint != before.checkpoint ||
          value.certificateDigest != before.certificateDigest ||
          value.certificateExpiresUtc != before.certificateExpiresUtc ||
          value.selectionObserved ||
          value.localBlocked != true) {
        throw const FormatException('Changed rotation candidate');
      }
      details = value;
      _cap(value);
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

  Future<void> confirm() async {
    final h = _handle, before = details;
    if (!canConfirm || h == null || before == null) return;
    confirmSpent = true;
    busy = true;
    final g = _generation;
    _emit();
    try {
      final value = await h.confirmOnce();
      if (!_same(g)) return;
      _validate(value);
      final expectedIncrement = mode == AgentRotationMode.rotate ? BigInt.one : BigInt.zero;
      if (value.phase != 'rotated' ||
          !before.sameOwnerAndOriginal(value) ||
          value.rotationId != before.rotationId ||
          !before.proposedNode!.same(value.proposedNode) ||
          !before.proposedNode!.same(value.node) ||
          BigInt.parse(value.actorRevision) != BigInt.parse(before.actorRevision) + expectedIncrement ||
          BigInt.parse(value.checkpoint) != BigInt.parse(before.checkpoint) + expectedIncrement ||
          value.certificateExpiresUtc.isAfter(before.certificateExpiresUtc) ||
          !value.selectionObserved ||
          value.localBlocked != false ||
          mode == AgentRotationMode.reconcile && value.certificateDigest != before.certificateDigest) {
        throw const FormatException('Rotation selection not observed');
      }
      result = value;
      _timer?.cancel();
      _release();
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
    if (_disposed || cancelRequested) return;
    cancelRequested = true;
    unknown = result == null && (mode == AgentRotationMode.reconcile || candidateSpent || confirmSpent);
    _generation++;
    busy = false;
    _timer?.cancel();
    _release();
    _emit();
  }

  void retire() {
    if (_disposed || _retired) return;
    _retired = true;
    _generation++;
    _timer?.cancel();
    _release();
    _emit();
  }

  @override
  void dispose() {
    if (_disposed) return;
    _disposed = true;
    _retired = true;
    _generation++;
    _timer?.cancel();
    _release();
    super.dispose();
  }
}
