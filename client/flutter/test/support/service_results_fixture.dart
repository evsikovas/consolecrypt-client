import 'package:consolecrypt/automation/automation_service.dart';

import 'service_delegation_fixture.dart';

const resultJob = 'b0000000-0000-4000-8000-00000000000b';
AutomationJson recordedServiceResults() => {
  ...serviceIntent().scope.frame,
  'runnerId': serviceRunner,
  'serviceId': serviceRunner,
  'policyDigest': 'a' * 64,
  'resultsAvailable': true,
  'jobs': [
    {
      'jobId': resultJob,
      'manifestDigest': 'b' * 64,
      'scheduleId': serviceSchedule,
      'localSlot': '2026-11-14T12:00',
      'dueUtc': 1794657600,
      'state': 'unknown',
      'runnerState': 'unknown',
      'targetOutcome': 'uncertain',
      'remoteChangesMayBePartial': true,
      'cancelRequested': true,
      'timedOut': null,
      'exitCode': -15,
      'acceptedUtc': '2026-11-14T12:00:00Z',
      'updatedUtc': '2026-11-14T12:01:00Z',
    },
  ],
};
