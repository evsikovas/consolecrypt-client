import 'package:consolecrypt/automation/automation_secret_view.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('INI masks several password aliases without touching users or following variables', () {
    const source =
        '[docker_hosts]\nserver ansible_user=root ansible_ssh_pass="secret value" ansible_port=22 '
        "ansible_become_password='sudopass'\n";
    final masked = maskAutomationPasswords(source);
    expect(masked, isNot(contains('secret value')));
    expect(masked, isNot(contains('sudopass')));
    expect(masked, contains('ansible_user=root'));
    expect(masked, contains('ansible_port=22'));
    expect(masked.length, source.length);
    expect(source, contains('secret value'));
  });
  test('YAML quoted keys, escaped quotes, multiline strings and CRLF preserve offsets', () {
    const source =
        r'"ansible_password": "s\"ecret😀"'
        '\r\n'
        "ansible_become_pass: 'it''s\nsecret'\nnext: visible\n";
    final masked = maskAutomationPasswords(source);
    expect(masked, isNot(contains('secret')));
    expect(masked, isNot(contains('ecret')));
    expect(masked, contains('next: visible'));
    expect(masked.length, source.length);
    for (var i = 0; i < source.length; i++) {
      if (source[i] == '\n' || source[i] == '\r') expect(masked[i], source[i]);
    }
  });
  test('YAML secret block hides contents but not the following sibling', () {
    const source = 'vars:\n  ansible_password: |-\n    long\n    secret\n  sibling: visible\n';
    final masked = maskAutomationPasswords(source);
    expect(masked, isNot(contains('long')));
    expect(masked, isNot(contains('secret')));
    expect(masked, contains('sibling: visible'));
  });
  test('Plain YAML scalar with spaces masks through end of scalar, not its comment', () {
    const source = 'ansible_password: two words # comment\nansible_user: root\n';
    final masked = maskAutomationPasswords(source);
    expect(masked, isNot(contains('two words')));
    expect(masked, contains('# comment'));
    expect(masked, contains('ansible_user: root'));
  });
  test('Empty values and commented assignments do not mask ordinary documents', () {
    const source = '# ansible_password: example\n; password=sample\nansible_password:\nansible_port: 22\n';
    expect(maskAutomationPasswords(source), source);
  });
  test('Unterminated quoted value is conservatively hidden to EOF', () {
    const source = 'host ansible_password="not closed\ncontinued secret';
    final masked = maskAutomationPasswords(source);
    expect(masked, isNot(contains('not closed')));
    expect(masked, isNot(contains('continued secret')));
    expect(masked.length, source.length);
  });
  test('INI backslash escaped whitespace is part of the secret', () {
    const source = r'host ansible_password=two\ words ansible_port=22';
    final masked = maskAutomationPasswords(source);
    expect(masked, isNot(contains('words')));
    expect(masked, contains('ansible_port=22'));
    expect(masked.length, source.length);
  });
  test('YAML plain multiline and indented values do not expose continuation lines', () {
    const source =
        'vars:\n  ansible_password: first\n    second secret\n  ansible_become_pass:\n    third secret\n  user: root\n';
    final masked = maskAutomationPasswords(source);
    expect(masked, isNot(contains('first')));
    expect(masked, isNot(contains('second secret')));
    expect(masked, isNot(contains('third secret')));
    expect(masked, contains('user: root'));
    expect(masked.length, source.length);
  });
}
