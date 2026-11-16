import 'dart:async';
import 'dart:math';

import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_revocation.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_status.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_view.dart';
import 'package:flutter/foundation.dart';

final class AgentCollectorServiceRevocationController extends ChangeNotifier {
  AgentCollectorServiceRevocationController({
    required this.port,
    required this.target,
    required this.original,
    this.elapsed,
  });
  final AgentCollectorServiceRevocationPort port;
  final AgentCollectorServiceStatus target;
  RuntimeNoticeScope get scope => target.scope;
  final bool Function() original;
  final Duration Function()? elapsed;
  AgentCollectorServiceRevocationHandle? _handle;
  AgentCollectorServiceRevocationView? view;
  AgentCollectorServiceRevocationCleanup? cleanup;
  AgentCollectorServiceReviewClock? _clock;
  Object? error;
  bool prepareSpent = false,
      confirmSpent = false,
      busy = false,
      cancelRequested = false,
      expired = false,
      unknown = false,
      awaitingCleanup = false,
      cleanupUnknown = false;
  bool _disposed = false, _retired = false, _cancelSent = false;
  int _generation = 0;
  Timer? _timer;
  Completer<void>? _pendingPreparation;
  Future<void>? _joining;
  bool get current => !_disposed && !_retired && original();
  int get remainingMs => _clock?.remainingMs ?? 0;
  int get remainingSeconds => (remainingMs + 999) ~/ 1000;
  bool get canPrepare => current && !prepareSpent && !busy && !cancelRequested;
  bool get canConfirm =>
      current &&
      !busy &&
      !confirmSpent &&
      !cancelRequested &&
      !expired &&
      remainingMs > 0 &&
      view?.phase == AgentCollectorServiceRevocationPhase.serviceRevokeReview &&
      _handle != null;
  bool get canRefresh => canConfirm;
  bool get localBlocked => view?.localBlocked == true || cleanup?.localBlocked == true;
  bool get remoteRevoked => view?.remoteRevoked == true || cleanup?.remoteRevoked == true;
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

  void _cancel(AgentCollectorServiceRevocationHandle handle) {
    if (_cancelSent) return;
    _cancelSent = true;
    try {
      handle.cancel();
    } catch (_) {
      cleanupUnknown = true;
    }
  }

  Future<void> _release({required bool cancel}) {
    final handle = _handle;
    if (handle == null) return Future.value();
    if (cancel) _cancel(handle);
    return _joining ??= _join(handle);
  }

  Future<void> _join(AgentCollectorServiceRevocationHandle handle) async {
    awaitingCleanup = true;
    _emit();
    try {
      final observed = await handle.joined();
      if (view?.localBlocked == true && !observed.localBlocked ||
          view?.remoteRevoked == true && !observed.remoteRevoked) {
        throw const FormatException('Service revocation cleanup changed');
      }
      cleanup = observed;
      cleanupUnknown |= observed.state == 'unknown';
    } catch (_) {
      cleanupUnknown = true;
    } finally {
      awaitingCleanup = false;
      if (identical(_handle, handle)) _handle = null;
      try {
        handle.dispose();
      } catch (_) {
        cleanupUnknown = true;
      }
      _emit();
    }
  }

  void _tick() {
    _timer?.cancel();
    if (!current) {
      retire();
      return;
    }
    if (confirmSpent || cancelRequested || _clock == null) return;
    if (remainingMs == 0) {
      expired = true;
      cancel();
      return;
    }
    _timer = Timer(Duration(milliseconds: min(1000, remainingMs)), () {
      _emit();
      _tick();
    });
  }

  void _accept(AgentCollectorServiceRevocationView value) {
    if (!identical(target, value.target) || view != null && !view!.sameOriginal(value)) {
      throw const FormatException('Original service revocation changed');
    }
    _clock!.acceptNativeRemaining(value.remainingMs);
  }

  Future<void> prepare() async {
    if (!canPrepare) return;
    prepareSpent = true;
    busy = true;
    _clock = AgentCollectorServiceReviewClock(elapsed: elapsed);
    final pending = Completer<void>();
    _pendingPreparation = pending;
    final generation = ++_generation;
    _tick();
    _emit();
    try {
      final issued = await port.prepareServiceRevocation(target);
      _handle = issued; // Retain before pending details; even late issuance is cleaned exactly.
      if (!_same(generation)) {
        await _release(cancel: true);
        return;
      }
      final value = await issued.details();
      if (!_same(generation)) return;
      _accept(value);
      if (value.phase != AgentCollectorServiceRevocationPhase.serviceRevokeReview) {
        throw const FormatException('Service review unavailable');
      }
      view = value;
      if (remainingMs == 0) {
        expired = true;
        cancel();
      }
    } catch (failure) {
      if (_handle == null) {
        cleanupUnknown = true;
        awaitingCleanup = false;
      }
      if (_same(generation)) {
        error = failure;
        await _release(cancel: true);
      }
    } finally {
      pending.complete();
      if (_handle == null) awaitingCleanup = false;
      if (_same(generation)) {
        busy = false;
        _tick();
        _emit();
      }
    }
  }

  Future<void> refreshReview() async {
    final handle = _handle;
    if (!canRefresh || handle == null) return;
    busy = true;
    final generation = _generation;
    _emit();
    try {
      final value = await handle.details();
      if (!_same(generation)) return;
      _accept(value);
      if (value.phase != AgentCollectorServiceRevocationPhase.serviceRevokeReview) {
        throw const FormatException('Service review changed');
      }
      view = value;
    } catch (failure) {
      if (_same(generation)) {
        error = failure;
        cancel();
      }
    } finally {
      if (_same(generation)) {
        busy = false;
        _tick();
        _emit();
      }
    }
  }

  Future<void> confirmRevocation() async {
    final handle = _handle, before = view;
    if (!canConfirm || handle == null || before == null) return;
    confirmSpent = true;
    busy = true;
    _timer?.cancel();
    final generation = _generation;
    _emit();
    try {
      final value = await handle.confirmRevocation();
      if (!_same(generation)) return;
      _accept(value);
      if (value.phase != AgentCollectorServiceRevocationPhase.revoked &&
          value.phase != AgentCollectorServiceRevocationPhase.unknown) {
        throw const FormatException('Service revocation outcome unknown');
      }
      view = value;
      unknown = value.phase == AgentCollectorServiceRevocationPhase.unknown;
      await _release(cancel: false);
    } catch (failure) {
      if (_same(generation)) {
        error = failure;
        unknown = true;
        await _release(cancel: true);
      }
    } finally {
      if (_same(generation)) {
        busy = false;
        _emit();
      }
    }
  }

  void cancel() {
    if (cancelRequested) return;
    cancelRequested = true;
    unknown |= confirmSpent && view?.phase != AgentCollectorServiceRevocationPhase.revoked;
    busy = false;
    _generation++;
    _timer?.cancel();
    if (_pendingPreparation != null && !_pendingPreparation!.isCompleted && _handle == null) awaitingCleanup = true;
    unawaited(_release(cancel: true));
    _emit();
  }

  void retire() {
    if (_retired) return;
    _retired = true;
    cancel();
  }

  Future<AgentCollectorServiceRevocationCleanup?> close() async {
    cancel();
    await _pendingPreparation?.future;
    await _release(cancel: true);
    return cleanup;
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
