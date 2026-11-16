import 'dart:async';

import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_rotation.dart';

import 'agent_rotation_fixture.dart' as f;

class RotationHandle implements AgentRotationHandle {
  RotationHandle(this.scope, this.record, {this.reconcile = false});
  final RuntimeNoticeScope scope;
  final AgentAssociationRecord record;
  final bool reconcile;
  int reads = 0, candidates = 0, confirms = 0, cancels = 0, joins = 0;
  Completer<AgentRotationDetails>? pendingDetails, pendingConfirm;
  Exception? candidateError, confirmError;
  AgentRotationDetails? confirmation;
  int candidateRemaining = 59000;
  @override
  Future<AgentRotationDetails> details() async {
    reads++;
    return pendingDetails?.future ??
        f.parsed(
          scope,
          record,
          phase: reconcile ? 'rotationReconcileReview' : 'rotationPreparationReview',
          reconcile: reconcile,
        );
  }

  @override
  Future<AgentRotationDetails> prepareCandidateOnce() async {
    candidates++;
    if (candidateError != null) throw candidateError!;
    return f.parsed(scope, record, phase: 'rotationCertificateReview', remaining: candidateRemaining);
  }

  @override
  Future<AgentRotationDetails> confirmOnce() async {
    confirms++;
    if (confirmError != null) throw confirmError!;
    return pendingConfirm?.future ??
        confirmation ??
        f.parsed(scope, record, phase: 'rotated', remaining: 58000, reconcile: reconcile);
  }

  @override
  void cancel() {
    cancels++;
  }

  @override
  Future<AgentRotationCleanup> joined() async {
    joins++;
    return const AgentRotationCleanup('unknown', 'closedObserved', false);
  }
}

class RotationPort implements AgentAssociationRotationPort {
  RotationPort(this.handle);
  final RotationHandle handle;
  int normal = 0, reconciles = 0;
  Completer<AgentRotationHandle>? pending;
  @override
  Future<AgentRotationHandle> prepareRotation(RuntimeNoticeScope scope, AgentAssociationRecord record) async {
    normal++;
    return pending?.future ?? handle;
  }

  @override
  Future<AgentRotationHandle> prepareRotationReconcile(RuntimeNoticeScope scope, AgentAssociationRecord record) async {
    reconciles++;
    return pending?.future ?? handle;
  }
}
