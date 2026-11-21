import 'package:consolecrypt/wslc/features/containers/templates.dart';
import 'package:consolecrypt/wslc/src/rust/models.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('templates drop the container name and, by default, env values', () {
    const spec = RunSpec(
      image: 'app:1',
      name: 'web-1',
      ports: [],
      mounts: [],
      env: [EnvVar(key: 'TOKEN', value: 's3cret')],
      command: ['serve'],
      workdir: '/srv',
      autoRemove: true,
      start: false,
    );
    final t = templateSpec(spec, withEnvValues: false);
    expect(t.name, isNull);
    expect(t.env.single.key, 'TOKEN');
    expect(t.env.single.value, isEmpty);
    expect(t.command, ['serve']);
    expect(t.workdir, '/srv');
    expect(t.autoRemove, isTrue);
    expect(t.start, isFalse);
    expect(templateSpec(spec, withEnvValues: true).env.single.value, 's3cret');
  });
}
