// Synthetic UI fixtures only. No release signature, target or installed proof.
import 'package:consolecrypt/automation/automation_service.dart';

const profileId = '17d8624a-66d8-4426-9dcf-088de74605d5';
const nonce = 'opaque original: \u0442\u0435\u0441\u0442';
const hostId = '99341cab-9d26-4b49-b798-91137b0a33d3';
const projectId = '8794c431-e72a-4f2d-b95e-d2623497eafd';
AutomationJson context() => {
  'version': 1,
  'profileId': profileId,
  'notificationScopeId': nonce,
  'hosts': [
    <String, dynamic>{
      'hostId': hostId,
      'hostRevision': '9007199254740993',
      'name': 'Existing SSH source target',
    },
  ],
};
AutomationJson created(AutomationJson request) => {
  'version': 1,
  'profileId': profileId,
  'notificationScopeId': nonce,
  'project': {
    'id': projectId,
    'name': request['name'],
    'source_kind': 'managed',
    'path': 'owned-fixture-project',
    'revision': 1,
    'read_only': false,
    'import_state': null,
    'import_error': null,
    'git_source': null,
  },
  'hostId': hostId,
  'hostRevision': '9007199254740993',
  'playbook': 'playbook.yml',
  'projectDigest': 'a' * 64,
  'actualRuntimeVersion': null,
  'claimedRelease':
      request['operation'] == 'install' || request['operation'] == 'update'
      ? {
          'artifactSha256': 'b' * 64,
          'artifactBytes': 1024,
          'manifestSha256': 'c' * 64,
          'signatureVerified': false,
        }
      : null,
};
