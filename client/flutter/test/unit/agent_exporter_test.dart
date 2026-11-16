import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_association_wire.dart';
import 'package:consolecrypt/monitoring/agent_exporter.dart';
import 'package:consolecrypt/monitoring/agent_exporter_controller.dart';
import 'package:consolecrypt/monitoring/agent_exporter_policy.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/agent_association_fixture.dart' as old;
import '../support/agent_exporter_fixture.dart' as f;

AgentExporterController owner(f.Calls c, {bool Function()? original, Duration Function()? elapsed}) =>
    AgentExporterController(
      port: c.port,
      scope: f.scope,
      record: f.record(),
      original: original ?? () => true,
      now: () => f.now,
      elapsed: elapsed,
    );
void main() {
  test('closed catalog binds exact original Host revision purpose and Node; unknown fields denied', () {
    final parsed = parseAgentExporterCatalog(jsonEncode(f.catalogWire()), f.scope, f.record());
    expect(parsed.policy.address, '127.0.0.1:19100');
    for (final change in [
      <String, dynamic>{'hostRevision': '2'},
      {'purpose': 'metrics'},
      {'runtimeObserved': true},
      {'extra': 'secret'},
    ]) {
      expect(
        () => parseAgentExporterCatalog(jsonEncode({...f.catalogWire(), ...change}), f.scope, f.record()),
        throwsFormatException,
      );
    }
  });
  test('full exporter view preserves installed metadata and unknown executing version; no source Ready inference', () {
    final c = parseAgentExporterCatalog(jsonEncode(f.catalogWire()), f.scope, f.record());
    expect(parseAgentExporterView(jsonEncode(f.view()), f.scope, f.record(), c).runningVersion, isNull);
    final observedUnknown = {...f.view(), 'phase': 'unknown', 'actorAdmission': true, 'runtimeObserved': true};
    expect(parseAgentExporterView(jsonEncode(observedUnknown), f.scope, f.record(), c).actorAdmission, isTrue);
    for (final change in [
      <String, dynamic>{'version': 1.0},
      {'runningVersion': 'v1.12.1'},
      {'phase': 'running'},
      {'automaticRenewal': true},
      {'installationPerformed': true},
      {'approved': true},
    ]) {
      expect(
        () => parseAgentExporterView(jsonEncode({...f.view(), ...change}), f.scope, f.record(), c),
        throwsFormatException,
      );
    }
  });
  test('policy denies wrong platform, receiver, collector and extra caller authority', () {
    for (final change in [
      <String, dynamic>{'bindIp': '0.0.0.0'},
      {
        'collectors': ['cpu', 'meminfo', 'textfile'],
      },
      {'binaryPath': '/tmp/other'},
    ]) {
      expect(() => parseAgentExporterPolicy({...f.policy(), ...change}, f.host), throwsFormatException);
    }
    final p = f.policy();
    (p['artifact'] as Map)['platform'] = 'windows-x86_64';
    expect(() => parseAgentExporterPolicy(p, f.host), throwsFormatException);
  });
  test('catalog and prepare remain explicit and one use; callback receives no install grant or path', () async {
    final c = f.Calls(), o = owner(c);
    expect(c.requests, isEmpty);
    await o.readCatalog();
    await o.readCatalog();
    expect(c.requests['catalog']!.length, 1);
    await o.prepare(120);
    await o.prepare(120);
    expect(c.requests['prepare']!.single.keys.toSet(), {
      'version',
      'profileId',
      'notificationScopeId',
      'associationId',
      'manifestSha256',
      'durationSeconds',
    });
    expect(c.raw.calls, ['details']);
    await o.close();
    o.dispose();
  });
  test('review details cannot reset original review or session deadline', () async {
    var elapsed = Duration.zero;
    final c = f.Calls(), o = owner(c, elapsed: () => elapsed);
    await o.readCatalog();
    await o.prepare(120);
    expect(o.remainingMs, 60000);
    elapsed = const Duration(seconds: 40);
    await o.refresh();
    expect(o.remainingMs, 20000);
    elapsed = const Duration(seconds: 61);
    expect(o.canApprove, isFalse);
    await o.close();
    o.dispose();
  });
  test('approval is spent on failure and Unknown cleanup never means stopped or Closed', () async {
    final c = f.Calls(), o = owner(c);
    c.raw.approval = Completer<String>();
    c.raw.cleanup = 'unknown';
    await o.readCatalog();
    await o.prepare(120);
    final approval = o.approveOnce();
    await Future<void>.delayed(Duration.zero);
    c.raw.approval!.completeError(StateError('synthetic lost reply'));
    await approval;
    await o.approveOnce();
    expect(c.raw.calls.where((x) => x == 'approve').length, 1);
    expect(o.unknown, isTrue);
    final cleanup = await o.close();
    expect(cleanup!.closedObserved, isFalse);
    expect(c.raw.calls, containsAllInOrder(['cancel', 'joined', 'dispose']));
    o.dispose();
  });
  test('late original prepare after retirement cancels joins and releases exactly that issued object', () async {
    final c = f.Calls();
    c.pending = Completer();
    var current = true;
    final o = owner(c, original: () => current);
    await o.readCatalog();
    final pending = o.prepare(120);
    await Future<void>.delayed(Duration.zero);
    current = false;
    final closed = o.close();
    c.pending!.complete(c.raw);
    await pending;
    await closed;
    expect(c.raw.calls, ['cancel', 'joined', 'dispose']);
    expect(o.view, isNull);
    o.dispose();
  });
  test('Stop is a request; only explicit joined result supplies cleanup observation', () async {
    var elapsed = Duration.zero;
    final c = f.Calls(), o = owner(c, elapsed: () => elapsed);
    await o.readCatalog();
    await o.prepare(120);
    await o.approveOnce();
    expect(o.view!.runtimeObserved, isTrue);
    await o.stop();
    expect(o.view!.phase, AgentExporterPhase.running);
    expect(o.cleanup, isNull);
    await o.observeCleanup();
    expect(o.cleanup!.closedObserved, isTrue);
    elapsed = const Duration(hours: 1);
    final calls = List<String>.of(c.raw.calls);
    await o.stop();
    await o.approveOnce();
    await o.refresh();
    expect(c.raw.calls, calls);
    expect(o.canStop, isFalse);
    expect(o.canApprove, isFalse);
    expect(o.canRefresh, isFalse);
    expect(o.unknown, isFalse);
    expect(o.view!.runtimeObserved, isTrue);
    expect(o.view!.phase, AgentExporterPhase.running);
    await o.close();
    o.dispose();
  });
  test('explicit ExporterOnly status is never eligible for tunnel or Metrics; legacy v1 parser preserved', () {
    expect(f.record().canRequestTunnel, isFalse);
    expect(f.record().canRequestMetricsCollection, isFalse);
    expect(canRequestAgentExporter(f.record(purpose: AgentAssociationPurpose.metrics)), isFalse);
    final status = old.statusWire();
    status['associations'] = [
      {
        'associationId': f.association,
        'hostId': f.host,
        'hostRevision': '9007199254740993',
        'agentId': f.agent,
        'nodeKeyFingerprint': 'a' * 64,
        'certificateDigest': 'b' * 64,
        'certificateExpiresUtc': '2030-01-01T00:02:00Z',
        'state': 'associated',
        'revocationState': 'notRequested',
        'remoteRevocationObserved': false,
        'purpose': 'exporters',
        'scope': 'exporters.manage',
        'scopes': ['exporters.manage'],
        'keyEpoch': '1',
        'rotationState': 'notRequested',
      },
    ];
    expect(
      parseAgentAssociationStatus(jsonEncode(status), old.scope).records.single.purpose,
      AgentAssociationPurpose.exporters,
    );
    final offer = {
      ...old.operationWire(),
      'purpose': 'exporters',
      'scope': 'exporters.manage',
      'scopes': ['exporters.manage'],
      'scopeSource': 'actualAdvertisedIntersection',
      'advertisedScopes': ['exporters.manage'],
      'collector': null,
      'exporter': {'policy': f.policy(), 'manifestSha256': '1' * 64},
    };
    expect(
      parseAgentAssociationReview(
        jsonEncode(offer),
        old.scope,
        old.selectedHost,
        AgentAssociationReviewStage.offer,
        expectedPurpose: AgentAssociationPurpose.exporters,
      ).exporterCatalog,
      isNotNull,
    );
    expect(() => parseAgentAssociationStatus(jsonEncode(old.statusWire()), old.scope), returnsNormally);
  });
}
