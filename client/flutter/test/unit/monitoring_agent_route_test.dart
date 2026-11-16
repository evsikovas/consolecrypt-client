import 'dart:convert';

import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_route_wire.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_saved_route_port.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_pinned_observation.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';
import 'package:flutter_test/flutter_test.dart';

import '../helpers/monitoring_fakes.dart';
import 'agent_tunnel_route_test.dart' as f;
import 'agent_tunnel_saved_route_test.dart' as s;
import 'monitoring_service_test.dart' show outcome;

const observedRoute = '70000000-0000-4000-8000-000000000007';
AgentTunnelSavedIntentCache retainedCache({DateTime Function()? now, bool withId = true}) {
  final cache = AgentTunnelSavedIntentCache(now: now ?? () => DateTime.utc(2030));
  final view = parseAgentTunnelRouteView(
    jsonEncode({...s.savedView(), if (withId) 'routeId': observedRoute}),
    f.scope,
    s.savedIntent(),
  );
  final receipt = parseAgentTunnelDetachReceipt(jsonEncode(s.detachWire()), view);
  cache.retainValidated(f.scope, s.savedIntent(), receipt, 60000);
  return cache;
}

void main() {
  test('one-use Agent sources cannot create or adopt background observation transport', () {
    var created=0;
    final owner=MonitoringPinnedObservation(createTransport:(){created++;throw StateError('No background transport');},isCurrent:()=>true);
    owner.setForegroundActive(true);
    final row=retainedCache().retainedRoutes(f.scope).single;
    for(final source in [
      MonitoringSource(id:'saved',revision:1,name:'Agent',endpoint:'http://127.0.0.1:9090',route:SourceRoute.agentDestination,
        hostId:row.hostId,associationId:row.associationId,destinationIp:row.destinationIp,destinationPort:row.destinationPort),
      const MonitoringSource(id:'legacy',revision:1,name:'Legacy',endpoint:'http://127.0.0.1:9090',route:SourceRoute.agentTunnel,routeId:observedRoute),
    ]) {
      final plan=MonitoringPinPlan(source:source,expression:'up',interval:MonitoringPinInterval.seconds15);
      expect(owner.pin(plan),isFalse);
      expect(owner.observeForeground(plan,MonitoringObservedQuery(MonitoringTestService().result,null)),isFalse);
      expect(owner.state.active,isFalse);
    }
    expect(created,0);owner.dispose();
  });

  test('saved Agent destination persists only canonical association Host and endpoint data', () {
    final row = retainedCache().retainedRoutes(f.scope).single;
    final source = MonitoringSource(id: 'saved', name: 'Agent destination', endpoint: 'http://127.0.0.1:9090',
      route: SourceRoute.agentDestination, hostId: row.hostId, associationId: row.associationId,
      destinationIp: row.destinationIp, destinationPort: row.destinationPort);
    source.validate();
    final json = jsonDecode(jsonEncode(source.toJson())) as Json;
    expect(json['route'], {'kind':'agentDestination','associationId':s.savedIntent().record.id,
      'hostId':f.host,'destinationIp':'127.0.0.1','destinationPort':9090});
    expect(MonitoringSource.fromJson(json).withRecord(revision: 7).toJson(), source.toJson());
    expect(jsonEncode(json), isNot(contains(observedRoute)));
    for (final key in ['grant','grantId','certificate','expiresUtc','routeId','handle','lease']) {
      expect((json['route'] as Map).containsKey(key), isFalse);
    }
    final legacy = MonitoringSource.fromJson({...json, 'route':{'kind':'agentTunnel','routeId':observedRoute}});
    expect(legacy.route, SourceRoute.agentTunnel);
    expect(legacy.route, isNot(SourceRoute.direct));
  });
  test('saved Agent destination denies mixed fields but preserves logical URL independent of physical target', () {
    final row=retainedCache().retainedRoutes(f.scope).single;
    final source=MonitoringSource(id:'saved', name:'Agent',endpoint:'http://127.0.0.1:9090',
      route:SourceRoute.agentDestination,hostId:row.hostId,associationId:row.associationId,
      destinationIp:row.destinationIp,destinationPort:row.destinationPort);
    final valid=source.toJson();
    for (final endpoint in ['https://metrics.example.invalid:9443/prefix','http://10.20.30.40:3000/api/datasources/proxy/1']) {
      final restored=MonitoringSource.fromJson({...valid,'endpoint':endpoint});
      expect(restored.endpoint,endpoint);expect(restored.destinationIp,'127.0.0.1');expect(restored.destinationPort,9090);
    }
    for (final bad in [
      {...valid,'route':{...(valid['route'] as Map),'routeId':observedRoute}},
      {...valid,'route':{...(valid['route'] as Map),'associationId':'00000000-0000-0000-0000-000000000000'}},
      {...valid,'route':{...(valid['route'] as Map),'destinationPort':0}},
      {...valid,'route':{...(valid['route'] as Map),'destinationPort':9090.0}},
    ]) {
      expect(() => MonitoringSource.fromJson(bad), throwsA(isA<MonitoringFailure>()));
    }
  });

  test('route view accepts exact legacy or canonical observed routeId and denies null malformed or extra keys', () {
    expect(parseAgentTunnelRouteView(jsonEncode(f.view()), f.scope, f.intent()).routeId, isNull);
    expect(
      parseAgentTunnelRouteView(jsonEncode({...f.view(), 'routeId': observedRoute}), f.scope, f.intent()).routeId,
      observedRoute,
    );
    for (final bad in [
      null,
      '',
      1,
      observedRoute.toUpperCase().replaceFirst('7', 'A'),
      '00000000-0000-0000-0000-000000000000',
      '$observedRoute ',
    ]) {
      expect(
        () => parseAgentTunnelRouteView(jsonEncode({...f.view(), 'routeId': bad}), f.scope, f.intent()),
        throwsFormatException,
      );
    }
    expect(
      () => parseAgentTunnelRouteView(
        jsonEncode({...f.view(), 'routeId': observedRoute, 'ready': true}),
        f.scope,
        f.intent(),
      ),
      throwsFormatException,
    );
  });
  test('retained route metadata requires original detach routeId scope expiry and exact numeric destination', () {
    var now = DateTime.utc(2030);
    final cache = retainedCache(now: () => now), row = cache.retainedRoutes(f.scope).single;
    expect(row.routeId, observedRoute);
    expect(row.hostId, f.host);
    expect(row.hostRevision, s.savedIntent().record.hostRevision);
    expect(row.matchesEndpoint('http://127.0.0.1:9090/tenant/prometheus'), isTrue);
    for (final endpoint in ['http://localhost:9090', 'http://127.0.0.1:9091', 'http://[::1]:9090']) {
      expect(row.matchesEndpoint(endpoint), isFalse);
    }
    expect(retainedCache(withId: false).retainedRoutes(f.scope), isEmpty);
    final foreign = RuntimeNoticeScope.fromProfile({
      'id': f.profile,
      'active': true,
      'vault_state': 'unlocked',
      'notification_scope_id': 'another original',
    })!;
    expect(cache.retainedRoutes(foreign), isEmpty);
    now = DateTime.utc(2030, 1, 1, 0, 1);
    expect(cache.retainedRoutes(f.scope), isEmpty);
  });
  test('actual read callback forwards transient routeId with saved source/revision and unchanged query; ordinary read has none', () async {
    final requests = <Json>[];
    const source = monitoringFixture;
    final original = source.toJson();
    final service = AppCoreMonitoringService(
      read: (r) async {
        requests.add(r);
        return {'sourceId': source.id, 'sourceRevision': source.revision, 'result': outcome()};
      },
      cancelOperation: (_) async {},
    );
    const query = MonitoringQuery(expression: 'up', start: 1, end: 10, stepSeconds: 3);
    await service.queryViaAgentRoute(source, query, routeId: observedRoute);
    expect(requests.single.keys.toSet(), {
      'operationId',
      'sourceId',
      'sourceRevision',
      'selectedAgentRouteId',
      'action',
    });
    expect(requests.single['sourceId'], source.id);
    expect(requests.single['sourceRevision'], source.revision);
    expect(requests.single['selectedAgentRouteId'], observedRoute);
    expect(requests.single['action'], query.toOperation());
    expect(source.toJson(), original);
    await service.query(source, query);
    expect(requests.last.containsKey('selectedAgentRouteId'), isFalse);
    await expectLater(
      service.queryViaAgentRoute(source, query, routeId: 'not an observed UUID'),
      throwsFormatException,
    );
    expect(requests, hasLength(2));
  });
}
