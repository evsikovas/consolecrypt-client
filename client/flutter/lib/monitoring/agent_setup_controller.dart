import 'dart:async';

import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_association_port.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_port.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_view.dart';
import 'package:consolecrypt/monitoring/agent_setup_models.dart';
import 'package:consolecrypt/monitoring/agent_setup_port.dart';
import 'package:flutter/foundation.dart';

enum AgentSetupStep {
  selection,
  review,
  installing,
  offerReview,
  certificateReview,
  collectionReview,
  configuring,
  verifying,
  awaitingEvidence,
  complete,
  failed,
}

/// The software, owner and collector reviews are distinct explicit actions.
/// Progress events never grant permissions or manufacture readiness evidence.
final class AgentSetupController extends ChangeNotifier {
  AgentSetupController({required this.port, required this.isCurrent, this.initialHostId});
  final AgentSetupPort port;
  final bool Function() isCurrent;
  final String? initialHostId;
  AgentSetupContext? context;
  AgentSetupHost? host;
  AgentSetupProfile? profile;
  AgentSetupDestinationRef? destination;
  AgentSetupPlan? plan;
  AgentSoftwareEvidence? installed;
  AgentAssociationReview? pairingReview;
  AgentAssociationResult? paired;
  AgentCollectorServiceView? collectionReview;
  AgentSetupVerification? verification;
  AgentSetupProgress? progress;
  AgentSetupIssue? issue;
  AgentSetupStep step = AgentSetupStep.selection;
  bool busy = false, stopping = false;
  bool _cancelled = false;
  bool _disposed = false,
      _softwareSubmitted = false,
      _offerSubmitted = false,
      _certificateSubmitted = false,
      _collectionSubmitted = false;
  int _generation = 0;
  AgentSetupOperation? _operation;
  Future<AgentSetupOperation>? _pendingOperation;
  Future<AgentAssociationHandle>? _pendingPairing;
  Future<AgentCollectorServiceHandle>? _pendingCollection;
  AgentAssociationHandle? _pairing;
  AgentCollectorServiceHandle? _collection;
  StreamSubscription<AgentSetupProgress>? _progressSubscription;
  Future<AgentSetupCleanup>? _cleanup;
  Future<AgentSetupCleanup>? get cleanup => _cleanup;
  bool get current => !_disposed && isCurrent();
  bool get _working => current && !_cancelled && !stopping;
  bool get canPrepare => _working && !busy && context != null && host != null && profile?.available == true;
  bool get canInstall =>
      _working && !busy && step == AgentSetupStep.review && plan?.canProceed == true && !_softwareSubmitted;
  bool get canOpenGraphs => current && step == AgentSetupStep.complete && verification?.graphs != null;
  bool _accept(int generation) => current && generation == _generation;
  void _check(bool condition) {
    if (!condition) throw const AgentSetupFailure(AgentSetupIssue.contextChanged);
  }

  AgentSetupIssue _failure(Object failure) =>
      failure is AgentSetupFailure ? failure.issue : AgentSetupIssue.unavailable;

  Future<void> load() async {
    if (!current || busy || _operation != null) return;
    final generation = ++_generation;
    busy = true;
    issue = null;
    notifyListeners();
    try {
      final value = await port.context();
      if (!_accept(generation)) return;
      _check(
        value.hosts.length <= 10000 &&
            value.profiles.length <= 32 &&
            value.releases.length <= 32 &&
            value.destinations.length <= 128,
      );
      context = value;
      host = value.hosts.where((h) => h.id == initialHostId).firstOrNull;
      if (host == null && initialHostId == null && value.hosts.length == 1) host = value.hosts.single;
      profile = value.profiles.where((p) => p.available).firstOrNull;
      destination = value.destinations.where((d) => d.available).firstOrNull;
    } catch (e) {
      if (_accept(generation)) issue = _failure(e);
    } finally {
      if (_accept(generation)) {
        busy = false;
        notifyListeners();
      }
    }
  }

  void select({AgentSetupHost? host, AgentSetupProfile? profile, AgentSetupDestinationRef? destination}) {
    if (!current || busy || step != AgentSetupStep.selection) return;
    final original = context;
    if (original == null) return;
    if (host != null && original.hosts.contains(host)) this.host = host;
    if (profile != null && original.profiles.contains(profile) && profile.available) this.profile = profile;
    if (destination != null && original.destinations.contains(destination) && destination.available) {
      this.destination = destination;
    }
    issue = null;
    notifyListeners();
  }

  Future<void> prepare() async {
    if (!canPrepare || _operation != null) return;
    final selected = AgentSetupSelection(
      host: host!,
      profile: profile!,
      destination: profile!.requiresCollection ? destination : null,
    );
    final generation = ++_generation;
    busy = true;
    issue = null;
    notifyListeners();
    try {
      _pendingOperation = port.prepare(selected);
      final operation = await _pendingOperation!;
      if (!_accept(generation)) {
        if (_cleanup == null) {
          _operation = operation;
          _cleanup = operation.cancelAndJoin().catchError((Object _) => const AgentSetupCleanup(closedObserved: false));
        }
        return;
      }
      _operation = operation;
      final value = await operation.review();
      if (!_accept(generation)) return;
      _check(
        value.id.isNotEmpty &&
            identical(value.selection.host, selected.host) &&
            identical(value.selection.profile, selected.profile) &&
            identical(value.selection.destination, selected.destination) &&
            (!value.requiresCollection || selected.profile.requiresCollection),
      );
      if (value.canProceed) {
        _check(
          !selected.profile.requiresCollection ||
              selected.destination?.available == true && selected.destination!.receiverApprovalId.isNotEmpty,
        );
        _check(
          value.blocker == null &&
              (!value.requiresPairing ||
                  selected.profile.pairingPurpose != null && value.certificateLifetimeSeconds > 0),
        );
        if (value.requiresInstall) {
          final release = value.release;
          _check(
            release != null &&
                context!.releases.any(
                  (r) => r.id == release.id && r.version == release.version && r.platform == release.platform,
                ),
          );
        }
      }
      plan = value;
      step = AgentSetupStep.review;
      _progressSubscription = operation.progress.listen(
        (value) {
          if (_accept(generation)) {
            progress = value;
            notifyListeners();
          }
        },
        onError: (Object _) {
          if (_accept(generation)) {
            progress = null;
            notifyListeners();
          }
        },
        onDone: () {
          if (identical(_operation, operation)) _progressSubscription = null;
        },
      );
    } catch (e) {
      if (_accept(generation)) {
        issue = _failure(e);
        step = AgentSetupStep.failed;
      }
    } finally {
      if (_accept(generation)) {
        busy = false;
        notifyListeners();
      }
    }
  }

  void _softwareMatches(AgentSoftwareEvidence value) {
    final selected = plan!.selection;
    _check(value.hostId == selected.host.id && value.hostRevision == selected.host.revision);
    if (value.installed) _check(value.observedAt?.isUtc == true && value.selectedVersion?.isNotEmpty == true);
  }

  Future<void> install() async {
    if (!canInstall) return;
    _softwareSubmitted = true;
    await _run(() async {
      if (plan!.requiresInstall) {
        step = AgentSetupStep.installing;
        notifyListeners();
        final evidence = await _operation!.installSoftware();
        if (!_working) return;
        _softwareMatches(evidence);
        _check(evidence.installed && evidence.releaseId == plan!.release!.id);
        installed = evidence;
      }
      await _next();
    });
  }

  void _pairingMatches(AgentAssociationReview value, AgentAssociationReviewStage stage) {
    final selected = plan!.selection;
    _check(
      context!.scope.same(value.scope) &&
          value.hostId == selected.host.id &&
          value.hostRevision == selected.host.revision &&
          value.stage == stage &&
          value.purpose == selected.profile.pairingPurpose &&
          value.remainingMs > 0,
    );
    final previous = pairingReview;
    if (previous != null) {
      _check(
        value.agentId == previous.agentId &&
            value.nodeFingerprint == previous.nodeFingerprint &&
            value.ownerId == previous.ownerId &&
            value.clientIdentityId == previous.clientIdentityId,
      );
    }
  }

  Future<void> _next() async {
    if (!_working) return;
    if (plan!.requiresPairing && paired == null) {
      _pendingPairing = _operation!.preparePairing();
      _pairing = await _pendingPairing!;
      if (!_working) {
        unawaited(_close());
        return;
      }
      final review = await _pairing!.probe();
      if (!_working) return;
      _pairingMatches(review, AgentAssociationReviewStage.offer);
      pairingReview = review;
      step = AgentSetupStep.offerReview;
    } else if (plan!.requiresCollection && collectionReview == null) {
      _pendingCollection = _operation!.prepareCollection();
      _collection = await _pendingCollection!;
      if (!_working) {
        unawaited(_close());
        return;
      }
      final review = await _collection!.details();
      if (!_working) return;
      final selected = plan!.selection;
      _check(
        context!.scope.same(review.scope) &&
            review.record.hostId == selected.host.id &&
            review.record.hostRevision == selected.host.revision &&
            review.record.purpose == AgentAssociationPurpose.metrics &&
            review.phase == AgentCollectorServicePhase.serviceReview,
      );
      collectionReview = review;
      step = AgentSetupStep.collectionReview;
    } else {
      await _verify();
    }
  }

  Future<void> connect() async {
    if (!_working || busy || step != AgentSetupStep.offerReview || _offerSubmitted) return;
    _offerSubmitted = true;
    await _run(() async {
      final review = await _pairing!.approveOffer();
      if (!_working) return;
      _pairingMatches(review, AgentAssociationReviewStage.certificate);
      _check(plan!.certificateLifetimeSeconds <= review.maxCertificateLifetimeSeconds);
      pairingReview = review;
      step = AgentSetupStep.certificateReview;
    });
  }

  Future<void> confirmConnection() async {
    if (!_working || busy || step != AgentSetupStep.certificateReview || _certificateSubmitted) return;
    _certificateSubmitted = true;
    await _run(() async {
      final value = await _pairing!.approveCertificate(lifetimeSeconds: plan!.certificateLifetimeSeconds);
      if (!_working) return;
      _check(
        context!.scope.same(value.scope) &&
            value.agentId == pairingReview!.agentId &&
            value.purpose == plan!.selection.profile.pairingPurpose,
      );
      paired = value;
      await _next();
    });
  }

  Future<void> enableCollection() async {
    if (!_working || busy || step != AgentSetupStep.collectionReview || _collectionSubmitted) return;
    _collectionSubmitted = true;
    await _run(() async {
      step = AgentSetupStep.configuring;
      notifyListeners();
      final value = await _collection!.confirmConfiguration();
      if (!_working) return;
      _check(collectionReview!.sameOriginal(value) && value.phase == AgentCollectorServicePhase.configurationRecorded);
      collectionReview = value;
      await _verify();
    });
  }

  Future<void> verify() async {
    if (!_working || busy || step != AgentSetupStep.awaitingEvidence) return;
    await _run(_verify);
  }

  Future<void> _verify() async {
    step = AgentSetupStep.verifying;
    verification = null;
    notifyListeners();
    final value = await _operation!.verify();
    if (!_working) return;
    final selected = plan!.selection;
    _check(
      value.hostId == selected.host.id &&
          value.hostRevision == selected.host.revision &&
          value.profileId == selected.profile.id &&
          value.destinationId == selected.destination?.id,
    );
    _softwareMatches(value.software);
    final heartbeat = value.heartbeat, graphs = value.graphs;
    if (heartbeat != null) {
      _check(
        heartbeat.agentId.isNotEmpty &&
            heartbeat.version.isNotEmpty &&
            heartbeat.observedAt.isUtc &&
            (paired == null || heartbeat.agentId == paired!.agentId),
      );
    }
    if (graphs != null) {
      _check(
        graphs.querySourceId == selected.destination?.querySourceId &&
            graphs.observedAt.isUtc &&
            graphs.bindingIds.isNotEmpty &&
            graphs.bindingIds.length <= 16,
      );
    }
    verification = value;
    installed = value.software;
    final ready =
        value.software.installed &&
        value.pairingAcknowledged &&
        heartbeat != null &&
        (!selected.profile.requiresCollection || value.collectorObservedReady && graphs != null);
    step = ready ? AgentSetupStep.complete : AgentSetupStep.awaitingEvidence;
  }

  Future<void> _run(Future<void> Function() action) async {
    if (!_working || busy) return;
    final generation = _generation;
    busy = true;
    issue = null;
    notifyListeners();
    try {
      await action();
    } catch (e) {
      if (_accept(generation)) {
        issue = _failure(e);
        step = AgentSetupStep.failed;
      }
    } finally {
      if (_accept(generation)) {
        busy = false;
        notifyListeners();
      }
    }
  }

  Future<AgentSetupCleanup> cancel() async {
    if (!current || stopping) return const AgentSetupCleanup(closedObserved: false);
    ++_generation;
    _cancelled = true;
    stopping = true;
    verification = null;
    busy = true;
    notifyListeners();
    final cleanup = await _close();
    if (!current) return cleanup;
    issue = cleanup.closedObserved ? AgentSetupIssue.cancelled : AgentSetupIssue.cleanupUnknown;
    step = AgentSetupStep.failed;
    busy = false;
    stopping = false;
    if (!cleanup.closedObserved) _cleanup = null;
    notifyListeners();
    return cleanup;
  }

  Future<void> back() async {
    if (!current || busy || step != AgentSetupStep.review || _softwareSubmitted) return;
    busy = true;
    notifyListeners();
    final cleanup = await _close();
    if (!current) return;
    if (!cleanup.closedObserved) {
      issue = AgentSetupIssue.readinessNotObserved;
      step = AgentSetupStep.failed;
    } else {
      ++_generation;
      _operation = null;
      _pendingOperation = null;
      _cleanup = null;
      _progressSubscription = null;
      plan = null;
      progress = null;
      issue = null;
      step = AgentSetupStep.selection;
    }
    busy = false;
    notifyListeners();
  }

  Future<AgentSetupCleanup> _close() => _cleanup ??= _join();
  Future<AgentSetupCleanup> _join() async {
    try {
      final originalPairing = _pairing, originalCollection = _collection;
      originalPairing?.cancel();
      originalCollection?.cancel();
      // Begin parent cancellation before waiting for any child join: the
      // parent's original transport may be needed to retire its child worker.
      final originalOperation = _operation;
      var parent = originalOperation?.cancelAndJoin();
      // Factories may complete after cancellation. Retain and join their late
      // owned handles before reporting cleanup, rather than just cancelling.
      if (originalOperation == null && _pendingOperation != null) {
        try {
          parent = (await _pendingOperation!).cancelAndJoin();
        } catch (_) {
          /* parent factory refused */
        }
      }
      AgentAssociationHandle? pairing = _pairing;
      AgentCollectorServiceHandle? collector = _collection;
      if (pairing == null && _pendingPairing != null) {
        try {
          pairing = await _pendingPairing!;
        } catch (_) {
          /* parent joins refused child */
        }
      }
      if (collector == null && _pendingCollection != null) {
        try {
          collector = await _pendingCollection!;
        } catch (_) {
          /* parent joins refused child */
        }
      }
      if (!identical(pairing, originalPairing)) pairing?.cancel();
      if (!identical(collector, originalCollection)) collector?.cancel();
      final pair = pairing?.joined(), collection = collector?.joined();
      await _progressSubscription?.cancel();
      final original = await parent;
      final joinedPair = await pair, joinedCollection = await collection;
      return AgentSetupCleanup(
        closedObserved:
            (original?.closedObserved ?? true) &&
            (joinedPair == null || joinedPair.state == 'closedObserved') &&
            (joinedCollection == null || joinedCollection.state == 'closedObserved'),
      );
    } catch (_) {
      return const AgentSetupCleanup(closedObserved: false);
    }
  }

  @override
  void dispose() {
    _disposed = true;
    ++_generation;
    unawaited(_close());
    super.dispose();
  }
}
