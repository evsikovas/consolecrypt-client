import 'dart:async';
import 'dart:math';

import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_association_port.dart';
import 'package:flutter/foundation.dart';

/// One original view owner and Native issued handle. No retry or new authority.
final class AgentAssociationController extends ChangeNotifier {
  AgentAssociationController({
    required this.port,
    required this.scope,
    required this.original,
    this.elapsedForReview,
  });
  final AgentAssociationPort port;
  final RuntimeNoticeScope scope;
  final bool Function() original;
  final Duration Function()? elapsedForReview;
  AgentAssociationHandle? _handle;
  AgentClientIdentityHandle? _identityHandle;
  AgentClientIdentityReview? identityReview;
  AgentClientIdentityResult? identityResult;
  AgentAssociationCleanup? cleanup;
  bool creationSpent = false,
      awaitingCleanup = false,
      cleanupUnavailable = false;
  bool _disposed = false, _retired = false;
  int _generation = 0, _reviewBudget = 0;
  Timer? _timer;
  Stopwatch? _clock;
  bool busy = false,
      offerSpent = false,
      certificateSpent = false,
      unknown = false,
      expired = false,
      cancelled = false;
  Object? error;
  AgentAssociationStatus? status;
  AgentAssociationHost? host;
  AgentAssociationReview? review;
  AgentAssociationResult? result;
  bool _same(int generation) {
    if (!current) {
      retire();
      return false;
    }
    return generation == _generation;
  }

  bool get current => !_disposed && !_retired && original();
  int get remainingMs =>
      (_reviewBudget -
              (elapsedForReview?.call() ?? _clock?.elapsed ?? Duration.zero)
                  .inMilliseconds)
          .clamp(0, _reviewBudget);
  int get remainingSeconds => (remainingMs + 999) ~/ 1000;
  bool get canApprove =>
      current &&
      !busy &&
      !expired &&
      !cancelled &&
      review != null &&
      remainingMs > 0 &&
      (review!.stage == AgentAssociationReviewStage.offer
          ? !offerSpent
          : !certificateSpent);
  void _emit() {
    if (!_disposed) notifyListeners();
  }

  void _cancel(AgentAssociationHandle? h) {
    try {
      h?.cancel();
    } catch (_) {
      /* Local view retirement remains sticky. */
    }
  }

  Future<void> _joined(
    Future<AgentAssociationCleanup> Function() joined,
  ) async {
    final generation = _generation;
    awaitingCleanup = true;
    try {
      final observed = await joined();
      if (_same(generation)) {
        cleanup = observed;
        cleanupUnavailable = false;
      }
    } catch (_) {
      if (_same(generation)) cleanupUnavailable = true;
    } finally {
      if (_same(generation)) {
        awaitingCleanup = false;
        _emit();
      }
    }
  }

  void _release() {
    final h = _handle, identity = _identityHandle;
    _handle = null;
    _identityHandle = null;
    _cancel(h);
    if (h != null) unawaited(_joined(h.joined));
    if (identity != null) {
      try {
        identity.cancel();
      } catch (_) {
        /* No success is inferred. */
      }
      unawaited(_joined(identity.joined));
    }
  }

  void _tick() {
    _timer?.cancel();
    if (!current ||
        review == null && identityReview == null ||
        remainingMs == 0) {
      if (!busy &&
          (review != null || identityReview != null) &&
          remainingMs == 0) {
        expired = true;
        unknown = offerSpent || creationSpent;
        review = null;
        identityReview = null;
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
        unknown = offerSpent || creationSpent;
        review = null;
        identityReview = null;
        _release();
      }
      _emit();
      _tick();
    });
  }

  void select(AgentAssociationHost value) {
    if (!current || busy || _handle != null || _identityHandle != null) return;
    if (status?.hosts.any((candidate) => identical(candidate, value)) != true) {
      return;
    }
    host = value;
    error = null;
    result = null;
    _emit();
  }

  Future<void> readStatus() async {
    if (!current || busy || _handle != null || _identityHandle != null) return;
    final generation = ++_generation;
    busy = true;
    error = null;
    result = null;
    identityResult = null;
    cleanup = null;
    cleanupUnavailable = false;
    _emit();
    try {
      final value = await port.status(scope);
      if (!scope.same(value.scope)) {
        throw const FormatException('Changed original scope');
      }
      if (_same(generation)) {
        status = value;
        host = null;
      }
    } catch (failure) {
      if (_same(generation)) {
        error = failure;
        status = null;
      }
    } finally {
      if (_same(generation)) {
        busy = false;
        _emit();
      }
    }
  }

  Future<void> probe() async {
    final selected = host;
    if (!current ||
        busy ||
        selected == null ||
        _handle != null ||
        status?.clientIdentityState !=
            AgentClientIdentityState.storedMetadata) {
      return;
    }
    final generation = ++_generation;
    busy = true;
    error = null;
    unknown = false;
    expired = false;
    cancelled = false;
    offerSpent = false;
    certificateSpent = false;
    result = null;
    _emit();
    AgentAssociationHandle? issued;
    try {
      issued = await port.registerProbe(scope, selected);
      if (!_same(generation)) {
        _cancel(issued);
        unawaited(_joined(issued.joined));
        return;
      }
      _handle = issued;
      final value = await issued.probe();
      if (!_same(generation)) return;
      _accept(value, AgentAssociationReviewStage.offer, selected);
      _reviewBudget = value.remainingMs;
      _clock = Stopwatch()..start();
      _tick();
    } catch (failure) {
      if (_same(generation)) {
        error = failure;
        review = null;
        identityReview = null;
        _release();
      }
    } finally {
      if (_same(generation)) {
        busy = false;
        _tick();
        _emit();
      }
    }
  }

  void _accept(
    AgentAssociationReview value,
    AgentAssociationReviewStage expected,
    AgentAssociationHost selected,
  ) {
    if (!scope.same(value.scope) ||
        value.stage != expected ||
        value.hostId != selected.id ||
        value.hostRevision != selected.revision) {
      throw const FormatException('Changed original review');
    }
    final prior = review;
    if (prior != null &&
        (value.purpose != prior.purpose ||
            value.metricsCollector?.collectorId != prior.metricsCollector?.collectorId ||
            value.metricsCollector?.manifestSha256 != prior.metricsCollector?.manifestSha256 ||
            value.agentId != prior.agentId ||
            value.nodeFingerprint != prior.nodeFingerprint ||
            value.ownerId != prior.ownerId ||
            value.ownerKind != prior.ownerKind ||
            value.clientIdentityId != prior.clientIdentityId ||
            value.certificateSha256 != prior.certificateSha256 ||
            value.nodePublicKey != prior.nodePublicKey ||
            value.nodeKeyEpoch != prior.nodeKeyEpoch ||
            value.ownerDeviceId != prior.ownerDeviceId ||
            value.ownerSigningKeySha256 != prior.ownerSigningKeySha256 ||
            value.nodeContextKind != prior.nodeContextKind ||
            value.nodeContextId != prior.nodeContextId ||
            value.nodeDeploymentId != prior.nodeDeploymentId)) {
      throw const FormatException('Changed original peer');
    }
    review = value;
  }

  Future<void> approve({int lifetimeSeconds = 86400}) async {
    final h = _handle, selected = host, accepted = review;
    if (!canApprove || h == null || selected == null || accepted == null) {
      return;
    }
    if (accepted.stage == AgentAssociationReviewStage.certificate &&
        (lifetimeSeconds < 1 ||
            lifetimeSeconds > accepted.maxCertificateLifetimeSeconds)) {
      return;
    }
    final generation = _generation;
    busy = true;
    error = null;
    if (accepted.stage == AgentAssociationReviewStage.offer) {
      offerSpent = true;
    } else {
      certificateSpent = true;
    }
    _emit();
    try {
      if (accepted.stage == AgentAssociationReviewStage.offer) {
        final value = await h.approveOffer();
        if (!_same(generation)) return;
        _accept(value, AgentAssociationReviewStage.certificate, selected);
        final elapsed =
            (elapsedForReview?.call() ?? _clock!.elapsed).inMilliseconds;
        _reviewBudget = min(
          _reviewBudget,
          elapsed + value.remainingMs,
        ); // Never restart or extend the original budget.
      } else {
        final value = await h.approveCertificate(
          lifetimeSeconds: lifetimeSeconds,
        );
        if (!_same(generation)) return;
        if (!scope.same(value.scope) ||
            value.purpose != accepted.purpose ||
            value.metricsCollector?.collectorId != accepted.metricsCollector?.collectorId ||
            value.metricsCollector?.manifestSha256 != accepted.metricsCollector?.manifestSha256 ||
            value.agentId != accepted.agentId ||
            value.associationId != accepted.associationId) {
          throw const FormatException('Changed result');
        }
        result = value;
        review = null;
        _timer?.cancel();
        _handle = null;
        unawaited(_joined(h.joined));
      }
    } catch (failure) {
      if (_same(generation)) {
        error = failure;
        unknown = true;
        review = null;
        identityReview = null;
        _release();
      }
    } finally {
      if (_same(generation)) {
        busy = false;
        _tick();
        _emit();
      }
    }
  }

  bool get canCreateIdentity =>
      current &&
      !busy &&
      !creationSpent &&
      identityReview != null &&
      remainingMs > 0 &&
      !cancelled &&
      !expired;
  Future<void> prepareIdentity() async {
    if (!current ||
        busy ||
        _handle != null ||
        _identityHandle != null ||
        status?.clientIdentityState != AgentClientIdentityState.missing ||
        identityResult != null) {
      return;
    }
    final generation = ++_generation;
    busy = true;
    error = null;
    expired = false;
    cancelled = false;
    creationSpent = false;
    unknown = false;
    cleanup = null;
    _emit();
    try {
      final issued = await port.prepareClientIdentity(scope);
      if (!_same(generation)) {
        try {
          issued.cancel();
        } catch (_) {}
        unawaited(_joined(issued.joined));
        return;
      }
      _identityHandle = issued;
      final accepted = await issued.details();
      if (!_same(generation)) return;
      if (!scope.same(accepted.scope)) {
        throw const FormatException('Changed original identity owner');
      }
      identityReview = accepted;
      _reviewBudget = accepted.remainingMs;
      _clock = Stopwatch()..start();
    } catch (failure) {
      if (_same(generation)) {
        error = failure;
        identityReview = null;
        _release();
      }
    } finally {
      if (_same(generation)) {
        busy = false;
        _tick();
        _emit();
      }
    }
  }

  Future<void> createIdentity() async {
    final issued = _identityHandle;
    if (!canCreateIdentity || issued == null) return;
    final generation = _generation;
    creationSpent = true;
    busy = true;
    error = null;
    _emit();
    try {
      final value = await issued.confirmCreation();
      if (!_same(generation)) return;
      if (!scope.same(value.scope)) {
        throw const FormatException('Changed original identity result');
      }
      identityResult = value;
      identityReview = null;
      _identityHandle = null;
      _timer?.cancel();
      unawaited(_joined(issued.joined));
    } catch (failure) {
      if (_same(generation)) {
        error = failure;
        unknown = true;
        identityReview = null;
        _release();
      }
    } finally {
      if (_same(generation)) {
        busy = false;
        _tick();
        _emit();
      }
    }
  }

  void cancel() {
    if (_disposed) return;
    _generation++;
    _timer?.cancel();
    _clock?.stop();
    _release();
    unknown = offerSpent || creationSpent;
    cancelled = true;
    review = null;
    identityReview = null;
    busy = false;
    _emit();
  }

  void retire() {
    if (_retired) return;
    _retired = true;
    cancel();
    status = null;
    result = null;
    identityResult = null;
    host = null;
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
