import 'dart:convert';

import 'package:consolecrypt/core/bridge/mapping.dart' show guard;
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_port.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_port.dart';
import 'package:consolecrypt/monitoring/agent_setup_models.dart';
import 'package:consolecrypt/monitoring/agent_setup_port.dart';
import 'package:consolecrypt/monitoring/agent_setup_wire.dart';
import 'package:consolecrypt/src/rust/api/agent_setup.dart' as native;

/// Scope is captured when the original unlocked service is created. It is never
/// refreshed to a newer profile when an installation dialog is still open.
final class RustAgentSetupPort implements AgentSetupPort {
  const RustAgentSetupPort(this.scope);
  final RuntimeNoticeScope scope;
  Map<String, Object> get _frame => {'version': 1, 'profileId': scope.profileId, 'notificationScopeId': scope.nonce};

  @override
  Future<AgentSetupContext> context() async =>
      parseAgentSetupContext(await guard(() => native.agentSetupContext(requestJson: jsonEncode(_frame))), scope);

  @override
  Future<AgentSetupOperation> prepare(AgentSetupSelection selection) async => _OriginalAgentSetup(
    await guard(
      () => native.agentSetupPrepare(
        requestJson: jsonEncode({
          ..._frame,
          'hostId': selection.host.id,
          'expectedHostRevision': selection.host.revision,
          'monitoringProfileId': selection.profile.id,
          if (selection.destination != null) 'destinationRef': selection.destination!.id,
        }),
      ),
    ),
    scope,
    selection,
  );
}

final class _OriginalAgentSetup implements AgentSetupOperation {
  const _OriginalAgentSetup(this.original, this.scope, this.selection);
  final native.AgentSetupHandle original;
  final RuntimeNoticeScope scope;
  final AgentSetupSelection selection;

  @override
  Future<AgentSetupPlan> review() async => parseAgentSetupPlan(await guard(original.review), scope, selection);
  @override
  Stream<AgentSetupProgress> get progress => const Stream.empty();
  @override
  Future<AgentSoftwareEvidence> installSoftware() async {
    // Native currently refuses this stage until a real release is available.
    // An unexpected acknowledgement is not installed software evidence.
    await guard(original.installSoftware);
    throw const AgentSetupFailure(AgentSetupIssue.releaseUnavailable);
  }

  @override
  Future<AgentAssociationHandle> preparePairing() async =>
      throw const AgentSetupFailure(AgentSetupIssue.ownerApprovalRequired);
  @override
  Future<AgentCollectorServiceHandle> prepareCollection() async =>
      throw const AgentSetupFailure(AgentSetupIssue.collectionUnavailable);
  @override
  Future<AgentSetupVerification> verify() async => throw const AgentSetupFailure(AgentSetupIssue.readinessNotObserved);
  @override
  Future<AgentSetupCleanup> cancelAndJoin() async {
    original.cancel();
    return parseAgentSetupCleanup(await guard(original.joined));
  }
}
