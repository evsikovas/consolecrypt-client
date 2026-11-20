import 'package:consolecrypt/automation/ansible_editor_controller.dart';
import 'package:consolecrypt/automation/ansible_editor_folding.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';

const source = '''---
- hosts: all
  tasks:
    - name: First
      ansible.builtin.debug:
        msg: "{{ greeting | default('hello') }}"
    - name: Second
      ansible.builtin.debug:
        msg: second
''';

void main() {
  test('task folds preserve complete save bytes and original line numbers', () {
    final raw = TextEditingController(text: source);
    final editor = AnsibleEditorDocument(raw);
    addTearDown(() {
      editor.dispose();
      raw.dispose();
    });
    expect(editor.toggleFold(4), isTrue);
    expect(raw.text, source);
    expect(editor.display.text, isNot(contains('greeting')));
    expect(editor.display.text, contains('# … 2'));
    expect(editor.projection.lineNumbers, containsAllInOrder([1, 2, 3, 4, null, 7, 8, 9]));
    editor.expandAll();
    expect(editor.display.text, source);
    expect(raw.text, source);
  });
  test('editing visible task while another is folded saves all hidden source', () {
    final raw = TextEditingController(text: source), saves = <String>[];
    final editor = AnsibleEditorDocument(raw, onChanged: saves.add);
    addTearDown(() {
      editor.dispose();
      raw.dispose();
    });
    editor.toggleFold(4);
    expect(saves, isEmpty);
    final start = editor.display.text.indexOf('second');
    editor.display.selection = TextSelection(baseOffset: start, extentOffset: start + 6);
    expect(editor.selectedText, 'second');
    editor.replaceSelection('edited');
    expect(raw.text, source.replaceFirst('second', 'edited'));
    expect(raw.text, contains("default('hello')"));
    expect(raw.text, isNot(contains('# …')));
    expect(saves.single, raw.text);
    expect(editor.collapsed, isNotEmpty);
  });
  test('native text editing delta maps correctly after a hidden task', () {
    final raw = TextEditingController(text: source);
    final doc = AnsibleEditorDocument(raw);
    addTearDown(() {
      doc.dispose();
      raw.dispose();
    });
    doc.toggleFold(4);
    final old = doc.display.text, at = old.indexOf('second');
    doc.display.selection = TextSelection.collapsed(offset: at + 6);
    doc.display.value = TextEditingValue(
      text: old.replaceFirst('second', 'updated'),
      selection: TextSelection.collapsed(offset: at + 7),
    );
    expect(raw.text, source.replaceFirst('second', 'updated'));
    expect(raw.selection.extentOffset, raw.text.indexOf('updated') + 7);
  });
  test('copy select-all, replace, undo and redo include hidden content and selection', () {
    final raw = TextEditingController(text: source);
    final editor = AnsibleEditorDocument(raw);
    addTearDown(() {
      editor.dispose();
      raw.dispose();
    });
    editor.toggleFold(4);
    editor.selectAll();
    expect(editor.selectedText, source);
    final before = raw.value;
    editor.replaceSelection('---\n');
    expect(raw.text, '---\n');
    editor.undo();
    expect(raw.text, source);
    expect(raw.selection, before.selection);
    editor.redo();
    expect(raw.text, '---\n');
  });
  test('typing in a synthetic marker expands without saving or dropping its body', () {
    final raw = TextEditingController(text: source), editor = AnsibleEditorDocument(raw);
    addTearDown(() {
      editor.dispose();
      raw.dispose();
    });
    editor.toggleFold(4);
    final marker = editor.display.text.indexOf('# …');
    final before = editor.display.text;
    editor.display.value = TextEditingValue(
      text: before.replaceRange(marker, marker, 'x'),
      selection: TextSelection.collapsed(offset: marker + 1),
    );
    expect(raw.text, source);
    expect(editor.collapsed, isEmpty);
    expect(editor.display.text, source);
  });
  test('folding CRLF and nested blocks is a pure view operation', () {
    final text = source.replaceAll('\n', '\r\n');
    final raw = TextEditingController(text: text), doc = AnsibleEditorDocument(raw);
    addTearDown(() {
      doc.dispose();
      raw.dispose();
    });
    doc.toggleFold(4);
    doc.toggleFold(3);
    doc.expandAll();
    expect(raw.text, text);
    expect(doc.display.text, text);
    expect(
      ansibleFoldRegions('''- name: shell
  ansible.builtin.shell: |
    - name: not a task
      something: data
- name: next
  ansible.builtin.debug:
    msg: ok
''').where((region) => region.headerLine == 3),
      isEmpty,
    );
  });
  test('read-only display refuses edits and disposal releases history/projection', () {
    final raw = TextEditingController(text: 'secret: •••••\n'), doc = AnsibleEditorDocument(raw, readOnly: true);
    doc.selectAll();
    expect(doc.replaceSelection('erase'), isFalse);
    doc.display.clear();
    expect(raw.text, 'secret: •••••\n');
    expect(doc.display.text, raw.text);
    doc.undo();
    expect(raw.text, 'secret: •••••\n');
    doc.dispose();
    expect(doc.disposed, isTrue);
    expect(doc.selectedText, isEmpty);
    expect(doc.projection.source, isEmpty);
    raw.dispose();
  });
  test('canonical UTF8 byte budget rejects Cyrillic and emoji without changing save bytes', () {
    final raw = TextEditingController(text: 'safe'), doc = AnsibleEditorDocument(raw);
    addTearDown(() {
      doc.dispose();
      raw.dispose();
    });
    for (final insert in ['я' * (512 * 1024 + 1), '😀' * (256 * 1024 + 1)]) {
      doc.selectAll();
      expect(doc.replaceSelection(insert), isFalse);
      expect(raw.text, 'safe');
      expect(doc.editRejected, isTrue);
    }
    doc.selectAll();
    expect(doc.replaceSelection('я' * (512 * 1024)), isTrue);
    expect(raw.text.length, 512 * 1024);
  });
}
