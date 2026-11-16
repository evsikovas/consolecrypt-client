import 'dart:async';

import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_association_port.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_route_port.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_saved_route_port.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_observation_controls.dart';
import 'package:consolecrypt/monitoring/monitoring_pinned_observation.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';
import 'package:consolecrypt/monitoring/monitoring_workspace.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/monitoring_fakes.dart';
import '../helpers/test_app.dart' show tapKey, enterKey;
import '../unit/agent_tunnel_route_test.dart' as f;
import '../unit/monitoring_agent_route_test.dart' as m;
import '../unit/monitoring_service_test.dart' show outcome;

class _RetainedPort extends Fake implements AgentTunnelRoutePort, AgentTunnelSavedRoutePort {
  _RetainedPort(this.savedIntents);
  @override
  final AgentTunnelSavedIntentCache savedIntents;
}

class _Store extends MonitoringTestStore {
  int writes = 0;
  @override
  Future<MonitoringSource> upsert(MonitoringSource source, {bool create = false}) {
    writes++;
    return super.upsert(source, create: create);
  }
}

class _LocalAssociations extends Fake implements AgentAssociationPort {
  int reads=0, prepares=0;
  bool available=true;
  Completer<AgentAssociationStatus>? response;
  @override
  Future<AgentAssociationStatus> status(RuntimeNoticeScope scope) async {
    reads++;
    final pending=response;if(pending!=null) return pending.future;
    return AgentAssociationStatus(scope:scope,clientIdentityState:AgentClientIdentityState.missing,
      hosts:[AgentAssociationHost(id:f.host,revision:f.intent().record.hostRevision!,name:'Owned Agent host')],
      records:available?[f.intent().record]:[]);
  }
  @override
  Future<AgentClientIdentityHandle> prepareClientIdentity(RuntimeNoticeScope scope) async {
    prepares++;throw StateError('No identity preparation in a source editor');
  }
  @override
  Future<AgentAssociationHandle> registerProbe(RuntimeNoticeScope scope,AgentAssociationHost host) async {
    prepares++;throw StateError('No enrollment or grant in a source editor');
  }
}

class _Harness {
  DateTime now = DateTime.utc(2030);
  bool current = true;
  final requests = <Json>[];
  final store = _Store();
  final associations=_LocalAssociations();
  MonitoringPinnedObservation? observation;
  Completer<Json>? response;
  late final cache = m.retainedCache(now: () => now);
  late final service = AppCoreMonitoringService(
    agentTunnelRoutePort: _RetainedPort(cache),
    agentAssociationPort: associations,
    read: (r) async {
      requests.add(r);
      return response?.future ??
          {'sourceId': r['sourceId'], 'sourceRevision': r['sourceRevision'], 'result': outcome()};
    },
    cancelOperation: (_) async {},
  );
  Future<void> pump(WidgetTester t, {RuntimeNoticeScope? scope}) async {
    await t.binding.setSurfaceSize(const Size(1250, 1100));
    await t.pumpWidget(
      MaterialApp(
        theme: AppTheme.dark(platform: TargetPlatform.windows),
        supportedLocales: AppLocalizations.supportedLocales,
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        home: Scaffold(
          body: MonitoringWorkspace(
            service: service,
            store: store,
            observation: observation,
            sshRouteHosts: const [MonitoringHostRef(id:f.host,name:'Owned Agent host')],
            panelSyncScope: scope ?? f.scope,
            panelSyncCurrent: () => current,
          ),
        ),
      ),
    );
    await t.pumpAndSettle();
  }

  Future<void> select(WidgetTester t) async {
    final control = find.byKey(const ValueKey('monitoring-agent-route'));
    await t.ensureVisible(control);
    t.widget<GlassSelect<int?>>(control).onChanged!(cache.retainedRoutes(f.scope).single.metadataVersion);
    await t.pumpAndSettle();
  }
}

void main() {
  testWidgets('manual saved Agent result remains one-shot without observation adoption or pin', (t) async {
    addTearDown(() => t.binding.setSurfaceSize(null));
    final h=_Harness();var created=0;
    final row=h.cache.retainedRoutes(f.scope).single;
    h.store.sources=[MonitoringSource(id:monitoringFixture.id,revision:1,name:'Agent one-shot',endpoint:'https://logical.example.invalid',
      route:SourceRoute.agentDestination,hostId:row.hostId,associationId:row.associationId,destinationIp:row.destinationIp,destinationPort:row.destinationPort)];
    final owner=MonitoringPinnedObservation(createTransport:(){created++;throw StateError('No background transport');},isCurrent:()=>true);
    h.observation=owner;addTearDown(owner.dispose);await h.pump(t);
    await tapKey(t,'monitoring-query');
    expect(h.requests,hasLength(1));expect(h.requests.single.containsKey('selectedAgentRouteId'),isFalse);
    final controls=t.widget<MonitoringObservationControls>(find.byType(MonitoringObservationControls));
    expect(controls.enabled,isFalse);expect(controls.acceptedPlan,isNull);expect(owner.state.active,isFalse);expect(created,0);
    expect(t.takeException(),isNull);
  });

  testWidgets('new saved Agent destination writes only DATA and reloads without preparing or reading a grant', (t) async {
    addTearDown(() => t.binding.setSurfaceSize(null));
    final h=_Harness();h.store.sources=[];h.now=DateTime.utc(2031);await h.pump(t);
    await enterKey(t,'monitoring-source-name','Agent API');
    await enterKey(t,'monitoring-source-endpoint','https://logical.example.invalid:9443');
    final routes=t.widget<GlassSelect<SourceRoute>>(find.byKey(const ValueKey('monitoring-source-route')));
    expect(routes.items.any((item)=>item.value==SourceRoute.agentDestination),isTrue);
    routes.onChanged!(SourceRoute.agentDestination);await t.pumpAndSettle();
    expect(h.cache.retainedRoutes(f.scope),isEmpty);
    await tapKey(t,'monitoring-source-agent-status');
    final picker=find.byKey(const ValueKey('monitoring-source-agent-destination'));
    t.widget<GlassSelect<String?>>(picker).onChanged!(f.association);await t.pumpAndSettle();
    await enterKey(t,'monitoring-source-agent-port','9090');
    await tapKey(t,'monitoring-save-source');
    final saved=h.store.sources.single;
    expect(saved.toJson()['route'],{'kind':'agentDestination','associationId':f.association,
      'hostId':f.host,'destinationIp':'127.0.0.1','destinationPort':9090});
    expect(saved.endpoint,'https://logical.example.invalid:9443');
    expect(h.associations.reads,1);expect(h.associations.prepares,0);
    expect(saved.routeId,isNull);expect(h.store.writes,1);expect(h.requests,isEmpty);
    h.now=DateTime.utc(2031);
    await t.pumpWidget(const SizedBox.shrink());await t.pumpAndSettle();
    await h.pump(t);
    expect(h.store.sources.single.toJson(),saved.toJson());
    expect(h.requests,isEmpty);expect(h.store.writes,1);expect(h.associations.reads,1);
    expect(t.widget<GlassSelect<int?>>(find.byKey(const ValueKey('monitoring-agent-route'))).items
      .map((item)=>item.value),[null]);
    final edit=find.text('Edit source');await t.ensureVisible(edit);await t.tap(edit);await t.pumpAndSettle();
    final reference=t.widget<Text>(find.byKey(const ValueKey('monitoring-source-agent-reference'))).data!;
    expect(reference,contains('Owned Agent host'));expect(reference,contains('127.0.0.1:9090'));
    expect(reference,isNot(contains(f.association)));expect(reference,isNot(contains(f.host)));
    await tapKey(t,'monitoring-save-source');
    expect(h.store.sources.single.route,SourceRoute.agentDestination);expect(h.store.sources.single.toJson(),saved.toJson());
    expect(h.store.writes,2);expect(h.requests,isEmpty);expect(h.associations.reads,1);expect(h.associations.prepares,0);
    expect(t.takeException(),isNull);
  });
  testWidgets('new Agent destination cannot save an unobserved reference', (t) async {
    addTearDown(() => t.binding.setSurfaceSize(null));
    final h=_Harness();h.store.sources=[];h.associations.available=false;await h.pump(t);
    await enterKey(t,'monitoring-source-name','Agent API');
    await enterKey(t,'monitoring-source-endpoint','https://logical.example.invalid:9443');
    t.widget<GlassSelect<SourceRoute>>(find.byKey(const ValueKey('monitoring-source-route')))
      .onChanged!(SourceRoute.agentDestination);await t.pumpAndSettle();
    await tapKey(t,'monitoring-source-agent-status');
    final picker=t.widget<GlassSelect<String?>>(find.byKey(const ValueKey('monitoring-source-agent-destination')));
    expect(picker.items.map((item)=>item.value),[null]);
    picker.onChanged!(f.association);
    await enterKey(t,'monitoring-source-agent-port','9090');
    await tapKey(t,'monitoring-save-source');
    expect(h.store.sources,isEmpty);expect(h.store.writes,0);expect(h.requests,isEmpty);expect(h.associations.prepares,0);
    expect(t.takeException(),isNull);
  });

  testWidgets('late local association metadata cannot populate a retired source edit scope', (t) async {
    addTearDown(() => t.binding.setSurfaceSize(null));
    final h=_Harness();h.store.sources=[];h.associations.response=Completer<AgentAssociationStatus>();await h.pump(t);
    await enterKey(t,'monitoring-source-name','Agent API');
    await enterKey(t,'monitoring-source-endpoint','https://logical.example.invalid:9443');
    t.widget<GlassSelect<SourceRoute>>(find.byKey(const ValueKey('monitoring-source-route')))
      .onChanged!(SourceRoute.agentDestination);await t.pumpAndSettle();
    t.widget<OutlinedButton>(find.byKey(const ValueKey('monitoring-source-agent-status'))).onPressed!();await t.pump();
    h.current=false;
    h.associations.response!.complete(AgentAssociationStatus(scope:f.scope,clientIdentityState:AgentClientIdentityState.missing,
      hosts:[AgentAssociationHost(id:f.host,revision:f.intent().record.hostRevision!,name:'Owned Agent host')],records:[f.intent().record]));
    await t.pumpAndSettle();
    final picker=t.widget<GlassSelect<String?>>(find.byKey(const ValueKey('monitoring-source-agent-destination')));
    expect(picker.items.map((item)=>item.value),[null]);picker.onChanged!(f.association);
    await enterKey(t,'monitoring-source-agent-port','9090');await tapKey(t,'monitoring-save-source');
    expect(h.store.writes,0);expect(h.requests,isEmpty);expect(h.associations.prepares,0);expect(t.takeException(),isNull);
  });
  testWidgets('saved-source query explicitly selects retained original once without source writes or automatic reads', (
    t,
  ) async {
    addTearDown(() => t.binding.setSurfaceSize(null));
    final h = _Harness();
    await h.pump(t);
    expect(h.requests, isEmpty);
    await h.select(t);
    expect(h.requests, isEmpty);
    expect(h.store.writes, 0);
    await tapKey(t, 'monitoring-query');
    expect(h.requests, hasLength(1));
    expect(h.requests.single['selectedAgentRouteId'], m.observedRoute);
    expect(h.requests.single['sourceId'], monitoringFixture.id);
    expect(h.requests.single['sourceRevision'], monitoringFixture.revision);
    expect(h.store.sources.single.toJson(), monitoringFixture.toJson());
    await t.pump(const Duration(seconds: 20));
    expect(h.requests, hasLength(1));
    expect(h.store.writes, 0);
    expect(t.takeException(), isNull);
  });
  testWidgets('mismatched destination and expired selected route deny without direct fallback or source mutation', (
    t,
  ) async {
    addTearDown(() => t.binding.setSurfaceSize(null));
    final foreignHost = _Harness();
    foreignHost.store.sources = [const MonitoringSource(id: 'foreign-source', revision: 1,
      name: 'Other SSH Host', endpoint: 'http://127.0.0.1:9090', route: SourceRoute.sshHost,
      hostId: '80000000-0000-4000-8000-000000000008')];
    await foreignHost.pump(t);
    expect(t.widget<GlassSelect<int?>>(find.byKey(const ValueKey('monitoring-agent-route'))).items
      .map((item) => item.value), [null]);
    expect(foreignHost.requests, isEmpty);
    expect(foreignHost.store.writes, 0);
    final h = _Harness();
    h.store.sources = [
      const MonitoringSource(
        id: 'fixture',
        revision: 1,
        name: 'Another destination',
        endpoint: 'http://127.0.0.1:9091',
      ),
    ];
    await h.pump(t);
    await h.select(t);
    await tapKey(t, 'monitoring-query');
    expect(h.requests, isEmpty);
    expect(h.store.writes, 0);
    h.now = DateTime.utc(2030, 1, 1, 0, 1);
    await tapKey(t, 'monitoring-query');
    expect(h.requests, isEmpty);
    expect(h.store.writes, 0);
    expect(t.takeException(), isNull);
  });
  testWidgets('scope retires selected override and drops late read result rather than adopting it', (t) async {
    addTearDown(() => t.binding.setSurfaceSize(null));
    final h = _Harness()..response = Completer<Json>();
    await h.pump(t);
    await h.select(t);
    await tapKey(t, 'monitoring-query');
    expect(h.requests, hasLength(1));
    h.current = false;
    h.response!.complete({
      'sourceId': monitoringFixture.id,
      'sourceRevision': monitoringFixture.revision,
      'result': outcome(),
    });
    await t.pumpAndSettle();
    expect(find.text('partial fixture warning'), findsNothing);
    expect(h.requests, hasLength(1));
    expect(h.store.writes, 0);
    expect(t.takeException(), isNull);
  });
}
