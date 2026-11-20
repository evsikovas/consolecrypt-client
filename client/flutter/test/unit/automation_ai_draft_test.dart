import 'dart:convert';

import 'package:consolecrypt/automation/automation_ai_draft.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:flutter_test/flutter_test.dart';

AutomationJson proposal({List<String> required = const ['new_input'], String version = '1.2.3'}) => {
  'files': [
    {'path': 'playbook.yml', 'content': '---\n# editable draft\n'},
  ],
  'variables': [
    for (final name in required) {'name': name, 'required': true},
  ],
  'dependencies': [
    {'source': 'role:demo', 'version': version},
  ],
};

void main() {
  test('reviewed requirements retain manual constraints and other playbooks, never invent provenance', () {
    final before = jsonEncode({
      'version': 1,
      'playbooks': {
        'playbook.yml': {
          'required_extra_vars': ['manual_input'],
          'required_dependencies': <AutomationJson>[],
        },
        'other.yml': {
          'required_extra_vars': ['other_input'],
          'required_dependencies': <AutomationJson>[],
        },
      },
    });
    final result = AutomationAiDraft(proposal()).requirements('playbook.yml', before: before);
    final decoded = jsonDecode(result) as AutomationJson;
    final playbooks = decoded['playbooks'] as AutomationJson;
    final spec = playbooks['playbook.yml'] as AutomationJson;
    expect(spec['required_extra_vars'], ['manual_input', 'new_input']);
    expect((playbooks['other.yml'] as AutomationJson)['required_extra_vars'], ['other_input']);
    expect(spec['required_dependencies'], [
      {'kind': 'role', 'name': 'demo', 'version': '1.2.3'},
    ]);
    expect(result, isNot(contains('sha256')));
    expect(result, isNot(contains('url')));
    expect(AutomationAiDraft(proposal()).requirements('playbook.yml', before: result), result);
  });
  test('regeneration retains unsaved manual review constraints and refuses conflicting pins', () {
    final previous = AutomationAiDraft(proposal(required: ['previous_input']));
    final next = AutomationAiDraft(proposal())..retainRequirements(previous);
    expect(next.requiredVariables, containsAll(['previous_input', 'new_input']));
    expect(next.dependencies, hasLength(1));
    expect(() => AutomationAiDraft(proposal(version: '2.0.0')).retainRequirements(previous), throwsFormatException);
    final before = previous.requirements('playbook.yml');
    expect(
      () => AutomationAiDraft(proposal(version: '2.0.0')).requirements('playbook.yml', before: before),
      throwsFormatException,
    );
  });
  test('duplicate escaped keys, future schemas, authority variables and unpinned dependencies are not rewritten', () {
    final draft = AutomationAiDraft(proposal());
    for (final raw in [
      '{"version":1,"version":1,"playbooks":{}}',
      r'{"version":1,"\u0076ersion":1,"playbooks":{}}',
      '{"version":2,"playbooks":{}}',
      '{"version":1,"playbooks":{},"future":true}',
      '{"version":1,"playbooks":{"p.yml":{"required_extra_vars":["ansible_host"],"required_dependencies":[]}}}',
      '{"version":1,"playbooks":{"p.yml":{"required_extra_vars":[],"required_extra_vars":[],"required_dependencies":[]}}}',
    ]) {
      expect(() => draft.requirements('playbook.yml', before: raw), throwsFormatException, reason: raw);
    }
    expect(() => AutomationAiDraft(proposal(required: ['ansible_host'])), throwsFormatException);
    expect(() => AutomationAiDraft(proposal(version: 'latest')), throwsFormatException);
    expect(() => draft.requirements('../outside.yml'), throwsFormatException);
    expect(() => draft.requirements('playbook.yml', before: ' ' * 65537), throwsFormatException);
  });
}
