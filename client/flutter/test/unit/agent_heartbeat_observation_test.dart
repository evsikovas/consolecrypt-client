import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/monitoring/agent_heartbeat_observation.dart';
import 'package:consolecrypt/monitoring/agent_reader_controller.dart';
import 'package:consolecrypt/monitoring/agent_reader_models.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/agent_reader_fixture.dart' as f;

final class _Heartbeat extends f.Handle {
  _Heartbeat(AgentReaderSelection selection) : super(selection, AgentReaderAction.read);
  Completer<String>? pending;
  @override
  Future<String> consumeRead() async {
    reads++;
    return pending?.future ?? f.dataset(f.heartbeat()..['host_id'] = f.hostId);
  }
}

void main() {
  test('only consumed original heartbeat publishes exact association and observed IDs, without extra RPC', () async {
    final selection = f.selection(), association = AgentReaderLocator('b' * 64, 'c' * 64);
    final port = f.Port(selection), handle = _Heartbeat(selection);
    port.pending = Completer()..complete(handle);
    final store = AgentHeartbeatObservations(), owner = Object(), received = DateTime.utc(2026, 11, 15);
    var observations = 0;
    final controller = AgentReaderController(
      port: port,
      selection: selection,
      isOriginal: () => true,
      onHeartbeat: (dataset, actualAssociation) {
        observations++;
        store.record(
          selection: selection,
          association: actualAssociation,
          dataset: dataset,
          host: f.host,
          scopeOwner: owner,
          epoch: 7,
          receivedAt: received,
        );
      },
    );
    addTearDown(controller.dispose);
    await controller.prepare(AgentReaderRequest.association(AgentReaderAction.read, association));
    await controller.confirm();
    expect(observations, 0);
    await controller.consumeRead();
    await controller.consumeRead();
    final observed = store.read(
      host: f.host,
      profileId: f.profile,
      notificationScopeId: selection.notificationScopeId,
      scopeOwner: owner,
      epoch: 7,
    )!;
    expect(observed.dataset.agentVersion, '1.0.0');
    expect(observed.dataset.agentId, f.hostId);
    expect(observed.dataset.hostId, f.hostId);
    expect(observed.dataset.targetHash, 'a' * 64);
    expect(observed.association.peerDigest, association.peerDigest);
    expect(observed.association.associationDigest, association.associationDigest);
    expect(observed.receivedAt, received);
    expect(observations, 1);
    expect(port.requests, hasLength(1));
    expect(port.statuses, isEmpty);
    expect(handle.reads, 1);
    expect(
      store.read(
        host: f.host,
        profileId: f.profile,
        notificationScopeId: selection.notificationScopeId,
        scopeOwner: owner,
        epoch: 8,
      ),
      isNull,
    );
  });

  test('changed Host, original scope and foreign configured Host cannot reuse heartbeat metadata', () {
    final selection = f.selection(), association = AgentReaderLocator('b' * 64, 'c' * 64);
    final store = AgentHeartbeatObservations(), owner = Object();
    final result = AgentReaderOutcome.parse(jsonEncode(f.result(AgentReaderAction.read)), AgentReaderAction.read);
    void record({bool foreign = false}) => store.record(
      selection: selection,
      association: association,
      dataset: AgentReaderDataset.parse(f.dataset(f.heartbeat()..['host_id'] = foreign ? f.knownId : f.hostId), result),
      host: f.host,
      scopeOwner: owner,
      epoch: 1,
      receivedAt: DateTime.utc(2026),
    );
    AgentHeartbeatObservation? read({String? nonce, Object? currentOwner}) => store.read(
      host: f.host,
      profileId: f.profile,
      notificationScopeId: nonce ?? selection.notificationScopeId,
      scopeOwner: currentOwner ?? owner,
      epoch: 1,
    );
    record(foreign: true);
    expect(read(), isNull);
    record();
    expect(read(currentOwner: Object()), isNull);
    record();
    expect(read(nonce: 'different unlock frame'), isNull);
    record();
    expect(
      store.read(
        host: f.host.withGroup(null),
        profileId: f.profile,
        notificationScopeId: selection.notificationScopeId,
        scopeOwner: owner,
        epoch: 1,
      ),
      isNull,
    );
    record();
    store.clear();
    expect(read(), isNull);
  });

  test('late consumed heartbeat after original retirement never publishes a lifecycle observation', () async {
    final selection = f.selection(), port = f.Port(f.selection()), handle = _Heartbeat(f.selection());
    handle.pending = Completer<String>();
    port.pending = Completer()..complete(handle);
    var original = true, observations = 0;
    final controller = AgentReaderController(
      port: port,
      selection: selection,
      isOriginal: () => original,
      onHeartbeat: (_, _) => observations++,
    );
    addTearDown(controller.dispose);
    await controller.prepare(
      AgentReaderRequest.association(AgentReaderAction.read, AgentReaderLocator('b' * 64, 'c' * 64)),
    );
    await controller.confirm();
    final pending = controller.consumeRead();
    original = false;
    handle.pending!.complete(f.dataset(f.heartbeat()..['host_id'] = f.hostId));
    await pending;
    expect(observations, 0);
    expect(controller.dataset, isNull);
    expect(handle.cancels, greaterThan(0));
  });
}
