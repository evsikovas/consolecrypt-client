import 'package:consolecrypt/automation/automation_ai_context.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:flutter_test/flutter_test.dart';

String id(int n) => '00000000-0000-4000-8000-${n.toString().padLeft(12, '0')}';
AutomationJson capture({bool runner = false}) => {
  'version': 1,
  'captureId': id(1),
  'profileId': id(2),
  'notificationScopeId': id(3),
  'projectId': id(4),
  'projectRevision': 7,
  'subjectId': id(5),
  'subjectRevision': 9,
  'kind': runner ? 'runnerVersions' : 'hostOs',
  'source': runner ? 'runnerProbeProjectPinsV1' : 'sshFixedOsV1',
  'capturedUtc': '2026-11-13T10:00:00.000Z',
  'expiresUtc': '2026-11-13T10:02:00.000Z',
  'fields': runner
      ? {
          'ansibleCore': '2.19.0',
          'python': '3.12.5',
          'collections': [
            {'name': 'ansible.posix', 'version': '2.0.0'},
          ],
        }
      : {'osId': 'debian', 'osVersion': '12', 'architecture': 'x86_64', 'kernelRelease': null},
};
AutomationAiCapturedContext parse(AutomationJson raw, {bool runner = false}) => AutomationAiCapturedContext.fromJson(
  raw,
  expectedProfileId: id(2),
  expectedNotificationScopeId: id(3),
  expectedProjectId: id(4),
  expectedProjectRevision: 7,
  expectedKind: runner ? AutomationAiContextKind.runnerVersions : AutomationAiContextKind.hostOs,
  expectedSubjectId: id(5),
);
void main() {
  test('actual capture/source/subject/revision UTC retained; omitted/null means unknown, never zero', () {
    final snapshot = parse(capture());
    expect(snapshot.subjectRevision, 9);
    expect(snapshot.projectRevision, 7);
    expect(snapshot.source, 'sshFixedOsV1');
    expect(snapshot.fields.containsKey('kernelRelease'), false);
    expect(snapshot.capturedUtc, DateTime.utc(2026, 11, 13, 10));
  });
  test('selection contains only Native refs and explicit known fields, never values', () {
    final snapshot = parse(capture());
    expect(snapshot.selection({'architecture', 'osId'}), {
      'captureId': id(1),
      'fieldIds': ['architecture', 'osId'],
    });
    expect(() => snapshot.selection({}), throwsFormatException);
    expect(() => snapshot.selection({'kernelRelease'}), throwsFormatException);
    expect(() => snapshot.fields['osId'] = 'changed', throwsUnsupportedError);
  });
  test('foreign original nonce/profile/project/subject/revision and future source fail closed', () {
    for (final field in ['profileId', 'notificationScopeId', 'projectId', 'subjectId']) {
      expect(() => parse(capture()..[field] = id(90)), throwsFormatException);
    }
    for (final change in [
      {'projectRevision': 8},
      {'subjectRevision': 0},
      {'subjectRevision': 9007199254740992},
      {'version': 2},
      {'version': 1.0},
      {'source': 'futureSource'},
      {'privateData': 'never accepted'},
      {'kind': 'runnerVersions'},
    ]) {
      expect(() => parse(capture()..addAll(change)), throwsFormatException);
    }
  });
  test('UTC calendar, offset, TTL and wall ordering validation reject normalized invalid dates', () {
    for (final date in [
      '2026-02-31T10:00:00Z',
      '2026-11-13T25:00:00Z',
      '2026-11-13T10:00:60Z',
      '2026-11-13T10:00:00+00:00',
      '2026-11-13',
    ]) {
      expect(() => parse(capture()..['capturedUtc'] = date), throwsFormatException);
    }
    for (final date in ['2026-11-13T09:59:59Z', '2026-11-13T10:02:00.001Z']) {
      expect(() => parse(capture()..['expiresUtc'] = date), throwsFormatException);
    }
  });
  test('OS fields are fixed, bounded printable strings; arbitrary tree/value not admitted', () {
    for (final fields in [
      {
        'osId': {'value': 'linux'},
      },
      {'osId': List.filled(257, 'a').join()},
      {'osId': 'linux\u001b[31m'},
      {'password': 'never accepted'},
    ]) {
      expect(() => parse(capture()..['fields'] = fields), throwsFormatException);
    }
  });
  test('Runner current probes and project pins stay distinct metadata, with 32-entry bound', () {
    final snapshot = parse(capture(runner: true), runner: true);
    expect(snapshot.source, 'runnerProbeProjectPinsV1');
    expect(snapshot.fields['ansibleCore'], '2.19.0');
    for (final rows in [
      List.generate(33, (i) => {'name': 'namespace.c$i', 'version': '1'}),
      [
        {'name': 'ansible.posix', 'version': '1'},
        {'name': 'ansible.posix', 'version': '2'},
      ],
      [
        {'name': 'ansible.posix', 'version': '1', 'path': 'not admitted'},
      ],
    ]) {
      expect(
        () => parse(capture(runner: true)..['fields'] = {'collections': rows}, runner: true),
        throwsFormatException,
      );
    }
  });
}
