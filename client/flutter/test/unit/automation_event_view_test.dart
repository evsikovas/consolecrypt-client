import 'dart:convert';

import 'package:consolecrypt/automation/automation_event_view.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('no_log and censored diagnostic subtrees never expose their contents', () {
    for (final marker in ['_ansible_no_log', 'censored']) {
      final details = automationEventDiagnostics({
        'payload': {
          'res': {marker: true, 'msg': 'PRIVATE_CONTENT', 'stdout': 'PRIVATE_OUTPUT'},
        },
      });
      expect(details, {'censored': true});
      expect(jsonEncode(details), isNot(contains('PRIVATE')));
    }
  });

  test('human and technical event projections exclude module output and terminal escape controls', () {
    final event = {
      'sequence': 1,
      'type': 'runner_on_ok',
      'timestamp': 'now',
      'payload': {
        'task': '\x1b]52;c;PRIVATE_CLIPBOARD\x07Task\x1b[31m name\x00',
        'host': 'approved',
        'res': {
          'changed': true,
          'stdout': 'PRIVATE_STDOUT',
          'stderr': 'PRIVATE_STDERR',
          'msg': 'PRIVATE_MESSAGE',
          'invocation': {'secret': 'PRIVATE'},
          'diff': 'PRIVATE_DIFF',
        },
      },
    };
    final aliases = {'approved': '192.0.2.1'};
    final lines = automationHumanEventLines(event, aliases).join();
    expect(lines, contains('changed [192.0.2.1]'));
    expect(lines, contains('Task name'));
    final projected = jsonEncode(automationEventProjection(event, aliases));
    expect(projected, isNot(contains('PRIVATE')));
    expect(projected, isNot(contains('stdout')));
    expect(projected, isNot(contains('001b')));
  });
  test('recap preserves known zero and missing counts and ignores unapproved aliases', () {
    final lines = automationHumanEventLines(
      {
        'type': 'runner_summary',
        'payload': {
          'stats': {
            'ok': {'approved': 0, 'foreign': 999},
          },
        },
      },
      {'approved': '192.0.2.1'},
    ).join();
    expect(lines, contains('ok=0'));
    expect(lines, contains('changed=—'));
    expect(lines, isNot(contains('999')));
  });
}
