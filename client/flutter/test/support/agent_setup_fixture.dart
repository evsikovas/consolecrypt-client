import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_association_port.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_port.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_view.dart';
import 'package:consolecrypt/monitoring/agent_setup_models.dart';
import 'package:consolecrypt/monitoring/agent_setup_port.dart';

import 'agent_association_fixture.dart' as a;
import 'agent_collector_service_fixture.dart' as c;
import 'agent_metrics_fixture.dart' as m;

// Synthetic UI evidence only, not a signed release, Node ACK or live collector.
final host = AgentSetupHost(
  id: a.hostId,
  revision: a.selectedHost.revision,
  name: 'Saved server',
  address: '192.0.2.8',
);
const metricsProfile = AgentSetupProfile(
  id: 'linux_basic',
  name: 'Linux metrics',
  description: 'CPU and memory',
  requiresCollection: true,
  pairingPurpose: AgentAssociationPurpose.metrics,
);
const statusProfile = AgentSetupProfile(
  id: 'status',
  name: 'Existing Agent status',
  description: 'Status only',
  requiresCollection: false,
);
const release = AgentSetupReleaseRef(id: 'verified-test-release', version: '0.1.0', platform: 'linux-x86_64');
const destination = AgentSetupDestinationRef(
  id: 'central-metrics',
  name: 'Existing central metrics',
  receiverApprovalId: 'approved-receiver',
  querySourceId: 'existing-query-source',
);
final observedAt = DateTime.utc(2030);

class SetupPort implements AgentSetupPort {
  bool releaseAvailable = true, destinationAvailable = true, existingStatusOnly = false;
  final operations = <SetupOperation>[];
  Completer<AgentSetupOperation>? pendingPrepare;
  AgentSetupContext get choices => AgentSetupContext(
    scope: a.scope,
    hosts: [host],
    profiles: [existingStatusOnly ? statusProfile : metricsProfile],
    releases: releaseAvailable ? [release] : [],
    destinations: destinationAvailable ? [destination] : [],
  );
  @override
  Future<AgentSetupContext> context() async => choices;
  @override
  Future<AgentSetupOperation> prepare(AgentSetupSelection selection) async {
    if (pendingPrepare != null) return pendingPrepare!.future;
    final operation = SetupOperation(
      selection,
      releaseAvailable: releaseAvailable,
      destinationAvailable: destinationAvailable,
      statusOnly: existingStatusOnly,
    );
    operations.add(operation);
    return operation;
  }
}

class SetupOperation implements AgentSetupOperation {
  SetupOperation(
    this.selection, {
    this.releaseAvailable = true,
    this.destinationAvailable = true,
    this.statusOnly = false,
  });
  final AgentSetupSelection selection;
  final bool releaseAvailable, destinationAvailable, statusOnly;
  final calls = <String>[];
  final pairing = Pairing(), collection = Collection();
  bool ready = false;
  Completer<AgentSoftwareEvidence>? pendingInstall;
  Completer<AgentSetupVerification>? pendingVerification;
  Completer<AgentAssociationHandle>? pendingPairing;
  Completer<AgentSetupCleanup>? pendingCleanup;
  @override
  Stream<AgentSetupProgress> get progress => const Stream.empty();
  @override
  Future<AgentSetupPlan> review() async => AgentSetupPlan(
    id: 'native-original-plan',
    selection: selection,
    canProceed: statusOnly || releaseAvailable && destinationAvailable,
    requiresInstall: !statusOnly,
    requiresPairing: !statusOnly,
    requiresCollection: !statusOnly,
    release: releaseAvailable ? release : null,
    certificateLifetimeSeconds: statusOnly ? 0 : 60,
    blocker: !releaseAvailable && !statusOnly
        ? AgentSetupIssue.releaseUnavailable
        : !destinationAvailable && !statusOnly
        ? AgentSetupIssue.destinationUnavailable
        : null,
    actions: statusOnly
        ? []
        : [
            AgentSetupPlannedAction.installAgent,
            AgentSetupPlannedAction.pairDevice,
            AgentSetupPlannedAction.configureCollection,
            AgentSetupPlannedAction.reuseExporter,
          ],
  );
  AgentSoftwareEvidence software() => AgentSoftwareEvidence(
    hostId: host.id,
    hostRevision: host.revision,
    installed: true,
    releaseId: release.id,
    selectedVersion: release.version,
    observedAt: observedAt,
  );
  @override
  Future<AgentSoftwareEvidence> installSoftware() async {
    calls.add('install');
    return pendingInstall?.future ?? software();
  }

  @override
  Future<AgentAssociationHandle> preparePairing() async {
    calls.add('pair');
    return pendingPairing?.future ?? pairing;
  }

  @override
  Future<AgentCollectorServiceHandle> prepareCollection() async {
    calls.add('collection');
    return collection;
  }

  AgentSetupVerification evidence({bool? readyOverride}) {
    final complete = readyOverride ?? ready;
    return AgentSetupVerification(
      hostId: host.id,
      hostRevision: host.revision,
      profileId: selection.profile.id,
      destinationId: selection.destination?.id,
      software: software(),
      pairingAcknowledged: true,
      collectorObservedReady: complete && !statusOnly,
      heartbeat: complete
          ? AgentSetupHeartbeatEvidence(agentId: a.nodeId, version: release.version, observedAt: observedAt)
          : null,
      graphs: complete && !statusOnly
          ? AgentSetupGraphEvidence(
              querySourceId: destination.querySourceId!,
              observedAt: observedAt,
              bindingIds: ['verified-binding'],
            )
          : null,
    );
  }

  @override
  Future<AgentSetupVerification> verify() async {
    calls.add('verify');
    return pendingVerification?.future ?? evidence();
  }

  @override
  Future<AgentSetupCleanup> cancelAndJoin() async {
    calls.add('cancel');
    return pendingCleanup?.future ?? const AgentSetupCleanup(closedObserved: true);
  }
}

class Pairing implements AgentAssociationHandle {
  int offers = 0, certificates = 0, cancels = 0;
  AgentAssociationReview review({bool certificate = false}) => AgentAssociationReview(
    scope: a.scope,
    stage: certificate ? AgentAssociationReviewStage.certificate : AgentAssociationReviewStage.offer,
    hostId: host.id,
    hostRevision: host.revision,
    agentId: a.nodeId,
    nodeFingerprint: 'a' * 64,
    ownerKind: 'localProfile',
    ownerId: a.profileId,
    ownerEpoch: '1',
    checkpoint: '1',
    actorRevision: '1',
    clientIdentityId: a.identityId,
    certificateSha256: 'b' * 64,
    remainingMs: 60000,
    associationId: certificate ? a.associationId : null,
    claimDigest: certificate ? 'c' * 64 : null,
    maxCertificateLifetimeSeconds: certificate ? 300 : 0,
    purpose: AgentAssociationPurpose.metrics,
  );
  @override
  Future<AgentAssociationReview> probe() async => review();
  @override
  Future<AgentAssociationReview> approveOffer() async {
    offers++;
    return review(certificate: true);
  }

  @override
  Future<AgentAssociationResult> approveCertificate({required int lifetimeSeconds}) async {
    certificates++;
    return AgentAssociationResult(
      scope: a.scope,
      associationId: a.associationId,
      agentId: a.nodeId,
      certificateDigest: 'b' * 64,
      checkpoint: '1',
      actorAdmission: false,
      purpose: AgentAssociationPurpose.metrics,
    );
  }

  @override
  void cancel() {
    cancels++;
  }

  @override
  Future<AgentAssociationCleanup> joined() async => const AgentAssociationCleanup('cancelled', 'closedObserved');
}

class Collection implements AgentCollectorServiceHandle {
  final record = m.metricRecord();
  int confirms = 0, cancels = 0;
  @override
  Future<AgentCollectorServiceView> details() async =>
      parseAgentCollectorServiceView(jsonEncode(c.serviceWire()), a.scope, record);
  @override
  Future<AgentCollectorServiceView> confirmConfiguration() async {
    confirms++;
    return parseAgentCollectorServiceView(jsonEncode(c.serviceWire(phase: 'configurationRecorded')), a.scope, record);
  }

  @override
  void cancel() {
    cancels++;
  }

  @override
  Future<AgentCollectorServiceCleanup> joined() async =>
      const AgentCollectorServiceCleanup(AgentCollectorServicePhase.cancelled, 'closedObserved', false);
  @override
  void dispose() {}
}
