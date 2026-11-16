import 'dart:convert';
import 'package:consolecrypt/core/bridge/rust_agent_association.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_association_port.dart';
import 'package:consolecrypt/src/rust/api/agent_node_association.dart' as native;
import 'package:consolecrypt/src/rust/frb_generated.dart';
import 'package:flutter_test/flutter_test.dart';
import '../support/agent_association_fixture.dart' as a;
import '../support/agent_metrics_fixture.dart' as f;

String review(String phase)=>jsonEncode(a.operationWire(phase:phase)..addAll({'purpose':'metrics','scope':'metrics.collect','scopes':['metrics.collect'],
 'scopeSource':'actualAdvertisedIntersection','advertisedScopes':['metrics.collect'],
 'collector':{'collectorId':'70000000-0000-8000-8000-000000000007','manifestSha256':'f'*64,'manifest':f.manifest()}}));
final class Operation extends Fake implements native.AgentNodeAssociationHandle {
 final calls=<String>[];
 @override Future<String> probe()async{calls.add('probe');return review('offerReview');}
 @override Future<String> approveOffer()async{calls.add('offer');return review('claimReview');}
 @override Future<String> approveCertificate({required int lifetimeSeconds})async{calls.add('certificate:$lifetimeSeconds');return review('associated');}
 @override void cancel(){calls.add('cancel');}
 @override Future<String> joined()async{calls.add('joined');return jsonEncode({'version':1,'phase':'unknown','cleanupState':'closedObserved'});}
}
final class Api implements RustLibApi {
 final original=Operation(),calls=<String>[],requests=<Map<String,dynamic>>[];
 @override Future<native.AgentNodeAssociationHandle> crateApiAgentNodeAssociationAgentMetricsNodeAssociationRegister({required String requestJson})async{calls.add('metricsRegister');requests.add(jsonDecode(requestJson)as Map<String,dynamic>);return original;}
 @override dynamic noSuchMethod(Invocation invocation)=>throw StateError('Unexpected endpoint ${invocation.memberName}');
}
void main(){
 late Api api;
 setUp((){api=Api();RustLib.initMock(api:api);});tearDown(RustLib.dispose);
 test('separate Metrics generated register reuses original two approvals without caller authority',()async{
  const actual=RustAgentAssociationPort();const port=AgentMetricsEnrollmentAssociationPort(actual,actual);
  expect(api.calls,isEmpty);final held=await port.registerProbe(a.scope,a.selectedHost);
  expect(api.calls,['metricsRegister']);expect(api.requests.single,{'version':1,'profileId':a.scope.profileId,'notificationScopeId':a.scope.nonce,'hostId':a.hostId,'expectedHostRevision':a.selectedHost.revision});
  final offer=await held.probe(),claim=await held.approveOffer(),done=await held.approveCertificate(lifetimeSeconds:60);
  expect(offer.purpose,AgentAssociationPurpose.metrics);expect(claim.metricsCollector!.manifest.mode,'remote_write');expect(done.purpose,AgentAssociationPurpose.metrics);
  held.cancel();await held.joined();expect(api.original.calls,['probe','offer','certificate:60','cancel','joined']);
 });
 test('known Metrics cannot enter Tunnel revoke or unsupported rotation generated endpoint',()async{
  const actual=RustAgentAssociationPort();final record=f.metricRecord();
  await expectLater(actual.prepareRevocation(a.scope,record),throwsFormatException);
  await expectLater(actual.prepareRotation(a.scope,record),throwsFormatException);
  await expectLater(actual.prepareRotationReconcile(a.scope,record),throwsFormatException);
  expect(api.calls,isEmpty);
 });
}
