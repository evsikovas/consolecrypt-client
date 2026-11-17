import 'package:consolecrypt/monitoring/agent_association_port.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_port.dart';
import 'package:consolecrypt/monitoring/agent_setup_models.dart';

abstract interface class AgentSetupCapability {
  AgentSetupPort? get agentSetupPort;
}

abstract interface class AgentSetupPort {
  Future<AgentSetupContext> context();

  /// Read-only preflight; Native retains the original session, exact target,
  /// release provenance, destination approval and immutable software review.
  Future<AgentSetupOperation> prepare(AgentSetupSelection selection);
}

abstract interface class AgentSetupOperation {
  Future<AgentSetupPlan> review();
  Stream<AgentSetupProgress> get progress;

  /// The sealed Native software review authorizes only the reviewed software
  /// action. It does not grant owner pairing or metric collection permission.
  Future<AgentSoftwareEvidence> installSoftware();
  Future<AgentAssociationHandle> preparePairing();
  Future<AgentCollectorServiceHandle> prepareCollection();
  Future<AgentSetupVerification> verify();

  /// Cancel/join only work owned by this operation, preserving completed
  /// installation and identities. Unknown cleanup is never a successful undo.
  Future<AgentSetupCleanup> cancelAndJoin();
}
