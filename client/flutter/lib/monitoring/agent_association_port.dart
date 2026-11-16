import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';

/// Optional typed UI seam. Production adapter awaits the actual Native factory.
/// No raw owner pin, certificate, private key, URL, command or permission input.
abstract interface class AgentAssociationCapability {
  AgentAssociationPort? get agentAssociationPort;
}

abstract interface class AgentAssociationPort {
  Future<AgentAssociationStatus> status(RuntimeNoticeScope scope);
  Future<AgentClientIdentityHandle> prepareClientIdentity(RuntimeNoticeScope scope);
  Future<AgentAssociationHandle> registerProbe(RuntimeNoticeScope scope, AgentAssociationHost host);
}

/// Distinct explicit MetricsOnly enrollment/revocation, never a Tunnel upgrade.
abstract interface class AgentMetricsEnrollmentPort {
  Future<AgentAssociationHandle> registerMetricsProbe(RuntimeNoticeScope scope, AgentAssociationHost host);
  Future<AgentRevocationHandle> prepareMetricsRevocation(RuntimeNoticeScope scope, AgentAssociationRecord record);
}

/// Reuses the existing two-stage invitation/approval UI lifecycle. Only the
/// register/revoke factory is different; caller supplies no scope or authority.
final class AgentMetricsEnrollmentAssociationPort implements AgentAssociationPort, AgentAssociationRevocationPort {
  const AgentMetricsEnrollmentAssociationPort(this.original, this.metrics);
  final AgentAssociationPort original;
  final AgentMetricsEnrollmentPort metrics;
  @override
  Future<AgentAssociationStatus> status(RuntimeNoticeScope scope) => original.status(scope);
  @override
  Future<AgentClientIdentityHandle> prepareClientIdentity(RuntimeNoticeScope scope) =>
      original.prepareClientIdentity(scope);
  @override
  Future<AgentAssociationHandle> registerProbe(RuntimeNoticeScope scope, AgentAssociationHost host) =>
      metrics.registerMetricsProbe(scope, host);
  @override
  Future<AgentRevocationHandle> prepareRevocation(RuntimeNoticeScope scope, AgentAssociationRecord record) =>
      metrics.prepareMetricsRevocation(scope, record);
}

abstract interface class AgentAssociationHandle {
  Future<AgentAssociationReview> probe();
  Future<AgentAssociationReview> approveOffer();
  Future<AgentAssociationResult> approveCertificate({required int lifetimeSeconds});
  void cancel();
  Future<AgentAssociationCleanup> joined();
}

abstract interface class AgentClientIdentityHandle {
  Future<AgentClientIdentityReview> details();
  Future<AgentClientIdentityResult> confirmCreation();
  void cancel();
  Future<AgentAssociationCleanup> joined();
}

/// Additive capability: older implementations keep revocation unavailable.
/// Native selects the saved original association/route; no caller pin/host/key.
abstract interface class AgentAssociationRevocationPort {
  Future<AgentRevocationHandle> prepareRevocation(RuntimeNoticeScope scope, AgentAssociationRecord record);
}

abstract interface class AgentRevocationHandle {
  Future<AgentRevocationDetails> details();
  Future<AgentRevocationDetails> confirmOnce();
  void cancel();
  Future<AgentRevocationCleanup> joined();
}

/// Separate fresh ExporterOnly factory; never converts Metrics/Tunnel grants.
abstract interface class AgentExporterEnrollmentPort {
  Future<AgentAssociationHandle> registerExporterProbe(RuntimeNoticeScope scope, AgentAssociationHost host);
  Future<AgentRevocationHandle> prepareExporterRevocation(RuntimeNoticeScope scope, AgentAssociationRecord record);
}

final class AgentExporterEnrollmentAssociationPort implements AgentAssociationPort, AgentAssociationRevocationPort {
  const AgentExporterEnrollmentAssociationPort(this.original, this.exporters);
  final AgentAssociationPort original;
  final AgentExporterEnrollmentPort exporters;
  @override
  Future<AgentAssociationStatus> status(RuntimeNoticeScope s) => original.status(s);
  @override
  Future<AgentClientIdentityHandle> prepareClientIdentity(RuntimeNoticeScope s) => original.prepareClientIdentity(s);
  @override
  Future<AgentAssociationHandle> registerProbe(RuntimeNoticeScope s, AgentAssociationHost h) =>
      exporters.registerExporterProbe(s, h);
  @override
  Future<AgentRevocationHandle> prepareRevocation(RuntimeNoticeScope s, AgentAssociationRecord r) =>
      exporters.prepareExporterRevocation(s, r);
}
