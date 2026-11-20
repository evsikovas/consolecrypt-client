import 'package:consolecrypt/automation/ansible_editor_syntax.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

void main() {
  test('Ansible directives modules Jinja filters strings anchors and comments are distinct', () {
    const source = '''- name: Example
  ansible.builtin.template:
    src: "{{ item.src | default('x#y') }}"
    dest: /tmp/value # actual comment
  become: true
  vars: &defaults
    other: *defaults
  when: item.enabled
{% if enabled %}
{# template comment #}
''';
    final tokens = ansibleSyntaxTokens(source);
    final kinds = tokens.map((token) => token.kind).toSet();
    expect(
      kinds,
      containsAll([
        AnsibleTokenKind.keyword,
        AnsibleTokenKind.module,
        AnsibleTokenKind.jinjaDelimiter,
        AnsibleTokenKind.jinjaFilter,
        AnsibleTokenKind.comment,
        AnsibleTokenKind.anchor,
        AnsibleTokenKind.scalar,
      ]),
    );
    for (var i = 1; i < tokens.length; i++) {
      expect(tokens[i].start, greaterThanOrEqualTo(tokens[i - 1].end));
    }
    expect(
      ansibleSyntaxSpan(source, const TextStyle(), ColorScheme.fromSeed(seedColor: Colors.blue)).toPlainText(),
      source,
    );
    expect(
      tokens
          .where((token) => token.kind == AnsibleTokenKind.comment)
          .map((token) => source.substring(token.start, token.end)),
      contains('# actual comment'),
    );
  });
  test('block scalar bodies do not become YAML keys but still highlight Jinja', () {
    const source = 'command: |\n  # script data\n  fake_key: {{ value }}\nnext: false\n';
    final tokens = ansibleSyntaxTokens(source);
    expect(
      tokens
          .where((token) => token.kind == AnsibleTokenKind.key)
          .map((token) => source.substring(token.start, token.end)),
      isNot(contains('fake_key')),
    );
    expect(tokens.any((token) => token.kind == AnsibleTokenKind.jinjaDelimiter), isTrue);
    expect(
      ansibleSyntaxSpan(source, const TextStyle(), ColorScheme.fromSeed(seedColor: Colors.blue)).toPlainText(),
      source,
    );
  });
  test('unfinished quotes and templates preserve exact bytes; large text stays plain', () {
    for (final text in ['value: "{{ unfinished', "value: 'unclosed # string", 'x: {{ value | default(\n']) {
      expect(ansibleSyntaxSpan(text, null, ColorScheme.fromSeed(seedColor: Colors.blue)).toPlainText(), text);
    }
    final large = 'x' * 200001;
    expect(ansibleSyntaxTokens(large), isEmpty);
    expect(ansibleSyntaxSpan(large, null, ColorScheme.fromSeed(seedColor: Colors.blue)).toPlainText(), large);
  });
}
