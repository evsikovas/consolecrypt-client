import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/automation/automation_service_delegation.dart';
import 'package:consolecrypt/automation/automation_service_results.dart';
import 'package:crypto/crypto.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/service_delegation_fixture.dart';
import '../support/service_results_fixture.dart';

ServiceRecordedResults parse(AutomationJson value) =>
    ServiceRecordedResults.parse(value, serviceIntent().scope, serviceRunner);
Map<String, dynamic> job(AutomationJson value) => (value['jobs'] as List).single as Map<String, dynamic>;
void main() {
  test('genuine post-exit public Native results preserve exact UNKNOWN partial outcome and original frame', () {
    final bytes = File('test/fixtures/service_results_native.json').readAsBytesSync();
    expect(bytes, hasLength(820));
    expect(sha256.convert(bytes).toString(), '6f8f181196ad77b501b4aa201ec749f5d4b927244d2a9556fce917f22514bffa');
    final raw = jsonDecode(utf8.decode(bytes)) as AutomationJson;
    // Historical owned fixture boundary only; never grants a live Native scope.
    final original = ServiceDelegationScope(raw['profileId'] as String, raw['notificationScopeId'] as String);
    final runner = raw['runnerId'] as String;
    final result = ServiceRecordedResults.parse(raw, original, runner);
    expect(result.policyDigest, 'caf44b02de2738113a53a4e61d45a42b8004ec33974d4e3e8f042506d1c572dc');
    final row = result.jobs.single;
    expect(row.jobId, 'fa757d56-18d3-8afc-94fb-d175641a42e2');
    expect(row.manifestDigest, 'd8cd03718ff1ce9cbc712133b6954ca953eb19aed1b334cb2fcbbe4789ef2e3c');
    expect(row.scheduleId, '65f807f2-c0b3-4c94-8465-ca10ed5f95ab');
    expect(row.localSlot, '2026-11-13T23:44');
    expect(row.state, 'unknown');
    expect(row.runnerState, 'unknown');
    expect(row.targetOutcome, 'pending');
    expect(row.partial, isTrue);
    expect(row.cancelRequested, isTrue);
    expect(row.timedOut, isFalse);
    expect(row.exitCode, -15);
    expect(row.dueUtc.millisecondsSinceEpoch, 1794613440000);
    expect(row.acceptedUtc, DateTime.parse('2026-11-13T23:44:10.361792Z'));
    expect(row.updatedUtc, DateTime.parse('2026-11-13T23:44:17.829297Z'));
    expect(() => ServiceRecordedResults.parse(raw, serviceIntent().scope, runner), throwsFormatException);
    expect(() => ServiceRecordedResults.parse(raw, original, serviceRunner), throwsFormatException);
    final inventedSuccess = jsonDecode(utf8.decode(bytes)) as AutomationJson;
    job(inventedSuccess)['state'] = 'succeeded';
    expect(() => ServiceRecordedResults.parse(inventedSuccess, original, runner), throwsFormatException);
    final concealedPartial = jsonDecode(utf8.decode(bytes)) as AutomationJson;
    job(concealedPartial)['remoteChangesMayBePartial'] = false;
    expect(() => ServiceRecordedResults.parse(concealedPartial, original, runner), throwsFormatException);
  });

  test('recorded terminal preserves SAME job unknown partial exit and nullable flags', () {
    final data = parse(recordedServiceResults());
    final row = data.jobs.single;
    expect(row.jobId, resultJob);
    expect(row.state, 'unknown');
    expect(row.runnerState, 'unknown');
    expect(row.partial, isTrue);
    expect(row.exitCode, -15);
    expect(row.cancelRequested, isTrue);
    expect(row.timedOut, isNull);
    expect(row.dueUtc.isUtc, isTrue);
    expect(() => data.jobs.clear(), throwsUnsupportedError);
  });
  test('clean success requires actual target outcome exit zero and false flags', () {
    final raw = recordedServiceResults();
    job(raw).addAll({
      'state': 'succeeded',
      'runnerState': 'succeeded',
      'targetOutcome': 'known',
      'remoteChangesMayBePartial': false,
      'cancelRequested': false,
      'timedOut': false,
      'exitCode': 0,
    });
    expect(parse(raw).jobs.single.state, 'succeeded');
    for (final change in [
      {'timedOut': null},
      {'cancelRequested': true},
      {'exitCode': null},
      {'targetOutcome': 'uncertain'},
      {'runnerState': 'unknown'},
    ]) {
      final other = recordedServiceResults();
      job(other).addAll(job(raw));
      job(other).addAll(change);
      expect(() => parse(other), throwsFormatException);
    }
  });
  test('foreign scope runner policy shape duplicate future and raw output are rejected', () {
    for (final change in [
      {'version': 1.0},
      {'version': 2},
      {'profileId': serviceHost},
      {'notificationScopeId': 'other'},
      {'runnerId': serviceHost},
      {'serviceId': serviceHost},
      {'policyDigest': 'not-digest'},
      {'resultsAvailable': false},
      {'rawOutput': 'PRIVATE'},
    ]) {
      final raw = recordedServiceResults()..addAll(change);
      expect(() => parse(raw), throwsFormatException);
    }
    final duplicate = recordedServiceResults();
    (duplicate['jobs'] as List).add(job(duplicate));
    expect(() => parse(duplicate), throwsFormatException);
  });
  test('bounded strict dates states and integer fields cannot invent a result', () {
    for (final change in [
      {'dueUtc': 1.0},
      {'dueUtc': 0},
      {'dueUtc': 253402300800},
      {'localSlot': '2026-02-30T12:00'},
      {'acceptedUtc': '2026-02-30T12:00:00Z'},
      {'updatedUtc': '2026-11-14T12:00:00+00:00'},
      {'state': 'running'},
      {'targetOutcome': 'success'},
      {'timedOut': 0},
      {'exitCode': 2147483648},
      {'remoteChangesMayBePartial': false},
      {'stdout': 'PRIVATE'},
    ]) {
      final raw = recordedServiceResults();
      job(raw).addAll(change);
      expect(() => parse(raw), throwsFormatException, reason: '$change');
    }
    final raw = recordedServiceResults();
    raw['jobs'] = List.generate(33, (_) => job(recordedServiceResults()));
    expect(() => parse(raw), throwsFormatException);
  });
  test('known empty readable store stays empty and is not unavailable or success', () {
    final raw = recordedServiceResults()..['jobs'] = <Object>[];
    expect(parse(raw).jobs, isEmpty);
  });
}
