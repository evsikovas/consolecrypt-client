import 'package:consolecrypt/docker/compose_ai_dialog.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('Compose AI accepts plain or fenced YAML, preserves placeholders', () {
    const yaml = 'services:\n  web:\n    image: nginx:1.28\n    environment:\n      TOKEN: \${TOKEN}';
    expect(composeYamlFromAi(yaml), yaml);
    expect(composeYamlFromAi('```yaml\n$yaml\n```'), yaml);
  });
  test('Compose AI rejects prose, empty services, broken YAML and oversized replies', () {
    for (final text in ['hello', 'services: []', 'services: {}', 'services: [', 'x' * (128 * 1024 + 1)]) {
      expect(() => composeYamlFromAi(text), throwsFormatException);
    }
  });
}
