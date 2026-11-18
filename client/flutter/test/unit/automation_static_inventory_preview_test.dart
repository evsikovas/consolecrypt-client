import 'package:consolecrypt/automation/automation_static_inventory_preview.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('static YAML exposes named groups and hosts without rendering variables', () {
    final groups = previewStaticYamlInventory([
      '''
all:
  children:
    web:
      hosts:
        app1:
          ansible_password: synthetic-value
    databases:
      hosts:
        db1:
''',
      'web:\n  hosts:\n    app2:\n',
    ]);
    expect(groups, {
      'all': <String>[],
      'databases': ['db1'],
      'web': ['app1', 'app2'],
    });
    expect(groups.toString(), isNot(contains('synthetic-value')));
  });

  test('dynamic plugin files and playbooks require another path', () {
    expect(() => previewStaticYamlInventory(['plugin: example']), throwsFormatException);
    expect(() => previewStaticYamlInventory(['- name: Example\n  hosts: all']), throwsFormatException);
  });
}
