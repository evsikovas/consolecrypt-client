import 'dart:async';

import 'package:consolecrypt/monitoring/agent_reader_controller.dart';
import 'package:consolecrypt/monitoring/agent_reader_models.dart';
import 'package:consolecrypt/monitoring/agent_reader_port.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/agent_reader_fixture.dart' as f;
import '../support/agent_reader_history_fixture.dart';

Future<void> ready(AgentReaderController owner) async {
  await owner.prepare(AgentReaderRequest.association(AgentReaderAction.read, AgentReaderLocator('a' * 64, 'b' * 64)));
  await owner.confirm();
}

void main() {
  test('closed unavailable expired and cancelled Native read failures remain typed and one-use', () async {
    for (final code in ['reader_unavailable', 'reader_expired', 'reader_cancelled']) {
      final port = ReadPort()..readFailure = AgentReaderException.fromCode(code);
      final owner = AgentReaderController(port: port, selection: f.selection(), isOriginal: () => true);
      await ready(owner);
      await owner.consumeRead();
      expect(owner.failure, code);
      expect(owner.dataset, isNull);
      await owner.consumeRead();
      expect(port.handles.single.reads, 1);
      expect(port.requests, hasLength(1));
      expect(port.statuses, isEmpty);
      owner.dispose();
    }
  });
  test('malformed data remains invalid and never becomes an offline claim or zero metrics', () async {
    final port = ReadPort()..body = 'PRIVATE_SYNTHETIC_RESPONSE_NOT_JSON';
    final owner = AgentReaderController(port: port, selection: f.selection(), isOriginal: () => true);
    await ready(owner);
    await owner.consumeRead();
    expect(owner.failure, 'reader_response_invalid');
    expect(owner.dataset, isNull);
    expect(owner.readConsumed, true);
    expect(port.handles.single.reads, 1);
    owner.dispose();
  });
  test('pending history stays owned until completion and retired scope cannot publish late points', () async {
    var original = true;
    final port = ReadPort()..pendingRead = Completer<String>();
    final owner = AgentReaderController(port: port, selection: f.selection(), isOriginal: () => original);
    await ready(owner);
    final pending = owner.consumeRead();
    original = false;
    port.pendingRead!.complete(port.body);
    await pending;
    expect(owner.dataset, isNull);
    expect(port.handles.single.cancels, 1);
    expect(port.handles.single.reads, 1);
    owner.dispose();
  });
}
