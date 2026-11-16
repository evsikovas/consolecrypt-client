import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/automation/automation_service_delegation.dart';

const serviceProfile = '10000000-0000-4000-8000-000000000001';
const serviceRunner = '20000000-0000-4000-8000-000000000002';
const serviceIdentity = '30000000-0000-4000-8000-000000000003';
const serviceProject = '40000000-0000-4000-8000-000000000004';
const serviceHost = '50000000-0000-4000-8000-000000000005';
const serviceReview = '60000000-0000-4000-8000-000000000006';
const servicePolicy = '70000000-0000-4000-8000-000000000007';
const serviceCredential = '80000000-0000-4000-8000-000000000008';
const serviceOwner = '90000000-0000-4000-8000-000000000009';
const serviceSchedule = 'a0000000-0000-4000-8000-00000000000a';

ServiceDelegationIntent serviceIntent({AutomationJson? parameters}) =>
    ServiceDelegationIntent(
      scope: ServiceDelegationScope(
        serviceProfile,
        'original opaque native unlock',
      ),
      runnerId: serviceRunner,
      identityId: serviceIdentity,
      runnerRevision: 3,
      projectRevision: 7,
      run: {
        'project_id': serviceProject,
        'runner_id': serviceRunner,
        'playbook': 'site.yml',
        'host_ids': [serviceHost],
        'mode': 'check',
        'cancel_on_lock': false,
        'timeout_seconds': 60,
        'forks': 2,
        'parameters':
            parameters ??
            {
              'extra_vars': {'private_example': 'PRIVATE_EXTRA_VALUE'},
              'tags': <String>[],
              'skip_tags': <String>[],
              'diff': false,
            },
      },
    );
AutomationJson serviceChoices(ServiceDelegationIntent intent) => intent.prepare(
  zone: 'UTC',
  hour: 12,
  minute: 0,
  weekdays: [0, 1, 2, 3, 4, 5, 6],
  validitySeconds: 3600,
  maxJobs: 1,
  maxPerUtcDay: 1,
);

/// Synthetic values for the reviewed Native serde contract. No user data,
/// Native issuer, signature or execution authority is constructed by this file.
AutomationJson servicePreview(ServiceDelegationIntent intent) {
  const issued = 1794566400;
  final origin = {
    'version': 1,
    'service_id': serviceRunner,
    'runner_revision': 3,
    'runner_configuration_digest': '1' * 64,
    'signer_key_sha256': '2' * 64,
    'recipient_key_sha256': '3' * 64,
    'environment_digest': 'e' * 64,
    'helper_digest': 'f' * 64,
    'known_hosts_digest': '4' * 64,
    'backend': 'linuxNativeSsh',
    'network': 'serviceLocal',
  };
  return {
    ...intent.scope.frame,
    'review': {
      'review_id': serviceReview,
      'project_id': serviceProject,
      'project_revision': 7,
      'runner_id': serviceRunner,
      'playbook': 'site.yml',
      'mode': 'check',
      'expires_at': issued + 600,
      'parameters': intent.run['parameters'],
      'policy': {
        'production': false,
        'strict_isolation': true,
        'timeout_seconds': 60,
        'forks': 2,
        'cancel_on_lock': false,
      },
      'revision': {'digest': 'c' * 64},
      'inventory_digest': 'a' * 64,
      'targets': [
        {
          'host_id': serviceHost,
          'alias': 'owned',
          'address': '127.0.0.1',
          'port': 22,
          'username': 'runner',
          'route_digest': 'b' * 64,
          'credential_refs': [serviceCredential],
        },
      ],
      'routes': <AutomationJson>[],
      'origin': {'kind': 'service_remote'},
      'privilege_review': {
        'escalation': 'not_verified',
        'accounts': [
          {'host_id': serviceHost, 'username': 'runner'},
        ],
      },
      'validation': {
        'valid': true,
        'parameters_digest': 'd' * 64,
        'inventory_digest': 'a' * 64,
        'environment_digest': 'e' * 64,
        'targets_digest': 'b' * 64,
        'checks': <AutomationJson>[],
      },
    },
    'policy': {
      'version': 4,
      'id': servicePolicy,
      'owner_id': serviceOwner,
      'owner_epoch': 1,
      'tenant': {'kind': 'localProfile', 'id': serviceProfile},
      'service_id': serviceRunner,
      'signer': {
        'kind': 'ed25519',
        'public_key': '1' * 64,
        'key_epoch': 1,
        'certificate_epoch': 1,
      },
      'recipient': {
        'kind': 'x25519',
        'public_key': '2' * 64,
        'key_epoch': 1,
        'certificate_epoch': 1,
      },
      'project_id': serviceProject,
      'project_revision': 7,
      'template_digest': 'c' * 64,
      'playbook': 'site.yml',
      'schedule_id': serviceSchedule,
      'schedule_revision': 1,
      'definition_digest': 'f' * 64,
      'inventory_digest': 'a' * 64,
      'parameters_digest': 'd' * 64,
      'environment_digest': 'e' * 64,
      'hosts': [
        {'id': serviceHost, 'revision': 1, 'target_digest': 'b' * 64},
      ],
      'credentials': [
        {'id': serviceCredential, 'revision': 1},
      ],
      'route_inputs': {'dependencies': <AutomationJson>[], 'key_trust': <AutomationJson>[]},
      'modes': ['check'],
      'quotas': {
        'max_jobs': 1,
        'max_per_utc_day': 1,
        'max_concurrent': 1,
        'max_job_seconds': 60,
        'max_forks': 2,
      },
      'policy_revision': 1,
      'revocation_epoch': 1,
      'issued_at': issued,
      'expires_at': issued + 3600,
      'desktop_lock_policy': 'independentDelegatedService',
      'access_policy': {
        'mode': 'boundedOffline',
        'offline_not_after': issued + 3600,
      },
      'execution_origin': origin,
    },
    'service': {
      'runnerId': serviceRunner,
      'runnerRevision': 3,
      'identityId': serviceIdentity,
      'namespace': 'cc-service-local-v1-${'5' * 64}',
      'signingFingerprint': '2' * 64,
      'recipientFingerprint': '3' * 64,
      'keyEpoch': 1,
      'serviceState': 'pending',
    },
    'schedule': {
      'version': 1,
      'zone': 'UTC',
      'hour': 12,
      'minute': 0,
      'weekdays': [0, 1, 2, 3, 4, 5, 6],
      'graceSeconds': 60,
      'gapPolicy': 'skip',
      'foldPolicy': 'first',
      'missedPolicy': 'skip',
    },
    'expiresUtc': DateTime.fromMillisecondsSinceEpoch(
      (issued + 3600) * 1000,
      isUtc: true,
    ).toIso8601String(),
    'consentRemainingMs': 60000,
  };
}

AutomationJson serviceStatus(
  ServiceDelegationScope scope, {
  String local = 'acknowledged',
  String remote = 'enrolled',
}) => {
  ...scope.frame,
  'runnerId': serviceRunner,
  'serviceId': serviceRunner,
  'localState': local,
  'remoteState': remote,
  'policyRevision': local == 'absent' ? null : 1,
  'revocationEpoch': local == 'absent' ? null : 1,
  'expiresUtc': local == 'absent'
      ? null
      : DateTime.fromMillisecondsSinceEpoch(
          1794570000 * 1000,
          isUtc: true,
        ).toIso8601String(),
};
