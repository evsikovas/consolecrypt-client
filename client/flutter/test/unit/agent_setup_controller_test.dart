import 'dart:async';

import 'package:consolecrypt/monitoring/agent_association_port.dart';
import 'package:consolecrypt/monitoring/agent_setup_controller.dart';
import 'package:consolecrypt/monitoring/agent_setup_models.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/agent_setup_fixture.dart' as f;

void main() {
  test('missing release and destination allow read-only preflight but never installation', () async {
    final port = f.SetupPort()
      ..releaseAvailable = false
      ..destinationAvailable = false;
    final c = AgentSetupController(port: port, isCurrent: () => true);
    addTearDown(c.dispose);
    await c.load();
    expect(c.canPrepare, true);
    await c.prepare();
    expect(c.plan!.blocker, AgentSetupIssue.releaseUnavailable);
    expect(c.canInstall, false);
    await c.install();
    expect(port.operations.single.calls, isEmpty);
  });

  test('software review never silently approves owner or collector permissions', () async {
    final port = f.SetupPort();
    final c = AgentSetupController(port: port, isCurrent: () => true);
    addTearDown(c.dispose);
    await c.load();
    await c.prepare();
    await c.install();
    final operation = port.operations.single;
    expect(operation.calls, ['install', 'pair']);
    expect(c.step, AgentSetupStep.offerReview);
    expect(operation.pairing.offers, 0);
    expect(operation.pairing.certificates, 0);
    expect(operation.collection.confirms, 0);
    await c.connect();
    expect(c.step, AgentSetupStep.certificateReview);
    expect(operation.pairing.certificates, 0);
    await c.confirmConnection();
    expect(c.step, AgentSetupStep.collectionReview);
    expect(operation.collection.confirms, 0);
    expect(c.canOpenGraphs, false);
  });

  test('configuration recorded is not collector or graph readiness', () async {
    final port = f.SetupPort();
    final c = AgentSetupController(port: port, isCurrent: () => true);
    addTearDown(c.dispose);
    await c.load();
    await c.prepare();
    await c.install();
    await c.connect();
    await c.confirmConnection();
    await c.enableCollection();
    expect(c.step, AgentSetupStep.awaitingEvidence);
    expect(c.canOpenGraphs, false);
    final operation = port.operations.single..ready = true;
    await c.verify();
    expect(c.step, AgentSetupStep.complete);
    expect(c.canOpenGraphs, true);
    expect(operation.calls.where((call) => call == 'install').length, 1);
    expect(operation.collection.confirms, 1);
  });

  test('status-only verification can finish without metric collection or graphs', () async {
    final port = f.SetupPort()
      ..existingStatusOnly = true
      ..releaseAvailable = false;
    final c = AgentSetupController(port: port, isCurrent: () => true);
    addTearDown(c.dispose);
    await c.load();
    await c.prepare();
    port.operations.single.ready = true;
    await c.install();
    expect(c.step, AgentSetupStep.complete);
    expect(c.canOpenGraphs, false);
    expect(port.operations.single.calls, ['verify']);
  });

  test('scope retirement during software delivery cannot start owner pairing', () async {
    var live = true;
    final port = f.SetupPort();
    final c = AgentSetupController(port: port, isCurrent: () => live);
    await c.load();
    await c.prepare();
    final operation = port.operations.single..pendingInstall = Completer();
    final pending = c.install();
    live = false;
    c.dispose();
    operation.pendingInstall!.complete(operation.software());
    await pending;
    expect(operation.calls, ['install', 'cancel']);
    expect(operation.pairing.offers, 0);
  });

  test('late preflight operation is joined when the original scope has retired', () async {
    var live = true;
    final port = f.SetupPort()..pendingPrepare = Completer();
    final c = AgentSetupController(port: port, isCurrent: () => live);
    await c.load();
    final pending = c.prepare();
    live = false;
    c.dispose();
    final operation = f.SetupOperation(
      AgentSetupSelection(host: f.host, profile: f.metricsProfile, destination: f.destination),
    );
    port.pendingPrepare!.complete(operation);
    await pending;
    await c.cleanup;
    expect(operation.calls, ['cancel']);
    expect(operation.pairing.offers, 0);
  });

  test('busy cancellation joins late child creation and prevents any owner approval', () async {
    final port = f.SetupPort();
    final c = AgentSetupController(port: port, isCurrent: () => true);
    addTearDown(c.dispose);
    await c.load();
    await c.prepare();
    final operation = port.operations.single
      ..pendingPairing = Completer<AgentAssociationHandle>()
      ..pendingCleanup = Completer<AgentSetupCleanup>();
    final install = c.install();
    await Future<void>.delayed(Duration.zero);
    expect(c.busy, true);
    final stop = c.cancel();
    expect(c.stopping, true);
    expect(operation.calls, ['install', 'pair', 'cancel']);
    operation.pendingPairing!.complete(operation.pairing);
    await install;
    await Future<void>.delayed(Duration.zero);
    expect(operation.pairing.cancels, greaterThan(0));
    expect(c.stopping, true, reason: 'Child cancellation alone is not parent cleanup proof');
    operation.pendingCleanup!.complete(const AgentSetupCleanup(closedObserved: true));
    expect((await stop).closedObserved, true);
    expect(c.issue, AgentSetupIssue.cancelled);
    expect(operation.pairing.offers, 0);
    await c.connect();
    expect(operation.pairing.offers, 0);
  });

  test('unknown cleanup stays explicit instead of reporting cancelled completion', () async {
    final port = f.SetupPort();
    final c = AgentSetupController(port: port, isCurrent: () => true);
    addTearDown(c.dispose);
    await c.load();
    await c.prepare();
    port.operations.single.pendingCleanup = Completer<AgentSetupCleanup>()
      ..complete(const AgentSetupCleanup(closedObserved: false));
    expect((await c.cancel()).closedObserved, false);
    expect(c.issue, AgentSetupIssue.cleanupUnknown);
    expect(c.canInstall, false);
  });
}
