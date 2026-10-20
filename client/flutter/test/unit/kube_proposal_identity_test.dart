import 'package:consolecrypt/ai/code_blocks.dart';
import 'package:consolecrypt/kubernetes/kube_proposal_identity.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('untrusted answers cap interactive fences while preserving remaining text', () {
    final answer = List.filled(20000, '```sh\necho example\n```\n').join();
    final segments = parseAnswer(answer, maxSegments: 32);
    expect(segments.whereType<CodeSegment>().length, 32);
    expect(segments.last, isA<TextSegment>());
    expect((segments.last as TextSegment).text, contains('echo example'));
  });
  test('only a bounded scalar resource identity leaves the YAML worker', () async {
    final parser = KubeProposalParser();
    addTearDown(parser.cancel);
    final identity = await parser.parse('''
apiVersion: apps/v1
kind: Deployment
metadata:
  name: api
  namespace: team
spec:
  template:
    metadata:
      labels: {app: api}
''');
    expect(identity?.apiVersion, 'apps/v1');
    expect(identity?.kind, 'Deployment');
    expect(identity?.name, 'api');
    expect(identity?.namespace, 'team');
    expect(await parser.parse('kind: [Pod]\nmetadata: {name: demo}\napiVersion: v1'), isNull);
    expect(await parser.parse('kind: Pod\nmetadata: {name: demo}\napiVersion: v1\n---\nkind: Secret'), isNull);
    expect(await parser.parse('x' * (256 * 1024 + 1)), isNull);
  });

  test('cancelled or replaced manifest work cannot publish its identity', () async {
    final parser = KubeProposalParser();
    addTearDown(parser.cancel);
    const text = 'apiVersion: v1\nkind: Pod\nmetadata: {name: demo}';
    final pending = parser.parse(text);
    parser.cancel();
    expect(await pending, isNull);
    final old = parser.parse(text);
    final replacement = parser.parse(text.replaceFirst('demo', 'new'));
    expect(await old, isNull);
    expect((await replacement)?.name, 'new');
  });
}
