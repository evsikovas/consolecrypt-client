import 'package:consolecrypt/automation/automation_service_delegation.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/service_delegation_fixture.dart';

void main() {
  test('opaque original nonce is retained; controls and excessive UTF8 are refused', () {
    final frame = ServiceDelegationScope(
      serviceProfile,
      'opaque native scope / α',
    );
    expect(frame.frame['notificationScopeId'], 'opaque native scope / α');
    for (final nonce in ['', 'x\n', '\u0085', 'α' * 129]) {
      expect(
        () => ServiceDelegationScope(serviceProfile, nonce),
        throwsFormatException,
      );
    }
  });
  test(
    'intent owns frozen run and confirmation carries reference IDs only',
    () {
      final values = {
        'extra_vars': {'example': 'PRIVATE'},
        'tags': <String>[],
        'skip_tags': <String>[],
        'diff': false,
      };
      final intent = serviceIntent(parameters: values);
      (values['extra_vars'] as Map)['example'] = 'MUTATED';
      expect((intent.run['parameters'] as Map)['extra_vars'], {
        'example': 'PRIVATE',
      });
      final accepted = ServiceDelegationPreview.parse(
        servicePreview(intent),
        intent,
        serviceChoices(intent),
      );
      expect(accepted.confirmation.keys.toSet(), {
        'version',
        'profileId',
        'notificationScopeId',
        'reviewId',
        'policyId',
      });
      expect(accepted.cancellation.keys.toSet(), {
        'version',
        'profileId',
        'notificationScopeId',
        'reviewId',
      });
      expect(
        () => accepted.policy['policy_revision'] = 99,
        throwsUnsupportedError,
      );
    },
  );
  test('foreign original profile, nonce, Runner identity or project revision never become a preview', () {
    final intent = serviceIntent();
    for (final field in ['profile', 'nonce', 'runner', 'identity', 'project']) {
      final raw = servicePreview(intent);
      switch (field) {
        case 'profile':
          raw['profileId'] = serviceOwner;
        case 'nonce':
          raw['notificationScopeId'] = 'new unlock';
        case 'runner':
          (raw['service'] as Map)['runnerRevision'] = 4;
        case 'identity':
          (raw['service'] as Map)['identityId'] = serviceOwner;
        case 'project':
          (raw['policy'] as Map)['project_revision'] = 8;
      }
      expect(
        () =>
            ServiceDelegationPreview.parse(raw, intent, serviceChoices(intent)),
        throwsFormatException,
        reason: field,
      );
    }
  });
  test('different hosts, variables, schedule, quota or caller authority fields are refused', () {
    final intent = serviceIntent();
    for (final field in [
      'host',
      'variables',
      'schedule',
      'quota',
      'authority',
    ]) {
      final raw = servicePreview(intent);
      switch (field) {
        case 'host':
          (((raw['review'] as Map)['targets'] as List).first as Map)['host_id'] =
              serviceOwner;
        case 'variables':
          (raw['review'] as Map)['parameters'] = {
            'extra_vars': {'injected': true},
            'tags': <String>[],
            'skip_tags': <String>[],
            'diff': false,
          };
        case 'schedule':
          (raw['schedule'] as Map)['hour'] = 13;
        case 'quota':
          ((raw['policy'] as Map)['quotas'] as Map)['max_jobs'] = 2;
        case 'authority':
          (raw['policy'] as Map)['approved'] = true;
      }
      expect(
        () =>
            ServiceDelegationPreview.parse(raw, intent, serviceChoices(intent)),
        throwsFormatException,
        reason: field,
      );
    }
  });
  test('consent remaining and signed window remain separate and bounded', () {
    final intent = serviceIntent();
    for (final remaining in [0, 60001, true]) {
      final raw = servicePreview(intent)..['consentRemainingMs'] = remaining;
      expect(
        () =>
            ServiceDelegationPreview.parse(raw, intent, serviceChoices(intent)),
        throwsFormatException,
      );
    }
    final raw = servicePreview(intent);
    final accepted = ServiceDelegationPreview.parse(
      raw,
      intent,
      serviceChoices(intent),
    );
    expect(accepted.consentRemainingMs, 60000);
    expect(
      accepted.expiresUtc.millisecondsSinceEpoch ~/ 1000,
      (raw['policy'] as Map)['expires_at'],
    );
    raw['expiresUtc'] = '2026-02-31T00:00:00Z';
    expect(
      () => ServiceDelegationPreview.parse(raw, intent, serviceChoices(intent)),
      throwsFormatException,
    );
  });
  test('unknown versions and unsigned local delivery data never imply running or a new grant', () {
    final intent = serviceIntent();
    for (final pair in [
      ('deliveryUnknown', 'enrolled'),
      ('localBlocked', 'unknown'),
      ('acknowledged', 'restored'),
    ]) {
      final value = ServiceDelegationStatus.parse(
        serviceStatus(intent.scope, local: pair.$1, remote: pair.$2),
        intent.scope,
        serviceRunner,
      );
      expect(value.enrollmentAcknowledged, isFalse);
    }
    final absent = serviceStatus(
      intent.scope,
      local: 'absent',
      remote: 'unbound',
    );
    expect(
      ServiceDelegationStatus.parse(
        absent,
        intent.scope,
        serviceRunner,
      ).policyRevision,
      isNull,
    );
    absent['version'] = 2;
    expect(
      () => ServiceDelegationStatus.parse(absent, intent.scope, serviceRunner),
      throwsFormatException,
    );
  });
}
