import 'dart:convert';

import 'package:consolecrypt/monitoring/agent_reader_models.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/agent_reader_fixture.dart' as f;

AgentReaderDataset point(Map<String, dynamic> additions) {
  final body = f.history(count: 1);
  body['points'] = [
    <String, dynamic>{...(body['points'] as List).single as Map<String, dynamic>, ...additions},
  ];
  final outcome = AgentReaderOutcome.parse(jsonEncode(f.result(AgentReaderAction.read)), AgentReaderAction.read);
  return AgentReaderDataset.parse(f.dataset(body), outcome);
}

void main() {
  test('Native closed collection metadata preserves all states and nullable gauges', () {
    for (final entry in <String, int?>{
      'running': 1,
      'disabled': 0,
      'stopped': 0,
      'not_running': 0,
      'starting': null,
      'degraded': null,
      'blocked': null,
      'unknown': null,
    }.entries) {
      final actual = point({
        'collection_state_version': 1,
        'collection_state': entry.key,
        'agent_collector_running': entry.value,
        'agent_uptime_seconds': null,
      }).points.single;
      expect(actual.collectionState, entry.key);
      expect(actual.values['agent_collector_running'], entry.value?.toDouble());
      expect(actual.values['agent_uptime_seconds'], isNull);
    }
  });
  test('legacy zero has unknown cause and missing remains Unknown rather than zero', () {
    for (final entry in <int?, String>{1: 'running', 0: 'not_running', null: 'unknown'}.entries) {
      final actual = point({'agent_collector_running': entry.key}).points.single;
      expect(actual.collectionState, entry.value);
      expect(actual.values['agent_collector_running'], entry.key?.toDouble());
    }
  });
  test('partial future contradictory and untrusted metadata never becomes an accepted state', () {
    for (final bad in <Map<String, dynamic>>[
      {'collection_state': 'unknown'},
      {'collection_state_version': 1},
      {'collection_state_version': true, 'collection_state': 'unknown'},
      {'collection_state_version': 1.0, 'collection_state': 'unknown'},
      {'collection_state_version': 2, 'collection_state': 'unknown'},
      {'collection_state_version': 1, 'collection_state': null},
      {'collection_state_version': 1, 'collection_state': 'PRIVATE diagnostic'},
      {'collection_state_version': 1, 'collection_state': 'running'},
      {'collection_state_version': 1, 'collection_state': 'unknown', 'agent_collector_running': 0},
      {'collection_state_version': 1, 'collection_state': 'running', 'agent_collector_running': 1.0},
      {'collection_state_version': 1, 'collection_state': 'unknown', 'collection_granted': true},
    ]) {
      expect(() => point(bad), throwsFormatException, reason: '$bad');
    }
  });
}
