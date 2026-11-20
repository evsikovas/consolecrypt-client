import 'package:consolecrypt/automation/automation_ansible_config.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  const gathering = AutomationAnsibleSetting.gathering;
  const fatal = AutomationAnsibleSetting.anyErrorsFatal;
  const timeout = AutomationAnsibleSetting.taskTimeout;

  group('lossless Ansible config proposals', () {
    test('opening and empty changes preserve every code unit', () {
      for (final text in [
        '',
        '\ufeff',
        '# Комментарий\r\n[defaults]\r\ngathering = implicit\r\nunknown = custom\n[ssh_connection]\nfoo: bar',
        '[defaults]\ngathering = implicit\ngathering = explicit\n',
        '[DEFAULT]\nany_errors_fatal=True\n[defaults]\n',
      ]) {
        expect(AutomationAnsibleConfigDocument.parse(text).propose({}), text);
      }
    });

    test('only changed value bytes replaced, inline comments and unknown keys retained', () {
      const text =
          '\ufeff# Привет\r\n[defaults]\r\n  GaThErInG : implicit  ; keep me\r\n'
          '  task_timeout = 42 \r\n  custom = a#b;c\n[other]\nvalue=not edited';
      final document = AutomationAnsibleConfigDocument.parse(text);
      expect(document.canEdit, isTrue);
      expect(document.value(gathering), 'implicit');
      expect(document.value(timeout), '42');
      expect(document.propose({gathering: 'smart'}), text.replaceFirst('implicit', 'smart'));
      expect(
        document.propose({gathering: 'smart', timeout: '30'}),
        text.replaceFirst('implicit', 'smart').replaceFirst('42 ', '30 '),
      );
    });

    test('same semantic choice preserves original spelling and whitespace', () {
      const text = '[defaults]\nANY_ERRORS_FATAL : yes  ; comment\ntask_timeout=00042\n';
      final document = AutomationAnsibleConfigDocument.parse(text);
      expect(document.propose({fatal: 'True', timeout: '42'}), text);
    });

    test('empty value replacement preserves semicolon comment semantics', () {
      const text = '[defaults]\ngathering =   ; keep this comment\ntask_timeout=; note\n';
      final document = AutomationAnsibleConfigDocument.parse(text);
      expect(
        document.propose({gathering: 'smart', timeout: '3'}),
        '[defaults]\ngathering =   smart ; keep this comment\ntask_timeout=3 ; note\n',
      );
    });

    test('hash inline is value; only whitespace semicolon is a comment', () {
      final document = AutomationAnsibleConfigDocument.parse(
        '[defaults]\nany_errors_fatal = False # literal\ntask_timeout=5;literal\ngathering=implicit ; comment\n',
      );
      expect(document.value(fatal), 'False # literal');
      expect(document.value(timeout), '5;literal');
      expect(document.value(gathering), 'implicit');
      expect(
        document.propose({fatal: 'True'}),
        '[defaults]\nany_errors_fatal = True\ntask_timeout=5;literal\ngathering=implicit ; comment\n',
      );
    });

    test('inserts only absent explicit choices into existing defaults', () {
      const text = '# header\r\n[defaults]\r\n# exact comment\r\nunknown=x\r\n[next]\r\nk=v';
      expect(
        AutomationAnsibleConfigDocument.parse(text).propose({timeout: '9', fatal: 'True'}),
        text.replaceFirst('[defaults]\r\n', '[defaults]\r\nany_errors_fatal = True\r\ntask_timeout = 9\r\n'),
      );
    });

    test('insertion handles missing section, EOF newline and BOM without rewrite', () {
      const cases = <String, String>{
        '': '[defaults]\ntask_timeout = 0\n',
        '\ufeff': '\ufeff[defaults]\ntask_timeout = 0\n',
        '[defaults]': '[defaults]\ntask_timeout = 0\n',
        '# c\r\n[other]\r\nk=v': '# c\r\n[other]\r\nk=v\r\n[defaults]\r\ntask_timeout = 0\r\n',
        '[Defaults]\nx=y\n': '[Defaults]\nx=y\n[defaults]\ntask_timeout = 0\n',
      };
      for (final entry in cases.entries) {
        expect(AutomationAnsibleConfigDocument.parse(entry.key).propose({timeout: '0'}), entry.value);
      }
    });

    test('does not modify a same-named key in other sections', () {
      const text = '[other]\ngathering=keep\n[defaults]\ngathering=implicit\n';
      expect(
        AutomationAnsibleConfigDocument.parse(text).propose({gathering: 'explicit'}),
        '[other]\ngathering=keep\n[defaults]\ngathering=explicit\n',
      );
    });

    test('unknown multiline values remain exact', () {
      const text = '[defaults]\ncustom = one\n  two\n  three=four\ngathering = implicit\n';
      final document = AutomationAnsibleConfigDocument.parse(text);
      expect(document.canEdit, isTrue);
      expect(document.propose({gathering: 'explicit'}), text.replaceFirst('implicit', 'explicit'));
    });

    test('ambiguous config cannot be rewritten by form', () {
      for (final text in [
        '[defaults]\ngathering=implicit\nGATHERING=smart\n',
        '[defaults]\na=1\na=2\n',
        '[defaults]\n[defaults]\n',
        '[other]\n[other]\n',
        '[DEFAULT]\ngathering=smart\n[defaults]\n',
        '[defaults]\ngathering=implicit\n  explicit\n',
        '[defaults]\ngathering=implicit\n\n  next=value\n',
        'gathering=implicit\n',
        '[defaults]\nnot an assignment\n',
        '[defaults] broken\ngathering=implicit\n',
        '[defaults]\nkey=\u0000\n',
        '#${'a' * (1024 * 1024)}',
        '#${'я' * (600 * 1024)}',
      ]) {
        final document = AutomationAnsibleConfigDocument.parse(text);
        expect(document.canEdit, isFalse, reason: text.length < 150 ? text : 'oversized');
        expect(() => document.propose({gathering: 'smart'}), throwsStateError);
        expect(document.propose({}), text);
      }
    });

    test('typed options cannot inject config sections, paths or unsafe values', () {
      final document = AutomationAnsibleConfigDocument.parse('');
      for (final setting in AutomationAnsibleSetting.values) {
        for (final value in ['True\nplugin_path=/tmp', '[other]', '', '-1', '1.5', 'a' * 1000]) {
          expect(() => document.propose({setting: value}), throwsArgumentError);
        }
      }
      expect(() => document.propose({timeout: '99999999999999999999999999999'}), throwsArgumentError);
    });

    test('result separates advanced navigation from CAS save proposal', () {
      const advanced = AutomationAnsibleConfigResult.advanced();
      const apply = AutomationAnsibleConfigResult.apply('[defaults]\n');
      expect(advanced.action, AutomationAnsibleConfigAction.advanced);
      expect(advanced.proposedText, isNull);
      expect(apply.action, AutomationAnsibleConfigAction.apply);
      expect(apply.proposedText, '[defaults]\n');
    });
  });
}
