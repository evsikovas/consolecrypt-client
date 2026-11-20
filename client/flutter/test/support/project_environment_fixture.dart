import 'package:consolecrypt/automation/automation_service.dart';

// Synthetic UI contract, never actual Native/SSH acceptance or authority.
const profileId = '10000000-0000-4000-8000-000000000001';
const projectId = '20000000-0000-4000-8000-000000000001';
const environmentId = '30000000-0000-4000-8000-000000000001';
const hostId = '40000000-0000-4000-8000-000000000001';
const credentialId = '50000000-0000-4000-8000-000000000001';
const nonce = 'original opaque nonce ñ';
const project = AutomationProject(id: projectId, name: 'Owned project', readOnly: false, revision: 1);
AutomationJson frame({String profile = profileId, String scope = nonce, int revision = 1}) => {
  'version': 1,
  'profile_id': profile,
  'notification_scope_id': scope,
  'project_id': projectId,
  'project_revision': revision,
  'policy_sha256': 'a' * 64,
};
AutomationJson policy({String host = hostId, String credential = credentialId, bool production = false}) => {
  'version': 1,
  'environments': [
    {
      'id': environmentId,
      'name': 'Staging',
      'production': production,
      'overrides': [
        {'host_id': host, 'credential_id': credential},
      ],
    },
  ],
};
AutomationJson context({bool configured = true, String profile = profileId, String scope = nonce}) => {
  'version': 1,
  'frame': frame(profile: profile, scope: scope),
  'policy': configured ? policy() : null,
  'supported_credential_kinds': ['password', 'ssh_private_key', 'ssh_certificate'],
};
