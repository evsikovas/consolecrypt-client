import 'dart:async';

import 'package:consolecrypt/core/bridge/rust_agent_association.dart';
import 'package:consolecrypt/core/bridge/rust_agent_metrics_collector.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/monitoring/agent_association_dialog.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_association_port.dart';
import 'package:consolecrypt/monitoring/agent_rotation.dart';
import 'package:consolecrypt/src/rust/frb_generated.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../support/agent_association_fixture.dart' as a;
import '../support/agent_metrics_fixture.dart' as f;
import '../unit/agent_metrics_enrollment_test.dart' as enrolled;

// Actual generated callback signatures with schema-shaped replies only.
// These UI tests do not prove Native/Agent execution or a real remote ACK.
class Source extends Fake implements AgentAssociationPort,AgentMetricsEnrollmentPort,AgentAssociationRevocationPort,AgentAssociationRotationPort {
 int statuses=0,tunnelRegistrations=0,metricsRegistrations=0,tunnelRevokes=0,metricsRevokes=0;
 @override Future<AgentAssociationStatus> status(RuntimeNoticeScope scope)async{
  statuses++;return AgentAssociationStatus(scope:a.scope,clientIdentityState:AgentClientIdentityState.storedMetadata,
   hosts:[a.selectedHost],records:[f.metricRecord()]);
 }
 @override Future<AgentAssociationHandle> registerProbe(RuntimeNoticeScope scope,AgentAssociationHost host)async{tunnelRegistrations++;throw StateError('Wrong Tunnel factory');}
 @override Future<AgentAssociationHandle> registerMetricsProbe(RuntimeNoticeScope scope,AgentAssociationHost host)async{
  metricsRegistrations++;return const RustAgentAssociationPort().registerMetricsProbe(scope,host);
 }
 @override Future<AgentRevocationHandle> prepareRevocation(RuntimeNoticeScope scope,AgentAssociationRecord record)async{tunnelRevokes++;throw StateError('Wrong Tunnel revoke');}
 @override Future<AgentRevocationHandle> prepareMetricsRevocation(RuntimeNoticeScope scope,AgentAssociationRecord record)async{metricsRevokes++;throw StateError('No recorded reply in this UI fixture');}
}
Widget host(Source source,f.Calls calls,ValueNotifier<bool> original)=>MaterialApp(
 supportedLocales:AppLocalizations.supportedLocales,localizationsDelegates:const[AppLocalizations.delegate,...GlobalMaterialLocalizations.delegates],
 home:Scaffold(body:ValueListenableBuilder<bool>(valueListenable:original,builder:(_,value,_)=>AgentAssociationDialog(
  port:source,scope:a.scope,original:()=>value,metricsPort:RustAgentMetricsCollectorPort(calls:calls)))));
Future<void> tap(WidgetTester t,String key)async{final finder=find.byKey(ValueKey(key)).last;await t.ensureVisible(finder);await t.tap(finder);await t.pump();await t.pump(const Duration(milliseconds:100));}
Future<void> finish(WidgetTester t)async{await t.pumpWidget(const SizedBox());await t.pump();}
void main(){
 testWidgets('Metrics row exposes explicit collection only; opening and closing creates no collection session',(t)async{
  final source=Source(),calls=f.Calls(f.NativeHandle()),original=ValueNotifier(true);
  await t.pumpWidget(host(source,calls,original));expect(source.statuses,0);expect(calls.requests,isEmpty);
  await tap(t,'agent-association-status');final id=f.metricRecord().id;
  expect(find.byKey(ValueKey('agent-tunnel-row-$id')),findsNothing);expect(find.byKey(ValueKey('agent-rotate-row-$id')),findsNothing);
  await tap(t,'agent-metrics-row-$id');expect(calls.requests,isEmpty);expect(find.byKey(const ValueKey('agent-metrics-prepare')),findsOneWidget);
  await tap(t,'agent-metrics-close');expect(calls.requests,isEmpty);await finish(t);
 });
 testWidgets('explicit Metrics enrollment selects distinct generated factory and shows actual catalog before approvals',(t)async{
  final api=enrolled.Api();RustLib.initMock(api:api);addTearDown(RustLib.dispose);
  final source=Source(),calls=f.Calls(f.NativeHandle()),original=ValueNotifier(true);
  await t.pumpWidget(host(source,calls,original));await tap(t,'agent-metrics-enrollment');
  expect(source.metricsRegistrations,0);expect(source.tunnelRegistrations,0);expect(api.calls,isEmpty);
  expect(find.textContaining('separate registration issues only'),findsOneWidget);
  await tap(t,'agent-association-status');final picker=t.widget<GlassSelect<String?>>(find.byKey(const ValueKey('agent-association-host')).last);
  picker.onChanged!(a.hostId);await t.pump();await tap(t,'agent-association-probe');
  expect(source.metricsRegistrations,1);expect(source.tunnelRegistrations,0);expect(api.calls,['metricsRegister']);
  expect(api.original.calls,['probe']);expect(find.text('metrics.collect'),findsWidgets);
  expect(find.text('app / application / http://127.0.0.1:9100/metrics'),findsOneWidget);
  expect(find.text('http://127.0.0.1:9090/api/v1/write'),findsOneWidget);
  await tap(t,'agent-association-approve');expect(api.original.calls,['probe','offer']);
  expect(source.tunnelRegistrations,0);await finish(t);expect(api.original.calls,contains('cancel'));
 });
 testWidgets('Metrics revocation uses its fixed purpose path and never Tunnel revoke',(t)async{
  final source=Source(),calls=f.Calls(f.NativeHandle()),original=ValueNotifier(true);
  await t.pumpWidget(host(source,calls,original));await tap(t,'agent-association-status');await tap(t,'agent-revoke-row-${f.metricRecord().id}');
  expect(source.metricsRevokes,0);await tap(t,'agent-revoke-prepare');
  expect(source.metricsRevokes,1);expect(source.tunnelRevokes,0);expect(calls.requests,isEmpty);await finish(t);
 });
 testWidgets('parent scope retirement during pending collection joins exact late handle without late metadata or Stop',(t)async{
  final h=f.NativeHandle(),calls=f.Calls(f.NativeHandle()),source=Source(),original=ValueNotifier(true);calls.current=h;calls.pending=Completer();
  await t.pumpWidget(host(source,calls,original));await tap(t,'agent-association-status');await tap(t,'agent-metrics-row-${f.metricRecord().id}');
  await tap(t,'agent-metrics-prepare');expect(calls.requests.length,1);original.value=false;await t.pump();await t.pump();
  calls.pending!.complete(h);await t.pump();await t.pump();expect(h.calls,['cancel','joined','dispose']);
  expect(find.byKey(const ValueKey('agent-metrics-approve')),findsNothing);expect(calls.requests.length,1);await finish(t);
 });
}
