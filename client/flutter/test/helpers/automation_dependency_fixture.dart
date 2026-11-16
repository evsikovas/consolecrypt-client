import 'dart:convert';

import 'package:consolecrypt/automation/automation_dependency_controller.dart';
import 'package:consolecrypt/automation/automation_dependency_models.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:flutter_test/flutter_test.dart';

const project = '11111111-1111-4111-8111-111111111111',
    runner = '22222222-2222-4222-8222-222222222222';
const operation = '33333333-3333-4333-8333-333333333333',
    reviewId = '44444444-4444-4444-8444-444444444444',
    transaction = '55555555-5555-4555-8555-555555555555';
final base = 'a' * 64,
    candidate = 'b' * 64,
    digest = 'c' * 64,
    archive = 'd' * 64;
final proposal = DependencyProposal(
  kind: 'role',
  name: 'demo',
  version: '1.2.3',
  url: 'https://fixtures.invalid/role.tar.gz',
  archiveSha256: archive,
);
String reply(String kind, AutomationJson data) =>
    jsonEncode({'version': 1, 'kind': kind, 'data': data});
AutomationJson source({bool supported = true}) => {
  'version': 1,
  'reviewId': reviewId,
  'operationId': operation,
  'projectId': project,
  'projectRevision': 1,
  'runnerId': runner,
  'runnerRevision': 1,
  'source': {
    'kind': 'role',
    'name': 'demo',
    'version': '1.2.3',
    'source': {'url': proposal.url, 'sha256': archive},
    'strip_components': 0,
  },
  'baseDigest': base,
  'oldLockSha256': null,
  'destination': 'roles/demo',
  'networkOrigin': 'https://fixtures.invalid',
  'publicationSupported': supported,
  'publicationReason': supported ? null : 'publication_location_unavailable',
  'expiresInSeconds': 300,
};
AutomationJson status([String phase = 'candidatePreview']) => {
  'version': 1,
  'operationId': operation,
  'phase': phase,
  'error': null,
  'cleanup': 'notStarted',
  'archiveVerified': true,
  'baseDigest': base,
  'candidateDigest': candidate,
  'reviewDigest': digest,
  'changeCount': 1,
  'changes': <AutomationJson>[],
  'changesTruncated': false,
};
AutomationJson evidence() => {
  'reviewDigest': digest,
  'baseDigest': base,
  'oldLockSha256': null,
  'candidateDigest': candidate,
  'candidateLockSha256': 'e' * 64,
  'candidateReceiptSha256': 'f' * 64,
  'archiveSha256': archive,
  'archiveTreeDigest': '0' * 64,
  'source': source()['source'],
  'destination': 'roles/demo',
  'archiveVerified': true,
  'receiptMergeSupported': false,
  'published': false,
  'executionEnabled': false,
};

class Service implements AutomationDependencyService {
  final requests = <AutomationJson>[];
  Future<String> Function(AutomationJson)? handler;
  bool supported = true;
  @override
  Future<String> dependencyAction(String raw) async {
    final frame = jsonDecode(raw) as AutomationJson;
    expect(frame.keys.toSet(), {'version', 'action'});
    expect(frame['version'], 1);
    final action = frame['action'] as AutomationJson;
    requests.add(action);
    if (handler != null) return handler!(action);
    return defaultReply(action);
  }

  String defaultReply(AutomationJson action) => switch (action['type']) {
    'review' => reply('sourceReview', source(supported: supported)),
    'start' || 'status' => reply('operation', status()),
    'evidence' => reply('evidence', evidence()),
    'accept' => reply('acceptedCandidate', {
      'operation': status('accepted'),
      'retainedCandidate': true,
      'published': false,
      'executionEnabled': false,
    }),
    'publish' => reply('published', {
      'version': 1,
      'operationId': operation,
      'projectId': project,
      'revision': 2,
      'transaction': transaction,
      'candidateDigest': candidate,
      'durability': 'WindowsFileFlushProcessRecovery',
    }),
    'cancel' => reply('cancelled', {
      'published': false,
      'executionEnabled': false,
    }),
    'changes' => reply('changes', {
      'review_digest': digest,
      'baseline_digest': base,
      'current_digest': candidate,
      'offset': action['offset'],
      'total': 1,
      'changes': <AutomationJson>[],
    }),
    'file' => reply('file', {
      'path': action['path'],
      'baseline_digest': base,
      'current_digest': candidate,
      'before': null,
      'after': '---\n',
      'preview': 'text',
    }),
    _ => throw StateError('unexpected test action'),
  };
}

DependencyInstallerController controller(
  Service service, {
  bool Function()? current,
  bool managed = true,
}) => DependencyInstallerController(
  client: DependencyActionClient(service),
  projectId: project,
  projectRevision: 1,
  runnerId: runner,
  runnerRevision: 1,
  managed: managed,
  isCurrent: current ?? () => true,
);
Future<void> ready(DependencyInstallerController c) async {
  await c.reviewSource(proposal);
  await c.download();
  expect(c.phase, DependencyInstallerPhase.candidateReview);
}
