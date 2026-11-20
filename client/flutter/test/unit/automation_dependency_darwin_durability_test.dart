import 'package:consolecrypt/automation/automation_dependency_models.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:flutter_test/flutter_test.dart';

import '../helpers/automation_dependency_fixture.dart' as f;

void main() {
  final review = NativeDependencyReview.fromJson(f.source(), f.proposal);
  final evidence = DependencyCandidateEvidence.fromJson(
    f.evidence(),
    review,
    NativeDependencyOperation.fromJson(f.status()),
  );
  AutomationJson published() => {
    'version': 1,
    'operationId': f.operation,
    'projectId': f.project,
    'revision': 2,
    'transaction': f.transaction,
    'candidateDigest': f.candidate,
    'durability': 'MacosFileFullSyncDirectoryFsync',
  };

  test('Darwin durability is explicit and existing platform values remain valid', () {
    for (final value in [
      'MacosFileFullSyncDirectoryFsync',
      'WindowsFileFlushProcessRecovery',
      'LinuxDirectoryFsync',
    ]) {
      final raw = published()..['durability'] = value;
      expect(DependencyPublished.fromJson(raw, review, evidence).revision, 2);
    }
    final unknown = published()..['durability'] = 'future';
    expect(
      () => DependencyPublished.fromJson(unknown, review, evidence),
      throwsA(isA<DependencyInstallerFailure>()),
    );
  });

  test('Mac durability data cannot bypass original publication identity checks', () {
    for (final mismatch in <AutomationJson>[
      {'operationId': f.runner},
      {'projectId': f.runner},
      {'revision': 3},
      {'candidateDigest': f.base},
    ]) {
      final raw = published()..addAll(mismatch);
      expect(
        () => DependencyPublished.fromJson(raw, review, evidence),
        throwsA(isA<DependencyInstallerFailure>()),
      );
    }
  });
}
