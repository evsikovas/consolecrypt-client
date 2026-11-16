import 'dart:convert';

import 'package:consolecrypt/monitoring/agent_reader_models.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/agent_reader_fixture.dart' as f;

AgentReaderDataset read(Object? version) {
  final body = f.heartbeat()..['version'] = version;
  final original = AgentReaderOutcome.parse(jsonEncode(f.result(AgentReaderAction.read)), AgentReaderAction.read);
  return AgentReaderDataset.parse(f.dataset(body), original);
}

void main() {
  test('Agent version preserves the exact bounded Native ASCII value and snapshot time', () {
    for (final version in ['1.0.0', 'v2.3.4-preview+owned.7', 'a' * 64]) {
      final data = read(version);
      expect(data.agentVersion, version);
      expect(data.kind, 'heartbeatV1');
      expect(data.observedAt!.isUtc, isTrue);
      expect(data.collectionState, 'unknown');
    }
  });
  test('Agent version rejects Native-invalid empty null nonASCII punctuation and oversized values', () {
    for (final invalid in [null, '', true, 1, 'a' * 65, 'версия', '1_2', '1 2', '1/2', 'v1\n']) {
      expect(() => read(invalid), throwsFormatException, reason: '$invalid');
    }
  });
}
