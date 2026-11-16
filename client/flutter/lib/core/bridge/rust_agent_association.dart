import 'dart:convert';

import 'package:consolecrypt/core/bridge/mapping.dart' show guard, toAppException;
import 'package:consolecrypt/core/security/vault_key_alarm.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_association_port.dart';
import 'package:consolecrypt/monitoring/agent_association_wire.dart';
import 'package:consolecrypt/monitoring/agent_rotation.dart';
import 'package:consolecrypt/src/rust/api/agent_node_association.dart' as native;

Map<String, Object> _frame(RuntimeNoticeScope scope) => {
  'version': 1,
  'profileId': scope.profileId,
  'notificationScopeId': scope.nonce,
};

void _cancel(void Function() original) {
  try {
    original();
  } catch (error) {
    final mapped = toAppException(error);
    reportVaultKeyMismatch(mapped);
    throw mapped;
  }
}

/// Lazy production adapter for the actual generated original-owner APIs.
/// Construction performs no RPC, probe, key creation or trust adoption.
final class RustAgentAssociationPort
    implements
        AgentAssociationPort,
        AgentAssociationRevocationPort,
        AgentAssociationRotationPort,
        AgentMetricsEnrollmentPort {
  const RustAgentAssociationPort();

  @override
  Future<AgentAssociationStatus> status(RuntimeNoticeScope scope) async => parseAgentAssociationStatus(
    await guard(() => native.agentNodeAssociationStatus(requestJson: jsonEncode(_frame(scope)))),
    scope,
  );

  @override
  Future<AgentAssociationHandle> registerProbe(RuntimeNoticeScope scope, AgentAssociationHost host) async {
    final original = await guard(
      () => native.agentNodeAssociationRegister(
        requestJson: jsonEncode({..._frame(scope), 'hostId': host.id, 'expectedHostRevision': host.revision}),
      ),
    );
    return _OriginalAssociation(original, scope, host);
  }

  @override
  Future<AgentAssociationHandle> registerMetricsProbe(RuntimeNoticeScope scope, AgentAssociationHost host) async =>
      _OriginalAssociation(
        await guard(
          () => native.agentMetricsNodeAssociationRegister(
            requestJson: jsonEncode({..._frame(scope), 'hostId': host.id, 'expectedHostRevision': host.revision}),
          ),
        ),
        scope,
        host,
        AgentAssociationPurpose.metrics,
      );
  @override
  Future<AgentRevocationHandle> prepareMetricsRevocation(
    RuntimeNoticeScope scope,
    AgentAssociationRecord record,
  ) async {
    if (record.purpose != AgentAssociationPurpose.metrics) throw const FormatException('Metrics association required');
    return _OriginalRevocation(
      await guard(
        () => native.agentMetricsNodeAssociationRevokePrepare(
          requestJson: jsonEncode({..._frame(scope), 'associationId': record.id}),
        ),
      ),
      scope,
      record,
    );
  }

  @override
  Future<AgentClientIdentityHandle> prepareClientIdentity(RuntimeNoticeScope scope) async => _OriginalClientIdentity(
    await guard(() => native.agentNodeClientIdentityPrepare(requestJson: jsonEncode(_frame(scope)))),
    scope,
  );
  @override
  Future<AgentRevocationHandle> prepareRevocation(RuntimeNoticeScope scope, AgentAssociationRecord record) async {
    if (record.purpose != null && record.purpose != AgentAssociationPurpose.tunnel) {
      throw const FormatException('Separate Metrics revocation required');
    }
    return _OriginalRevocation(
      await guard(
        () => native.agentNodeAssociationRevokePrepare(
          requestJson: jsonEncode({..._frame(scope), 'associationId': record.id}),
        ),
      ),
      scope,
      record,
    );
  }

  @override
  Future<AgentRotationHandle> prepareRotation(RuntimeNoticeScope scope, AgentAssociationRecord record) async {
    if (record.purpose != null && record.purpose != AgentAssociationPurpose.tunnel) {
      throw const FormatException('Metrics rotation unsupported');
    }
    return _OriginalRotation(
      await guard(
        () => native.agentNodeAssociationRotationPrepare(
          requestJson: jsonEncode({..._frame(scope), 'associationId': record.id}),
        ),
      ),
      scope,
      record,
    );
  }

  @override
  Future<AgentRotationHandle> prepareRotationReconcile(RuntimeNoticeScope scope, AgentAssociationRecord record) async {
    if (record.purpose != null && record.purpose != AgentAssociationPurpose.tunnel) {
      throw const FormatException('Metrics rotation unsupported');
    }
    return _OriginalRotation(
      await guard(
        () => native.agentNodeAssociationRotationReconcilePrepare(
          requestJson: jsonEncode({..._frame(scope), 'associationId': record.id}),
        ),
      ),
      scope,
      record,
    );
  }
}

/// All calls use this exact issued Native instance. No global Core recapture.
final class _OriginalAssociation implements AgentAssociationHandle {
  const _OriginalAssociation(this.original, this.scope, this.host, [this.purpose = AgentAssociationPurpose.tunnel]);
  final native.AgentNodeAssociationHandle original;
  final RuntimeNoticeScope scope;
  final AgentAssociationHost host;
  final AgentAssociationPurpose purpose;

  @override
  Future<AgentAssociationReview> probe() async => parseAgentAssociationReview(
    await guard(original.probe),
    scope,
    host,
    AgentAssociationReviewStage.offer,
    expectedPurpose: purpose,
  );

  @override
  Future<AgentAssociationReview> approveOffer() async => parseAgentAssociationReview(
    await guard(original.approveOffer),
    scope,
    host,
    AgentAssociationReviewStage.certificate,
    expectedPurpose: purpose,
  );

  @override
  Future<AgentAssociationResult> approveCertificate({required int lifetimeSeconds}) async =>
      parseAgentAssociationResult(
        await guard(() => original.approveCertificate(lifetimeSeconds: lifetimeSeconds)),
        scope,
        host,
        expectedPurpose: purpose,
      );

  @override
  void cancel() => _cancel(original.cancel);

  @override
  Future<AgentAssociationCleanup> joined() async => parseAgentAssociationCleanup(await guard(original.joined));
}

final class _OriginalClientIdentity implements AgentClientIdentityHandle {
  const _OriginalClientIdentity(this.original, this.scope);
  final native.AgentNodeClientIdentityHandle original;
  final RuntimeNoticeScope scope;

  @override
  Future<AgentClientIdentityReview> details() async =>
      parseAgentClientIdentityReview(await guard(original.details), scope);

  @override
  Future<AgentClientIdentityResult> confirmCreation() async =>
      parseAgentClientIdentityResult(await guard(original.confirmCreation), scope);

  @override
  void cancel() => _cancel(original.cancel);

  @override
  Future<AgentAssociationCleanup> joined() async => parseAgentAssociationCleanup(await guard(original.joined));
}

/// The actual issued revocation handle owns Native review/one-use authority.
/// The public record is only a strict original-response matching boundary.
final class _OriginalRevocation implements AgentRevocationHandle {
  const _OriginalRevocation(this.original, this.scope, this.record);
  final native.AgentNodeRevocationHandle original;
  final RuntimeNoticeScope scope;
  final AgentAssociationRecord record;
  @override
  Future<AgentRevocationDetails> details() async =>
      parseAgentRevocationDetails(await guard(original.details), scope, record);
  @override
  Future<AgentRevocationDetails> confirmOnce() async =>
      parseAgentRevocationDetails(await guard(original.confirmOnce), scope, record);
  @override
  void cancel() => _cancel(original.cancel);
  @override
  Future<AgentRevocationCleanup> joined() async => parseAgentRevocationCleanup(await guard(original.joined));
}

/// Exactly the issued Native handle; no later global Core/current-scope lookup.
final class _OriginalRotation implements AgentRotationHandle {
  const _OriginalRotation(this.original, this.scope, this.record);
  final native.AgentNodeRotationHandle original;
  final RuntimeNoticeScope scope;
  final AgentAssociationRecord record;
  @override
  Future<AgentRotationDetails> details() async =>
      parseAgentRotationDetails(await guard(original.details), scope, record);
  @override
  Future<AgentRotationDetails> prepareCandidateOnce() async =>
      parseAgentRotationDetails(await guard(original.prepareCandidateOnce), scope, record);
  @override
  Future<AgentRotationDetails> confirmOnce() async =>
      parseAgentRotationDetails(await guard(original.confirmOnce), scope, record);
  @override
  void cancel() => _cancel(original.cancel);
  @override
  Future<AgentRotationCleanup> joined() async => parseAgentRotationCleanup(await guard(original.joined));
}

/// Reuse the exact issued association/revocation objects from a distinct factory.
AgentAssociationHandle originalRustAgentAssociationHandle(
  native.AgentNodeAssociationHandle raw,
  RuntimeNoticeScope scope,
  AgentAssociationHost host,
  AgentAssociationPurpose purpose,
) => _OriginalAssociation(raw, scope, host, purpose);
AgentRevocationHandle originalRustAgentRevocationHandle(
  native.AgentNodeRevocationHandle raw,
  RuntimeNoticeScope scope,
  AgentAssociationRecord record,
) => _OriginalRevocation(raw, scope, record);
