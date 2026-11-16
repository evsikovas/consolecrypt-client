import 'dart:async';
import 'dart:convert';
import 'package:consolecrypt/core/bridge/rust_agent_metrics_collector.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/src/rust/api/agent_collector.dart' as native;
import 'package:flutter_test/flutter_test.dart';
import 'agent_association_fixture.dart' as a;
export 'agent_association_fixture.dart' show scope, selectedHost;

final now=DateTime.utc(2030);
AgentAssociationRecord metricRecord()=>AgentAssociationRecord(associationId:a.associationId,hostId:a.hostId,agentId:a.nodeId,
  phase:AgentAssociationPhase.associated,hostRevision:a.selectedHost.revision,nodeKeyFingerprint:'a'*64,certificateDigest:'b'*64,
  nodeKeyEpoch:'1',rotationState:'notRequested',purpose:AgentAssociationPurpose.metrics);
Map<String,dynamic> manifest()=>{'schema_version':1,'mode':'remote_write',
  'targets':[{'id':'app','url':'http://127.0.0.1:9100/metrics','kind':'application','sample_limit':1000,'series_limit':1000,'response_bytes':4096,'label_limit':32}],
  'labels':{'cc_host_id':a.hostId,'site':'local','environment':'test'},'scrape_interval_seconds':1,
  'receiver':{'url':'http://127.0.0.1:9090/api/v1/write'},'buffer':{'max_bytes':33554432,'max_age_seconds':300}};
Map<String,dynamic> view({String phase='collectorReview',int remaining=60000}){
  final admitted=const {'running','stopped'}.contains(phase);
  return {'version':1,'profileId':a.scope.profileId,'notificationScopeId':a.scope.nonce,
    'phase':phase,'associationId':a.associationId,'hostId':a.hostId,'hostRevision':a.selectedHost.revision,
    'owner':{'kind':'localProfile','id':a.scope.profileId,'deviceId':'80000000-0000-4000-8000-000000000008','signingKeySha256':'e'*64},
    'node':{'agentId':a.nodeId,'keyEpoch':'1','publicKey':'d'*64,'keyFingerprint':'a'*64},
    'scope':'metrics.collect','collectorId':'70000000-0000-8000-8000-000000000007','manifestSha256':'f'*64,'manifest':manifest(),
    'reviewRemainingMs':admitted?null:remaining,'originalExpiresUtc':'2030-01-01T00:05:00Z','profile':'volatile-collection-session-v1',
    'maxMemoryBytes':536870912,'maxTemporaryStateBytes':33554432,'lossUnknown':true,'unacknowledgedDataCanBeLost':true,
    'crashReplay':false,'automaticRenewal':false,'strictRemoteWriteAvailable':false,'deliveryGuaranteed':false,
    'receiverPolicy':'operator-controlled-nonredirecting-loopback','arbitraryReceiversSupported':false,
    'actorAdmission':admitted,'cleanupState':'pending',if(admitted)'leaseHash':'c'*64};
}
String cleanup({String state='closedObserved',String phase='unknown',bool admitted=false})=>jsonEncode({
  'version':1,'phase':phase,'actorAdmission':admitted,'lossUnknown':true,'deliveryGuaranteed':false,'cleanupState':state});

/// Actual generated opaque interface; synthetic public metadata, no Native/TLS ACK proof.
final class NativeHandle extends Fake implements native.AgentCollectorHandle {
  final calls=<String>[];
  String detailsReply=jsonEncode(view()),approveReply=jsonEncode(view(phase:'running')),stopReply=jsonEncode(view(phase:'running')),joinedReply=cleanup();
  Completer<String>? pendingDetails,pendingApprove,pendingJoin;
  @override Future<String> details(){calls.add('details');return pendingDetails?.future??Future.value(detailsReply);}
  @override Future<String> approveOnce(){calls.add('approve');return pendingApprove?.future??Future.value(approveReply);}
  @override Future<String> stop(){calls.add('stop');return Future.value(stopReply);}
  @override Future<String> joined(){calls.add('joined');return pendingJoin?.future??Future.value(joinedReply);}
  @override void cancel(){calls.add('cancel');}
  @override void dispose(){calls.add('dispose');}
}
final class Calls implements AgentMetricsCollectorNativeCalls {
  Calls(this.current);
  NativeHandle current;
  final requests=<Map<String,dynamic>>[];
  Completer<native.AgentCollectorHandle>? pending;
  @override Future<native.AgentCollectorHandle> prepare(String raw){requests.add(jsonDecode(raw)as Map<String,dynamic>);return pending?.future??Future.value(current);}
}
