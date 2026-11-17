import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';

/// Public Native-selected references, never executable paths, keys or authority.
final class AgentSetupHost {
  const AgentSetupHost({required this.id, required this.revision, required this.name, required this.address});
  final String id, revision, name, address;
}

final class AgentSetupProfile {
  const AgentSetupProfile({
    required this.id,
    required this.name,
    required this.description,
    required this.requiresCollection,
    this.pairingPurpose,
    this.available = true,
  });
  final String id, name, description;
  final bool requiresCollection, available;

  /// Only an actual supported existing enrollment factory may supply this.
  final AgentAssociationPurpose? pairingPurpose;
}

final class AgentSetupReleaseRef {
  const AgentSetupReleaseRef({required this.id, required this.version, required this.platform});
  final String id, version, platform;
}

final class AgentSetupDestinationRef {
  const AgentSetupDestinationRef({
    required this.id,
    required this.name,
    required this.receiverApprovalId,
    this.querySourceId,
    this.available = true,
  });
  final String id, name, receiverApprovalId;
  final String? querySourceId;
  final bool available;
}

final class AgentSetupContext {
  AgentSetupContext({
    required this.scope,
    required List<AgentSetupHost> hosts,
    required List<AgentSetupProfile> profiles,
    required List<AgentSetupReleaseRef> releases,
    required List<AgentSetupDestinationRef> destinations,
  }) : hosts = List.unmodifiable(hosts),
       profiles = List.unmodifiable(profiles),
       releases = List.unmodifiable(releases),
       destinations = List.unmodifiable(destinations);
  final List<AgentSetupHost> hosts;
  final RuntimeNoticeScope scope;
  final List<AgentSetupProfile> profiles;

  /// Already independently approved releases. An empty list is not permission
  /// to accept a key or an unsigned archive from the target machine.
  final List<AgentSetupReleaseRef> releases;
  final List<AgentSetupDestinationRef> destinations;
}

final class AgentSetupSelection {
  const AgentSetupSelection({required this.host, required this.profile, this.destination});
  final AgentSetupHost host;
  final AgentSetupProfile profile;
  final AgentSetupDestinationRef? destination;
}

enum AgentSetupIssue {
  releaseUnavailable,
  unsupportedHost,
  insufficientPrivileges,
  policyConflict,
  destinationUnavailable,
  ownerApprovalRequired,
  collectionUnavailable,
  readinessNotObserved,
  cleanupUnknown,
  contextChanged,
  cancelled,
  unavailable,
}

final class AgentSetupFailure implements Exception {
  const AgentSetupFailure(this.issue);
  final AgentSetupIssue issue;
}

enum AgentSetupPlannedAction {
  installAgent,
  createServiceAccount,
  pairDevice,
  configureCollection,
  reuseExporter,
  installExporter,
  useExistingDestination,
}

final class AgentSetupPlan {
  AgentSetupPlan({
    required this.id,
    required this.selection,
    required this.canProceed,
    required this.requiresInstall,
    required this.requiresPairing,
    required this.requiresCollection,
    required List<AgentSetupPlannedAction> actions,
    this.release,
    this.certificateLifetimeSeconds = 0,
    this.blocker,
  }) : actions = List.unmodifiable(actions);
  final String id;
  final AgentSetupSelection selection;
  final AgentSetupReleaseRef? release;
  final bool canProceed, requiresInstall, requiresPairing, requiresCollection;
  final int certificateLifetimeSeconds;
  final List<AgentSetupPlannedAction> actions;
  final AgentSetupIssue? blocker;
}

enum AgentSetupStage { software, pairing, collection, verification }

final class AgentSetupProgress {
  const AgentSetupProgress(this.stage);
  final AgentSetupStage stage;
}

/// File/service installation observation, separate from an authenticated running
/// version or connection to ConsoleCrypt. No PID/configuration implies readiness.
final class AgentSoftwareEvidence {
  const AgentSoftwareEvidence({
    required this.hostId,
    required this.hostRevision,
    required this.installed,
    this.releaseId,
    this.selectedVersion,
    this.observedAt,
  });
  final String hostId, hostRevision;
  final bool installed;
  final String? releaseId, selectedVersion;
  final DateTime? observedAt;
}

final class AgentSetupHeartbeatEvidence {
  const AgentSetupHeartbeatEvidence({required this.agentId, required this.version, required this.observedAt});
  final String agentId, version;
  final DateTime observedAt;
}

final class AgentSetupGraphEvidence {
  AgentSetupGraphEvidence({required this.querySourceId, required this.observedAt, required List<String> bindingIds})
    : bindingIds = List.unmodifiable(bindingIds);
  final String querySourceId;
  final DateTime observedAt;
  final List<String> bindingIds;
}

/// Native's fresh observations for the original target. ConfigurationRecorded
/// and local association metadata MUST NOT populate these readiness facts.
final class AgentSetupVerification {
  const AgentSetupVerification({
    required this.hostId,
    required this.hostRevision,
    required this.profileId,
    required this.software,
    required this.pairingAcknowledged,
    required this.collectorObservedReady,
    this.destinationId,
    this.heartbeat,
    this.graphs,
  });
  final String hostId, hostRevision, profileId;
  final String? destinationId;
  final AgentSoftwareEvidence software;
  final bool pairingAcknowledged, collectorObservedReady;
  final AgentSetupHeartbeatEvidence? heartbeat;
  final AgentSetupGraphEvidence? graphs;
}

final class AgentSetupCleanup {
  const AgentSetupCleanup({required this.closedObserved});
  final bool closedObserved;
}
