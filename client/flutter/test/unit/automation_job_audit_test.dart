import 'dart:convert';

import 'package:consolecrypt/automation/automation_job_admission_audit.dart';
import 'package:consolecrypt/automation/automation_job_result_metadata.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/automation_audit_fixture.dart' as f;

void main() {
  test('closed audit binds only original recorded signer revision Runner and exact target sequence', () {
    final row = f.fixture(production: true), a = parseAutomationJobAdmissionAudit(row)!;
    expect(a.signerId, f.deviceId);
    expect(a.ownerEvidenceDigest, 'e' * 64);
    expect(a.targets.length, 2);
    expect(a.issuedAt, 1700000000);
    expect(a.targets.first.hostId, f.hostId(0));
    for (final mutate in <void Function(AutomationJson)>[
      (a) => a['version'] = 1.0,
      (a) => a['signerId'] = f.runnerId,
      (a) => a['jobId'] = f.projectId,
      (a) => a['revisionDigest'] = 'f' * 64,
      (a) => a['runnerId'] = f.deviceId,
      (a) => a['scope'] = 'tunnel.open',
      (a) => a['ownerEvidenceDigest'] = null,
      (a) => a['issuedAt'] = 1700000000.0,
      (a) => (a['targets'] as List).removeLast(),
      (a) => (a['targets'] as List).insert(0, (a['targets'] as List).removeLast()),
      (a) => a['rawToken'] = 'DO_NOT_RENDER',
    ]) {
      final bad = f.fixture(production: true);
      mutate(bad['admission_audit'] as AutomationJson);
      expect(() => parseAutomationJobAdmissionAudit(bad), throwsFormatException);
    }
  });
  test('legacy unavailable and future audit states never infer current identity or approved permission', () {
    final missing = f.fixture()..remove('admission_audit');
    expect(parseAutomationJobAdmissionAudit(missing), isNull);
    for (final reason in ['legacy_native_provenance', 'runner_unrecorded', 'not_observed']) {
      final row = f.fixture()..['admission_audit'] = {'version': 1, 'status': 'unrecorded', 'reason': reason};
      expect(parseAutomationJobAdmissionAudit(row)!.reason, reason);
    }
    final bad = f.fixture()
      ..['admission_audit'] = {'version': 1, 'status': 'unavailable', 'reason': 'binding_mismatch'};
    expect(parseAutomationJobAdmissionAudit(bad)!.targets, isEmpty);
    bad['admission_audit'] = {'version': 1, 'status': 'ready', 'approved': true};
    expect(() => parseAutomationJobAdmissionAudit(bad), throwsFormatException);
  });
  test('audit rejects oversize duplicate targets foreign row and contradictory production evidence', () {
    final many = f.fixture(count: 513);
    expect(() => parseAutomationJobAdmissionAudit(many), throwsFormatException);
    final dup = f.fixture();
    final a = dup['admission_audit'] as AutomationJson;
    (a['targets'] as List)[1] = (a['targets'] as List).first;
    expect(() => parseAutomationJobAdmissionAudit(dup), throwsFormatException);
    final foreign = f.fixture()..['job_id'] = f.projectId;
    expect(() => parseAutomationJobAdmissionAudit(foreign), throwsFormatException);
    final proof = f.fixture();
    (proof['admission_audit'] as AutomationJson)['ownerEvidenceDigest'] = 'e' * 64;
    expect(() => parseAutomationJobAdmissionAudit(proof), throwsFormatException);
  });
  test('technical projection preserves real state exit flags cursor UTC and process interval only', () {
    final row = f.fixture(), projection = automationResultMetadata(row);
    expect(projection.recognized, true);
    final encoded = jsonEncode(projection.facts);
    expect(encoded, contains('unknown'));
    expect(encoded, contains('"exit_code":-15'));
    expect(encoded, contains('"timed_out":false'));
    expect(encoded, contains('"cancel_requested":true'));
    expect(encoded, contains('runner-process'));
    expect(encoded, contains('2023-11-14T22:13:22Z'));
    expect(encoded, contains('"sequence":14'));
    expect(encoded, contains('2.20.1'));
    expect(encoded, contains('"remote_changes_may_be_partial":true'));
  });
  test('technical projection never traverses extra raw or no_log fields and never invents zero or success', () {
    final row = f.fixture(), raw = row['runner_result'] as AutomationJson, result = raw['result'] as AutomationJson;
    raw['password'] = 'RAW_CANARY';
    raw['owner_key'] = 'RAW_CANARY';
    result['stdout'] = 'RAW_CANARY';
    result['summary'] = {
      '_ansible_no_log': true,
      'stats': {
        'ok': {'cc_0': 99},
      },
      'status': 'successful',
      'secret': 'NOLOG_CANARY',
    };
    var encoded = jsonEncode(automationResultMetadata(row).facts);
    expect(encoded, contains('"exit_code":-15'));
    expect(encoded, contains('"state":"unknown"'));
    expect(encoded, isNot(contains('CANARY')));
    expect(encoded, isNot(contains('successful')));
    expect(encoded, isNot(contains('HIDDEN')));
    result['censored'] = true;
    encoded = jsonEncode(automationResultMetadata(row).facts);
    expect(encoded, isNot(contains('exit_code')));
    expect(encoded, isNot(contains('"timed_out":false')));
    row['runner_result'] = {'output': 'RAW_CANARY'};
    final unknown = automationResultMetadata(row);
    expect(unknown.recognized, false);
    expect(jsonEncode(unknown.facts), isNot(contains('RAW_CANARY')));
    expect(jsonEncode(unknown.facts), isNot(contains('succeeded')));
  });
  test('known Native control loss and exact nested wrapper preserve uncertainty without raw fallback', () {
    final row = f.fixture(), original = row['runner_result'];
    row['runner_result'] = {'reason': 'native_operation_not_acknowledged', 'runner_result': original};
    expect(automationResultMetadata(row).recognized, true);
    row['runner_result'] = {
      'reason': 'control_channel_or_storage_lost',
      'control_failure': 'rpc_closed',
      'remote_changes_may_be_partial': true,
      'diagnostic': 'RAW_CANARY',
    };
    final p = automationResultMetadata(row);
    expect(p.recognized, true);
    expect(jsonEncode(p.facts), contains('rpc_closed'));
    expect(jsonEncode(p.facts), isNot(contains('RAW_CANARY')));
    row['runner_result'] = {'reason': 'RAW_CANARY', 'exit_code': 0};
    expect(automationResultMetadata(row).recognized, false);
    expect(jsonEncode(automationResultMetadata(row).facts), isNot(contains('exit_code')));
  });
  test('legacy environment text is withheld despite matching digest; only actual closed version domains survive', () {
    final row = f.fixture(),
        raw = row['runner_result'] as AutomationJson,
        identity = (raw['environment'] as AutomationJson)['identity'] as AutomationJson;
    for (final key in ['system', 'machine', 'python', 'helper']) {
      identity[key] = 'KNOWN_LEGACY_SECRET';
    }
    identity['packages'] = {
      'ansible-core': {'actual': 'KNOWN_LEGACY_SECRET'},
    };
    expect(jsonEncode(automationResultMetadata(row).facts), isNot(contains('KNOWN_LEGACY_SECRET')));
    identity['system'] = 'Linux';
    identity['machine'] = 'x86_64';
    identity['python'] = '3.12.3';
    identity['helper'] = '0.1.0';
    identity['packages'] = {
      'ansible-core': {'actual': '2.20.11'},
    };
    final encoded = jsonEncode(automationResultMetadata(row).facts);
    for (final value in ['Linux', 'x86_64', '3.12.3', '0.1.0', '2.20.11']) {
      expect(encoded, contains(value));
    }
    for (final invalid in ['1', '1.2.3.4.5', '1.2-secret', '123456789.2', '1.2\n', '１.２']) {
      identity['python'] = invalid;
      expect(automationClosedEnvironmentIdentity(identity)?.containsKey('python'), false);
    }
  });
}
