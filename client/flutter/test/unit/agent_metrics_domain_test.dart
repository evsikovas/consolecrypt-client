import 'dart:convert';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_association_wire.dart';
import 'package:consolecrypt/monitoring/agent_metrics_manifest.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_route_models.dart';
import 'package:flutter_test/flutter_test.dart';
import '../support/agent_association_fixture.dart' as f;

Map<String,dynamic> manifest()=>{'schema_version':1,'mode':'remote_write','targets':[{'id':'app','url':'http://127.0.0.1:9100/metrics','kind':'application','sample_limit':1000,'series_limit':1000,'response_bytes':4096,'label_limit':32}],
  'labels':{'cc_host_id':f.hostId,'site':'local','environment':'test'},'scrape_interval_seconds':1,'receiver':<String,dynamic>{'url':'http://127.0.0.1:9090/api/v1/write'},'buffer':{'max_bytes':33554432,'max_age_seconds':300}};
Map<String,dynamic> catalog()=>{'collectorId':'70000000-0000-8000-8000-000000000007','manifestSha256':'b'*64,'manifest':manifest()};
Map<String,dynamic> operation(String purpose,{String phase='offerReview'})=>f.operationWire(phase:phase)..addAll({'purpose':purpose,'scope':purpose=='metrics'?'metrics.collect':'tunnel.open','scopes':[purpose=='metrics'?'metrics.collect':'tunnel.open'],
  'scopeSource':purpose=='metrics'?'actualAdvertisedIntersection':'explicitFactoryRequest','advertisedScopes':purpose=='metrics'?['metrics.collect']:null,'collector':purpose=='metrics'?catalog():null});
Map<String,dynamic> record({String? purpose})=>{'associationId':f.associationId,'hostId':f.hostId,'hostRevision':f.selectedHost.revision,'agentId':f.nodeId,'nodeKeyFingerprint':'a'*64,'certificateDigest':'d'*64,'certificateExpiresUtc':'2030-01-01T00:00:00Z','state':'associated',
  'revocationState':'notRequested','remoteRevocationObserved':false,'keyEpoch':'1','rotationState':'notRequested',if(purpose!=null)...{'purpose':purpose,'scope':purpose=='metrics'?'metrics.collect':'tunnel.open','scopes':[purpose=='metrics'?'metrics.collect':'tunnel.open']}};
AgentAssociationRecord parsedRecord(Map<String,dynamic> row){final value=f.statusWire()..['associations']=[row];return parseAgentAssociationStatus(jsonEncode(value),f.scope).records.single;}

void main(){
  test('legacy closed Tunnel operation remains accepted',(){final value=parseAgentAssociationReview(jsonEncode(f.operationWire()),f.scope,f.selectedHost,AgentAssociationReviewStage.offer);expect(value.purpose,AgentAssociationPurpose.tunnel);expect(value.metricsCollector,isNull);});
  test('current closed Tunnel operation carries only explicit Tunnel scope',(){final value=parseAgentAssociationReview(jsonEncode(operation('tunnel')),f.scope,f.selectedHost,AgentAssociationReviewStage.offer);expect(value.scopeName,'tunnel.open');expect(value.metricsCollector,isNull);});
  test('Metrics operation requires explicitly selected Metrics factory and actual catalog',(){final raw=jsonEncode(operation('metrics'));
    expect(()=>parseAgentAssociationReview(raw,f.scope,f.selectedHost,AgentAssociationReviewStage.offer),throwsFormatException);
    final value=parseAgentAssociationReview(raw,f.scope,f.selectedHost,AgentAssociationReviewStage.offer,expectedPurpose:AgentAssociationPurpose.metrics);
    expect(value.purpose,AgentAssociationPurpose.metrics);expect(value.metricsCollector!.manifest.targets.single.id,'app');expect(value.metricsCollector!.manifest.mode,'remote_write');});
  test('Metrics claim and certificate result retain purpose and no Actor admission',(){
    final claim=parseAgentAssociationReview(jsonEncode(operation('metrics',phase:'claimReview')),f.scope,f.selectedHost,AgentAssociationReviewStage.certificate,expectedPurpose:AgentAssociationPurpose.metrics);expect(claim.metricsCollector,isNotNull);
    final result=parseAgentAssociationResult(jsonEncode(operation('metrics',phase:'associated')),f.scope,f.selectedHost,expectedPurpose:AgentAssociationPurpose.metrics);expect(result.purpose,AgentAssociationPurpose.metrics);expect(result.metricsCollector!.manifest.hostId,f.hostId);
  });
  test('current stored records distinguish Metrics from Tunnel without classifying legacy',(){
    final metric=parsedRecord(record(purpose:'metrics')),tunnel=parsedRecord(record(purpose:'tunnel')),legacy=parsedRecord(record());
    expect(metric.canRequestMetricsCollection,isTrue);expect(metric.canRequestTunnel,isFalse);expect(tunnel.canRequestTunnel,isTrue);expect(tunnel.canRequestMetricsCollection,isFalse);expect(legacy.purpose,isNull);expect(legacy.canRequestMetricsCollection,isFalse);
  });
  test('mixed, missing and conflicting current scope metadata refuses',(){
    final mixed=record(purpose:'metrics')..['scopes']=['metrics.collect','tunnel.open'];expect(()=>parsedRecord(mixed),throwsFormatException);
    final missing=record(purpose:'metrics')..remove('scope');expect(()=>parsedRecord(missing),throwsFormatException);
    final conflicting=operation('metrics')..['scope']='tunnel.open';expect(()=>parseAgentAssociationReview(jsonEncode(conflicting),f.scope,f.selectedHost,AgentAssociationReviewStage.offer,expectedPurpose:AgentAssociationPurpose.metrics),throwsFormatException);
  });
  test('claimed or absent advertised intersection cannot become Metrics review',(){
    for(final field in['scopeSource','advertisedScopes','collector']){final value=operation('metrics');value[field]=field=='scopeSource'?'explicitFactoryRequest':null;expect(()=>parseAgentAssociationReview(jsonEncode(value),f.scope,f.selectedHost,AgentAssociationReviewStage.offer,expectedPurpose:AgentAssociationPurpose.metrics),throwsFormatException);}
  });
  test('Metrics rotation is unsupported and known Metrics cannot enter TUN prepare',(){
    final rotated=record(purpose:'metrics')..addAll({'keyEpoch':'2','rotationState':'selectedObserved'});expect(()=>parsedRecord(rotated),throwsFormatException);
    final metric=parsedRecord(record(purpose:'metrics'));expect(()=>AgentTunnelRouteIntent(record:metric,listenIp:'127.0.0.1',listenPort:0,destinationIp:'127.0.0.1',destinationPort:9090,ttlSeconds:60,idleSeconds:30),throwsFormatException);
  });
  test('manifest display rejects host substitution, receiver redirects and extra command keys',(){
    final host=manifest();(host['labels']as Map)['cc_host_id']=f.nodeId;expect(()=>parseAgentMetricsManifest(host,f.hostId),throwsFormatException);
    final redirect=manifest();(redirect['receiver']as Map)['follow_redirects']=true;expect(()=>parseAgentMetricsManifest(redirect,f.hostId),throwsFormatException);
    final command=manifest()..['command']='run';expect(()=>parseAgentMetricsManifest(command,f.hostId),throwsFormatException);
  });
  test('manifest permits real loopback sockets only and rejects control suffix',(){
    final loop=manifest();((loop['targets']as List).single as Map)['url']='https://[::1]:9100/metrics';expect(parseAgentMetricsManifest(loop,f.hostId).targets.single.url,contains('[::1]'));
    for(final endpoint in['http://localhost:9100/metrics','http://0.0.0.0:9100/metrics','http://user@127.0.0.1:9100/metrics','http://127.0.0.1:9100/metrics\n']){final value=manifest();((value['targets']as List).single as Map)['url']=endpoint;expect(()=>parseAgentMetricsManifest(value,f.hostId),throwsFormatException);}
  });
}
