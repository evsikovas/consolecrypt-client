import 'dart:convert';

import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/automation/automation_windows_prerequisites.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:flutter_test/flutter_test.dart';

const profile = '11111111-1111-4111-8111-111111111111';
final originalScope = RuntimeNoticeScope.fromProfile({
  'id': profile,
  'active': true,
  'vault_state': 'unlocked',
  'notification_scope_id': 'original-opaque-\u03b1',
})!;
AutomationJson windowsFact() => {
  'version': 1,
  'profileId': profile,
  'notificationScopeId': originalScope.nonce,
  'capturedAtUtc': '2026-11-13T10:20:30.123Z',
  'source': 'windowsReadOnlyV1',
  'elevation': {'state': 'notElevated', 'reason': null},
  'features': {
    'subsystemLinux': {'state': 'unknown', 'reason': 'requiresElevation'},
    'virtualMachinePlatform': {
      'state': 'unknown',
      'reason': 'requiresElevation',
    },
  },
  'reboot': {'state': 'notObserved', 'reason': null, 'indicators': <String>[]},
};
void main() {
  test('original opaque Native nonce and read-only unknown facts retain actual UTC, never disabled', () {
    final facts = WindowsPrerequisiteSnapshot.parse(
      windowsFact(),
      originalScope,
    );
    expect(facts.subsystem.state, WindowsFeatureObservation.unknown);
    expect(facts.subsystem.reason, WindowsFeatureUnknown.requiresElevation);
    expect(facts.elevation, WindowsElevationObservation.notElevated);
    expect(facts.reboot, WindowsRebootObservation.notObserved);
    expect(facts.capturedAt, DateTime.utc(2026, 11, 13, 10, 20, 30, 123));
  });
  test('closed producer domains reject foreign owner, future fields, impossible token/feature and invalid calendar', () {
    for (final mutate in <void Function(AutomationJson)>[
      (v) => v['notificationScopeId'] = 'replacement',
      (v) => v['profileId'] = '22222222-2222-4222-8222-222222222222',
      (v) => v['version'] = 2,
      (v) => v['ready'] = true,
      (v) => v['source'] = 'stderr',
      (v) => v['capturedAtUtc'] = '2026-02-31T10:20:30Z',
      (v) => (v['features'] as Map)['subsystemLinux'] = {
        'state': 'disabled',
        'reason': null,
      },
      (v) => (v['features'] as Map)['subsystemLinux'] = {
        'state': 'unknown',
        'reason': 'unsupportedPlatform',
      },
      (v) => v['reboot'] = {
        'state': 'notObserved',
        'reason': null,
        'indicators': ['windowsUpdate'],
      },
    ]) {
      final value = jsonDecode(jsonEncode(windowsFact())) as AutomationJson;
      mutate(value);
      expect(
        () => WindowsPrerequisiteSnapshot.parse(value, originalScope),
        throwsA(anything),
      );
    }
  });
  test('elevated feature transitions and concrete reboot indicators remain separate observations', () {
    final value = windowsFact();
    value['elevation'] = {'state': 'elevated', 'reason': null};
    value['features'] = {
      'subsystemLinux': {'state': 'enabled', 'reason': null},
      'virtualMachinePlatform': {'state': 'enablePending', 'reason': null},
    };
    value['reboot'] = {
      'state': 'indicatorsPresent',
      'reason': null,
      'indicators': ['windowsUpdate'],
    };
    final facts = WindowsPrerequisiteSnapshot.parse(value, originalScope);
    expect(facts.virtualMachine.state, WindowsFeatureObservation.enablePending);
    expect(facts.indicators, [WindowsRebootIndicator.windowsUpdate]);
  });
}
