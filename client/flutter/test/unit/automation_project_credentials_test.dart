import 'dart:convert';

import 'package:consolecrypt/automation/automation_project_credentials.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/project_environment_fixture.dart' as f;

ProjectCredentialsContext read(AutomationJson value, {String nonce = f.nonce}) =>
    ProjectCredentialsContext.parse(value, profileId: f.profileId, nonce: nonce, project: f.project);
void main() {
  test('actual closed context is immutable, original-bound and never chooses an environment implicitly', () {
    final raw = f.context(), value = read(raw);
    expect(value.frame.nonce, f.nonce);
    expect(value.policy!.environments.single.references.single.credentialId, f.credentialId);
    expect(() => value.selection('60000000-0000-4000-8000-000000000001'), throwsFormatException);
    final selected = value.selection(f.environmentId);
    expect(selected, {'version': 1, 'frame': f.frame(), 'environment_id': f.environmentId});
    (raw['frame'] as AutomationJson)['notification_scope_id'] = 'later';
    expect(value.frame.nonce, f.nonce);
  });
  test('legacy policy remains null and no selection can be fabricated', () {
    final value = read(f.context(configured: false));
    expect(value.policy, isNull);
    expect(() => value.selection(f.environmentId), throwsFormatException);
    expect(value.saveRequest(null)['policy'], isNull);
  });
  test('context rejects future shapes, mismatched owners projects revisions and unsupported credential kinds', () {
    for (final mutate in <void Function(AutomationJson)>[
      (v) => v['version'] = 1.0,
      (v) => v['extra'] = 'not copied',
      (v) => (v['frame'] as AutomationJson)['profile_id'] = f.hostId,
      (v) => (v['frame'] as AutomationJson)['notification_scope_id'] = 'other original',
      (v) => (v['frame'] as AutomationJson)['project_id'] = f.hostId,
      (v) => (v['frame'] as AutomationJson)['project_revision'] = 2,
      (v) => v['supported_credential_kinds'] = ['password', 'ssh_private_key', 'os_ssh_agent'],
    ]) {
      final raw = f.context();
      mutate(raw);
      expect(() => read(raw), throwsFormatException);
    }
  });
  test('opaque nonce preserves exact multibyte bytes and rejects C0 C1 surrogate and byte overflow', () {
    final boundary = 'ñ' * 128;
    expect(read(f.context(scope: boundary), nonce: boundary).frame.nonce, boundary);
    for (final invalid in ['', 'ñ' * 129, 'a\n', 'a\u0085', String.fromCharCode(0xd800)]) {
      expect(() => read(f.context(scope: invalid), nonce: invalid), throwsFormatException);
    }
  });
  test('policy enforces exact bounds, distinct environment/Host refs and no secret or future fields', () {
    for (final mutate in <void Function(AutomationJson)>[
      (v) => v['version'] = 2,
      (v) => (v['environments'] as List).clear(),
      (v) => (v['environments'] as List).add((v['environments'] as List).first),
      (v) => ((v['environments'] as List).first as AutomationJson)['name'] = ' padded ',
      (v) => ((v['environments'] as List).first as AutomationJson)['production'] = 'false',
      (v) => ((v['environments'] as List).first as AutomationJson)['secret'] = 'not accepted',
      (v) => (((v['environments'] as List).first as AutomationJson)['overrides'] as List).add({
        'host_id': f.hostId,
        'credential_id': f.credentialId,
      }),
    ]) {
      final raw = jsonDecode(jsonEncode(f.policy())) as AutomationJson;
      mutate(raw);
      expect(() => ProjectCredentialsPolicy.parse(raw), throwsFormatException);
    }
  });
  test('same existing credential can be chosen deliberately in distinct environments and hosts', () {
    final raw = f.policy(), environments = raw['environments'] as List;
    environments.add({
      'id': f.hostId,
      'name': 'Production',
      'production': true,
      'overrides': [
        {'host_id': f.projectId, 'credential_id': f.credentialId},
      ],
    });
    final value = ProjectCredentialsPolicy.parse(raw)!;
    expect(value.environments.length, 2);
    expect(value.environments.map((v) => v.references.single.credentialId).toSet(), {f.credentialId});
  });
  test('save reuses exact original frame and requires the actual next project CAS revision', () {
    final value = read(f.context()), policy = value.policy;
    expect(value.saveRequest(policy)['frame'], f.frame());
    final result = {
      'version': 1,
      'project': {'id': f.projectId, 'revision': 2},
      'frame': f.frame(revision: 2),
      'policy': policy!.toJson(),
    };
    expect(value.saved(result).frame.revision, 2);
    result['frame'] = f.frame(revision: 1);
    expect(() => value.saved(result), throwsFormatException);
  });
  test('Native review binds exact chosen references and OR risk; caller choice cannot suppress production', () {
    final value = read(f.context()), env = value.policy!.environments.single;
    AutomationJson preview() => {
      'id': env.id,
      'name': env.name,
      'production': true,
      'policy_sha256': 'a' * 64,
      'credential_overrides': [for (final ref in env.references) ref.toJson()],
    };
    expect(checkedDeploymentReview(preview(), value, env.id, true)!.production, true);
    final wrong = preview()..['policy_sha256'] = 'b' * 64;
    expect(() => checkedDeploymentReview(wrong, value, env.id, true), throwsFormatException);
    final risk = read(f.context()..['policy'] = f.policy(production: true));
    final lowered = preview()..['production'] = false;
    expect(() => checkedDeploymentReview(lowered, risk, env.id, false), throwsFormatException);
    expect(() => checkedDeploymentReview(preview(), null, null, true), throwsFormatException);
  });
}
