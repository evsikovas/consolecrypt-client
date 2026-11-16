import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/monitoring/agent_reader_models.dart';
import 'package:consolecrypt/monitoring/agent_reader_port.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';

import 'agent_reader_fixture.dart' as f;

class ReadHandle extends f.Handle {
  ReadHandle(this.owner, super.selection, super.action);
  final ReadPort owner;
  @override
  Future<String> consumeRead() async {
    reads++;
    if (owner.readFailure case final failure?) throw failure;
    return owner.pendingRead?.future ?? owner.body;
  }
}

class ReadPort extends f.Port {
  ReadPort() : super(f.selection()) {
    final value = f.statusJson(selection);
    (value['associationChoices'] as Json)['items'] = [
      f.choice()
        ..['action'] = 'historyRead'
        ..['datasetKind'] = 'localAgentHistoryV1',
    ];
    statusResponse = value;
  }
  AgentReaderException? readFailure;
  Completer<String>? pendingRead;
  String body = f.dataset(f.history(count: 70));
  @override
  Future<AgentReaderIssuedHandle> prepareAction(String raw) async {
    final request = readerDecode(raw);
    requests.add(request);
    final action = AgentReaderAction.values.byName((request['action'] as Json)['kind'] as String);
    final handle = ReadHandle(this, selection, action);
    final details = f.review(selection, action);
    if (action == AgentReaderAction.read) ((details['details'] as Json)['target'] as Json)['action'] = 'historyRead';
    handle.customDetails = encode(details);
    handles.add(handle);
    return handle;
  }
}

String encode(Json value) => jsonEncode(value);
