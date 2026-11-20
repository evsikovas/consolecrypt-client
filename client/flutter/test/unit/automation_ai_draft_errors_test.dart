import 'package:consolecrypt/automation/automation_ai_draft.dart';
import 'package:consolecrypt/automation/automation_ai_draft_errors.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/l10n/app_localizations_en.dart';
import 'package:consolecrypt/l10n/app_localizations_ru.dart';
import 'package:flutter_test/flutter_test.dart';

AutomationJson _proposal({String version = '1.0.0'}) => {
  'files': [
    {'path': 'playbook.yml', 'content': '---\n'},
  ],
  'variables': <AutomationJson>[],
  'dependencies': [
    {'source': 'role:demo', 'version': version},
  ],
};

void main() {
  final en = AppLocalizationsEn(), ru = AppLocalizationsRu();

  test('malformed AI proposal fails with typed localized error rather than cast diagnostics', () {
    for (final proposal in <AutomationJson>[
      {},
      {..._proposal(), 'files': <AutomationJson>[]},
      {
        ..._proposal(),
        'variables': [null],
      },
      {
        ..._proposal(),
        'dependencies': [4],
      },
      {
        ..._proposal(),
        'dependencies': [
          {'source': null, 'version': '1.0'},
        ],
      },
      {
        ..._proposal(),
        'dependencies': [
          {'source': 'invalid', 'version': '1.0'},
        ],
      },
    ]) {
      try {
        AutomationAiDraft(proposal);
        fail('Malformed proposal accepted');
      } on AutomationAiDraftException catch (error) {
        expect(error.code, AutomationAiDraftError.invalidProposal);
        expect(automationAiDraftErrorMessage(ru, error), ru.automationAiInvalidResponse);
        expect(automationAiDraftErrorMessage(ru, error), isNot(en.automationAiInvalidResponse));
        expect(error.message, 'automation_ai_draft');
      }
    }
  });

  test('invalid requirements grammar, shapes and duplicate keys never expose input in errors', () {
    final draft = AutomationAiDraft(_proposal());
    for (final before in [
      '{"synthetic-secret": broken JSON}',
      '[]',
      'null',
      '{"version":1,"version":1,"playbooks":{}}',
      '{"version":1,"playbooks":[]}',
    ]) {
      try {
        draft.requirements('playbook.yml', before: before);
        fail('Invalid requirements accepted');
      } on AutomationAiDraftException catch (error) {
        expect(error.code, AutomationAiDraftError.invalidRequirements);
        final visible = automationAiDraftErrorMessage(ru, error);
        expect(visible, ru.automationAiRequirementsInvalid);
        expect(visible, isNot(contains('synthetic-secret')));
        expect(error.toString(), isNot(contains('synthetic-secret')));
        expect(error.source, isNull);
      }
    }
  });

  test('dependency pin conflicts remain rejected with typed requirements guidance', () {
    final previous = AutomationAiDraft(_proposal());
    final before = previous.requirements('playbook.yml');
    final next = AutomationAiDraft(_proposal(version: '2.0.0'));
    expect(
      () => next.requirements('playbook.yml', before: before),
      throwsA(
        isA<AutomationAiDraftException>().having((e) => e.code, 'code', AutomationAiDraftError.dependencyConflict),
      ),
    );
    expect(previous.requirements('playbook.yml'), before);
  });

  test('canonical filename and all requirements codes have localized actionable text', () {
    for (final code in AutomationAiDraftError.values) {
      final error = AutomationAiDraftException(code);
      final visible = automationAiDraftErrorMessage(ru, error);
      expect(visible, isNotEmpty);
      expect(visible, isNot(automationAiDraftErrorMessage(en, error)));
      expect(visible, isNot(contains('automation_ai_draft')));
      expect(visible, isNot(contains('Use the exact lowercase requirements filename')));
      if (code == AutomationAiDraftError.canonicalFilename) {
        expect(visible, contains(automationRequirementsFile));
        expect(visible, ru.automationAiRequirementsInvalid);
      }
    }
  });

  test('path validation and file budget stay distinct typed failures', () {
    final draft = AutomationAiDraft(_proposal());
    expect(
      () => draft.requirements('../outside.yml'),
      throwsA(isA<AutomationAiDraftException>().having((e) => e.code, 'code', AutomationAiDraftError.invalidPath)),
    );
    expect(
      () => draft.requirements('playbook.yml', before: ' ' * 65537),
      throwsA(isA<AutomationAiDraftException>().having((e) => e.code, 'code', AutomationAiDraftError.limitExceeded)),
    );
  });
}
