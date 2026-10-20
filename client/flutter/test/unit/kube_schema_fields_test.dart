import 'package:consolecrypt/kubernetes/kube_schema_fields.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('schema fields resolve local references, inheritance, required and array children', () {
    const document = {
      'schema': {r'$ref': '#/definitions/Root'},
      'definitions': {
        'Root': {
          'allOf': [
            {r'$ref': '#/definitions/Base'},
            {
              'properties': {
                'spec': {r'$ref': '#/definitions/Spec'},
              },
            },
          ],
        },
        'Base': {
          'properties': {
            'apiVersion': {'type': 'string'},
          },
          'required': ['apiVersion'],
        },
        'Spec': {
          'properties': {
            'containers': {
              'type': 'array',
              'items': {r'$ref': '#/definitions/Container'},
            },
          },
        },
        'Container': {
          'properties': {
            'image': {'type': 'string', 'description': 'Image reference'},
          },
          'required': ['image'],
        },
      },
    };
    final roots = kubeSchemaFields(document, []);
    expect(roots.map((f) => f.name), ['apiVersion', 'spec']);
    expect(roots.first.required, isTrue);
    final nested = kubeSchemaFields(document, ['spec', 'containers']);
    expect(nested.single.name, 'image');
    expect(nested.single.schema['description'], 'Image reference');
    expect(nested.single.required, isTrue);
    expect(kubeSchemaFields(document, ['missing']), isEmpty);
  });

  test('recursive schema and remote references stay bounded without network access', () {
    const recursive = {
      'schema': {r'$ref': '#/definitions/Loop'},
      'definitions': {
        'Loop': {
          'allOf': [
            {r'$ref': '#/definitions/Loop'},
          ],
          'properties': {
            'local': {'type': 'string'},
          },
        },
      },
    };
    expect(kubeSchemaFields(recursive, []).map((f) => f.name), ['local']);
    expect(
      kubeSchemaFields({
        'schema': {r'$ref': 'https://untrusted.example/schema'},
      }, []),
      isEmpty,
    );
  });
}
