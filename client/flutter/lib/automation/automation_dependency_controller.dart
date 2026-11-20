import 'dart:async';

import 'package:consolecrypt/automation/automation_dependency_models.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:flutter/foundation.dart';

enum DependencyInstallerPhase {
  editing,
  reviewing,
  sourceReviewed,
  preparing,
  candidateReview,
  installing,
  cancelling,
  published,
  cancelled,
  failed,
  recoveryRequired,
}

/// UI state only, never a replacement for Native's one-use registry owner.
/// Every actual mutation uses the same captured IDs/digest; there is no retry,
/// alternate URL, global installation, command, credential or job dispatch.
final class DependencyInstallerController extends ChangeNotifier
    implements AutomationDiffService {
  DependencyInstallerController({
    required this.client,
    required this.projectId,
    required this.projectRevision,
    required this.runnerId,
    required this.runnerRevision,
    required this.isCurrent,
    required this.managed,
  });
  final DependencyActionClient client;
  final String projectId, runnerId;
  final int projectRevision, runnerRevision;
  final bool managed;
  final bool Function() isCurrent;
  DependencyInstallerPhase phase = DependencyInstallerPhase.editing;
  DependencyFailureCode? failure;
  NativeDependencyReview? review;
  DependencyCandidateEvidence? evidence;
  DependencyPublished? published;
  bool diffViewed = false;
  bool _disposed = false,
      _statusPending = false,
      _publishSent = false,
      _cancelRequested = false;
  int _epoch = 0, _polls = 0;
  bool get current => !_disposed && isCurrent();
  bool get busy => {
    DependencyInstallerPhase.reviewing,
    DependencyInstallerPhase.preparing,
    DependencyInstallerPhase.installing,
    DependencyInstallerPhase.cancelling,
  }.contains(phase);
  bool get canInstall =>
      current &&
      phase == DependencyInstallerPhase.candidateReview &&
      evidence != null &&
      diffViewed;
  bool _same(int epoch) => current && epoch == _epoch;
  void _notify() {
    if (!_disposed) notifyListeners();
  }

  Future<void> _discard(String? operation) async {
    if (operation == null) return;
    try {
      await client.call({
        'type': 'cancel',
        'operationId': operation,
      }, 'cancelled');
    } catch (_) {
      /* Native scope/TTL/owner still fences and joins cleanup. */
    }
  }

  void changed() {
    if (busy ||
        _disposed ||
        {
          DependencyInstallerPhase.published,
          DependencyInstallerPhase.recoveryRequired,
        }.contains(phase)) {
      return;
    }
    final old = review?.operationId;
    _epoch++;
    review = null;
    evidence = null;
    diffViewed = false;
    published = null;
    failure = null;
    _publishSent = false;
    _cancelRequested = false;
    phase = DependencyInstallerPhase.editing;
    unawaited(_discard(old));
    _notify();
  }

  Future<void> reviewSource(DependencyProposal proposal) async {
    if (!current ||
        busy ||
        !managed ||
        {
          DependencyInstallerPhase.published,
          DependencyInstallerPhase.recoveryRequired,
        }.contains(phase)) {
      return;
    }
    changed();
    final epoch = _epoch;
    phase = DependencyInstallerPhase.reviewing;
    _notify();
    try {
      final reply = await client.call({
        'type': 'review',
        'projectId': projectId,
        'projectRevision': projectRevision,
        'runnerId': runnerId,
        'runnerRevision': runnerRevision,
        'dependency': proposal.toJson(),
      }, 'sourceReview');
      final value = NativeDependencyReview.fromJson(reply.data, proposal);
      if (!_same(epoch)) {
        unawaited(_discard(value.operationId));
        return;
      }
      if (value.projectId != projectId ||
          value.projectRevision != projectRevision ||
          value.runnerId != runnerId ||
          value.runnerRevision != runnerRevision) {
        throw const DependencyInstallerFailure(DependencyFailureCode.changed);
      }
      review = value;
      phase = DependencyInstallerPhase.sourceReviewed;
    } catch (error) {
      if (_same(epoch)) _failed(error);
    }
    if (_same(epoch)) _notify();
  }

  Future<void> download() async {
    final selected = review;
    if (!current ||
        phase != DependencyInstallerPhase.sourceReviewed ||
        selected == null ||
        !selected.publicationSupported) {
      return;
    }
    final epoch = _epoch;
    phase = DependencyInstallerPhase.preparing;
    failure = null;
    _polls = 0;
    _notify();
    try {
      // Explicit action consumes ONLY captured Native review ID. No source URL
      // or network permission is reconstructed/sent again by the UI.
      final reply = await client.call({
        'type': 'start',
        'reviewId': selected.reviewId,
      }, 'operation');
      if (!_same(epoch)) {
        unawaited(_discard(selected.operationId));
        return;
      }
      await _observe(NativeDependencyOperation.fromJson(reply.data), epoch);
    } catch (error) {
      if (_same(epoch)) _failed(error);
    }
    if (_same(epoch)) _notify();
  }

  Future<void> poll() async {
    if (!current ||
        _statusPending ||
        !{
          DependencyInstallerPhase.preparing,
          DependencyInstallerPhase.candidateReview,
        }.contains(phase)) {
      return;
    }
    final selected = review;
    if (selected == null) return;
    if (++_polls > 300) {
      await cancel();
      return;
    }
    final epoch = _epoch;
    _statusPending = true;
    try {
      final reply = await client.call({
        'type': 'status',
        'operationId': selected.operationId,
      }, 'operation');
      if (_same(epoch)) {
        await _observe(NativeDependencyOperation.fromJson(reply.data), epoch);
      }
    } catch (error) {
      if (_same(epoch)) _failed(error);
    } finally {
      _statusPending = false;
      if (_same(epoch)) _notify();
    }
  }

  Future<void> _observe(NativeDependencyOperation operation, int epoch) async {
    final selected = review!;
    if (operation.operationId != selected.operationId) {
      throw const DependencyInstallerFailure(DependencyFailureCode.changed);
    }
    if (operation.phase == 'candidatePreview') {
      if (!operation.archiveVerified ||
          operation.reviewDigest == null ||
          operation.candidateDigest == null) {
        throw const DependencyInstallerFailure(
          DependencyFailureCode.invalidResponse,
        );
      }
      if (evidence == null) {
        final reply = await client.call({
          'type': 'evidence',
          'operationId': selected.operationId,
          'reviewDigest': operation.reviewDigest,
        }, 'evidence');
        if (!_same(epoch)) return;
        evidence = DependencyCandidateEvidence.fromJson(
          reply.data,
          selected,
          operation,
        );
      } else if (evidence!.reviewDigest != operation.reviewDigest ||
          evidence!.candidateDigest != operation.candidateDigest) {
        throw const DependencyInstallerFailure(DependencyFailureCode.changed);
      }
      phase = DependencyInstallerPhase.candidateReview;
    } else if (!{
      'opening',
      'preparing',
      'archiveVerified',
    }.contains(operation.phase)) {
      if (operation.phase == 'recoveryRequired') {
        throw const DependencyInstallerFailure(
          DependencyFailureCode.recoveryRequired,
        );
      }
      if (operation.error == 'dependency_publication_unsupported') {
        throw const DependencyInstallerFailure(
          DependencyFailureCode.unsupported,
        );
      }
      throw const DependencyInstallerFailure(DependencyFailureCode.changed);
    }
  }

  void markDiffViewed() {
    if (current &&
        phase == DependencyInstallerPhase.candidateReview &&
        evidence != null) {
      diffViewed = true;
      _notify();
    }
  }

  Future<void> install() async {
    if (!canInstall) return;
    final selected = review!, verified = evidence!;
    final epoch = ++_epoch; // Invalidates any earlier status/diff reply.
    _cancelRequested = false;
    phase = DependencyInstallerPhase.installing;
    failure = null;
    _notify();
    try {
      final accepted = await client.call({
        'type': 'accept',
        'operationId': selected.operationId,
        'reviewDigest': verified.reviewDigest,
      }, 'acceptedCandidate');
      if (!_same(epoch)) {
        unawaited(_discard(selected.operationId));
        return;
      }
      if (_cancelRequested) {
        phase = DependencyInstallerPhase.cancelled;
        unawaited(_discard(selected.operationId));
        _notify();
        return; // Cancel while accept is pending MUST NOT publish.
      }
      final acceptedOperation = accepted.data['operation'];
      if (accepted.data.length != 4 ||
          accepted.data['retainedCandidate'] != true ||
          accepted.data['published'] != false ||
          accepted.data['executionEnabled'] != false ||
          acceptedOperation is! Map<String, dynamic>) {
        throw const DependencyInstallerFailure(
          DependencyFailureCode.invalidResponse,
        );
      }
      final owner = NativeDependencyOperation.fromJson(acceptedOperation);
      if (owner.operationId != selected.operationId ||
          owner.phase != 'accepted' ||
          owner.reviewDigest != verified.reviewDigest ||
          owner.candidateDigest != verified.candidateDigest) {
        throw const DependencyInstallerFailure(DependencyFailureCode.changed);
      }
      // The SAME explicit Install action performs the prior Native one-use
      // acceptance then accepted-only publication. No approval boolean exists.
      _publishSent = true;
      final result = await client.call({
        'type': 'publish',
        'operationId': selected.operationId,
        'reviewDigest': verified.reviewDigest,
      }, 'published');
      if (!_same(epoch)) return;
      published = DependencyPublished.fromJson(result.data, selected, verified);
      phase = DependencyInstallerPhase.published;
    } catch (error) {
      if (_same(epoch)) {
        final code = error is DependencyInstallerFailure
            ? error.code
            : DependencyFailureCode.unavailable;
        // Unknown transport/scope outcome AFTER publication request cannot be
        // presented as a cancelled/rolled-back install or a retry suggestion.
        if (_publishSent &&
            !{
              DependencyFailureCode.busy,
              DependencyFailureCode.unsupported,
              DependencyFailureCode.changed,
            }.contains(code)) {
          failure = DependencyFailureCode.recoveryRequired;
          phase = DependencyInstallerPhase.recoveryRequired;
        } else {
          _failed(error);
        }
      }
    }
    if (_same(epoch)) _notify();
  }

  void _failed(Object error) {
    failure = error is DependencyInstallerFailure
        ? error.code
        : DependencyFailureCode.unavailable;
    phase = failure == DependencyFailureCode.recoveryRequired
        ? DependencyInstallerPhase.recoveryRequired
        : DependencyInstallerPhase.failed;
  }

  Future<void> cancel() async {
    if (_disposed ||
        phase == DependencyInstallerPhase.published ||
        phase == DependencyInstallerPhase.recoveryRequired) {
      return;
    }
    final operation = review?.operationId;
    if (phase == DependencyInstallerPhase.installing ||
        phase == DependencyInstallerPhase.cancelling) {
      _cancelRequested = true;
      phase = DependencyInstallerPhase.cancelling;
      _notify();
      await _discard(operation); // Publication response remains authoritative.
      return;
    }
    _epoch++;
    phase = DependencyInstallerPhase.cancelled;
    _notify();
    await _discard(operation);
  }

  void _diffContext(String id, String digest) {
    if (!current ||
        phase != DependencyInstallerPhase.candidateReview ||
        id != projectId ||
        evidence == null ||
        digest != evidence!.candidateDigest) {
      throw const DependencyInstallerFailure(DependencyFailureCode.changed);
    }
  }

  @override
  Future<AutomationJson> projectDiff(
    String id, {
    required String currentDigest,
    int offset = 0,
  }) async {
    _diffContext(id, currentDigest);
    final epoch = _epoch, verified = evidence!;
    final reply = await client.call({
      'type': 'changes',
      'operationId': review!.operationId,
      'reviewDigest': verified.reviewDigest,
      'offset': offset,
      'limit': 50,
    }, 'changes');
    _diffContext(id, currentDigest);
    if (!_same(epoch) ||
        reply.data['review_digest'] != verified.reviewDigest ||
        reply.data['baseline_digest'] != review!.baseDigest ||
        reply.data['current_digest'] != currentDigest) {
      throw const DependencyInstallerFailure(DependencyFailureCode.changed);
    }
    return reply.data; // Existing bounded AutomationDiffDialog renderer/paging.
  }

  @override
  Future<AutomationJson> projectFileDiff(
    String id,
    String path, {
    required String currentDigest,
  }) async {
    _diffContext(id, currentDigest);
    final epoch = _epoch, verified = evidence!;
    final reply = await client.call({
      'type': 'file',
      'operationId': review!.operationId,
      'reviewDigest': verified.reviewDigest,
      'path': path,
    }, 'file');
    _diffContext(id, currentDigest);
    if (!_same(epoch) ||
        reply.data['path'] != path ||
        reply.data['baseline_digest'] != review!.baseDigest ||
        reply.data['current_digest'] != currentDigest) {
      throw const DependencyInstallerFailure(DependencyFailureCode.changed);
    }
    return reply.data;
  }

  @override
  void dispose() {
    final old = review?.operationId;
    final terminal =
        phase == DependencyInstallerPhase.published ||
        phase == DependencyInstallerPhase.recoveryRequired;
    _disposed = true;
    _epoch++;
    if (!terminal) unawaited(_discard(old));
    super.dispose();
  }
}
