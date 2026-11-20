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

  test('static INI and YAML preview names together without showing variables', () {
    final groups = previewStaticInventory({
      'hosts.ini':
          'loose\n[web]\napp1 ansible_password=synthetic-value\n[2001:db8::1]:22\n'
          '[production:children]\nweb\n[web:vars]\nsecret=synthetic-value\n',
      'hosts.yml': 'web:\n  hosts:\n    app2:\n',
    });
    expect(groups, {
      'production': <String>[],
      'ungrouped': ['loose'],
      'web': ['[2001:db8::1]:22', 'app1', 'app2'],
    });
  });

  test('dynamic plugin files and playbooks require another path', () {
    expect(() => previewStaticYamlInventory(['plugin: example']), throwsFormatException);
    expect(() => previewStaticYamlInventory(['- name: Example\n  hosts: all']), throwsFormatException);
  });
}
