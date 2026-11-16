import 'dart:convert';

import 'package:consolecrypt/automation/automation_agent_installer.dart';
import 'package:consolecrypt/automation/automation_monitoring_node.dart';
import 'package:consolecrypt/automation/automation_monitoring_node_source.dart';
import 'package:consolecrypt/core/bridge/rust_automation.dart';
import 'package:consolecrypt/src/rust/frb_generated.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/agent_installer_fixture.dart' as f;

AgentInstallerContext metadata() =>
    AgentInstallerContext.parse(f.context(), AgentInstallerFrame(profileId: f.profileId, notificationScopeId: f.nonce));
Map<String, dynamic> request(AgentInstallerContext c, {MonitoringNodeOperation op = MonitoringNodeOperation.install}) =>
    MonitoringNodePlan().request(context: c, host: c.hosts.single, name: 'Owned monitoring node', operation: op);
// Schema-shaped source-project response, never an installed node/version proof.
Map<String, dynamic> created(Map<String, dynamic> r) => f.created(r)..['claimedRelease'] = null;

class Api extends Fake implements RustLibApi {
  final requests = <Map<String, dynamic>>[];
  @override
  Future<String> crateApiAgentInstallerAutomationMonitoringNodeCreateJson({required String requestJson}) async {
    final r = jsonDecode(requestJson) as Map<String, dynamic>;
    requests.add(r);
    return jsonEncode(created(r));
  }
}

void main() {
  test('plan matches actual Native eight snake fields and fixed bounds, storage reserve and sample estimate', () {
    final p = MonitoringNodePlan();
    expect(p.toJson(), {
      'cpu_cores': 2,
      'memory_mib': 2048,
      'disk_gib': 50,
      'retention_days': 15,
      'retention_gib': 40,
      'expected_samples_per_second': 2000,
      'receiver_enabled': false,
      'alertmanager_enabled': false,
    });
    expect(p.estimatedSampleBytes, 5184000000);
    for (final make in <MonitoringNodePlan Function()>[
      () => MonitoringNodePlan(memoryMib: 2047),
      () => MonitoringNodePlan(cpuCores: 1),
      () => MonitoringNodePlan(retentionGib: 41),
      () => MonitoringNodePlan(expectedSamplesPerSecond: 1000000),
    ]) {
      expect(make, throwsFormatException);
    }
  });
  test('request freezes original frame and actual host revision, no path, release key, execution or Agent scopes', () {
    final c = metadata(), r = request(metadata());
    expect(
      () => MonitoringNodePlan().request(
        context: c,
        host: metadata().hosts.single,
        name: 'Node',
        operation: MonitoringNodeOperation.install,
      ),
      throwsFormatException,
    );
    expect(r.keys.toSet(), {
      'version',
      'profileId',
      'notificationScopeId',
      'name',
      'operation',
      'hostId',
      'expectedHostRevision',
      'plan',
    });
    expect(r['expectedHostRevision'], '9007199254740993');
    expect(r['notificationScopeId'], f.nonce);
    expect(request(c, op: MonitoringNodeOperation.restore)['operation'], 'restore');
  });
  test('actual closed producer shape accepts install with null release and rejects authority/version/frame/project substitutions', () {
    final c = metadata(), r = request(c), raw = created(r);
    final value = MonitoringNodeSourceCreated.parse(raw, c, r);
    expect(value.project.id, f.projectId);
    expect(value.projectDigest, 'a' * 64);
    for (final mutate in <void Function(Map<String, dynamic>)>[
      (v) => v['claimedRelease'] = {'signatureVerified': true},
      (v) => v['actualRuntimeVersion'] = '3.16.0',
      (v) => v['notificationScopeId'] = 'other',
      (v) => (v['project'] as Map)['read_only'] = true,
      (v) => (v['project'] as Map)['revision'] = 1.0,
      (v) => v['ready'] = true,
    ]) {
      final v = jsonDecode(jsonEncode(raw)) as Map<String, dynamic>;
      mutate(v);
      expect(() => MonitoringNodeSourceCreated.parse(v, c, r), throwsFormatException);
    }
  });
  test('production optional facade forwards actual generated monitoring-node method lazily and exactly once', () async {
    final api = Api();
    RustLib.initMock(api: api);
    addTearDown(RustLib.dispose);
    final AutomationMonitoringNodeService service = RustAutomationService();
    expect(api.requests, isEmpty);
    final c = metadata(), r = request(c);
    final raw = await service.createMonitoringNodeProject(r);
    expect(api.requests, [r]);
    expect(MonitoringNodeSourceCreated.parse(raw, c, r).project.id, f.projectId);
  });
}
