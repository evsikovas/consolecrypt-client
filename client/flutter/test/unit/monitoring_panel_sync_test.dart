import 'dart:async';

import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_saved_panel_models.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';
import 'package:flutter_test/flutter_test.dart';

import '../helpers/monitoring_saved_panel_fakes.dart';
import '../helpers/monitoring_sync_fixture.dart';

void main() {
  test('MON07 exact local snapshot preserves local catalog write availability and no Ready field exists', () async {
    final raw=syncSnapshot(storage:'local');
    final value=MonitoringPanelSyncSnapshot.fromJson(raw);
    expect(value.storage,PanelSyncStorage.local);expect(value.readiness,PanelSyncReadiness.localOnly);
    expect(value.matches(syncProfile,syncNonce),true);
    expect(value.matches(syncProfile,syncNextNonce),false);
    for (final invalid in [syncSnapshot()..['ready']=true,syncSnapshot()..['version']=2,
      syncSnapshot()..['readiness']={'state':'ready','checkedActiveDevices':0,'blockedDevices':<Json>[]},
      syncSnapshot(server:'available',account:'enabled')]) {
      expect(()=>MonitoringPanelSyncSnapshot.fromJson(invalid),throwsA(anything));
    }
  });
  test('MON07 constructor and catalog load do not check network; explicit callback carries original frame only', () async {
    final requests=<Json>[];
    final store=AppCoreMonitoringSourceStore((method,args) async => savedPanelCatalog()..['panelSync']=syncSnapshot(),
      syncCheck:(request) async {requests.add(request);return syncSnapshot(server:'unsupported');});
    expect(requests,isEmpty);await store.load();expect(requests,isEmpty);
    expect(store.readOnly,false);expect(store.panelSync!.readiness,PanelSyncReadiness.floorUndeclared);
    final result=await store.checkPanelSync(original:syncScope(),isCurrent:()=>true);
    expect(result.server,PanelSyncServer.unsupported);
    expect(requests,[{'version':1,'profileId':syncProfile,'notificationScopeId':syncNonce}]);
    expect(store.readOnly,false,reason:'Diagnostic result is not source catalog authority');
  });
  test('MON07 missing optional callback is unavailable without another RPC or guessed route', () async {
    var calls=0;
    final store=AppCoreMonitoringSourceStore((method,args) async {calls++;return savedPanelCatalog();});
    await store.load();expect(store.canCheckPanelSync,false);
    await expectLater(store.checkPanelSync(original:syncScope(),isCurrent:()=>true),throwsA(isA<MonitoringFailure>()));
    expect(calls,1);expect(store.panelSync,isNull);
  });
  test('MON07 retired original UI epoch cannot accept correct or replacement Native nonce after await', () async {
    final pending=Completer<Json>();var current=true;
    final store=AppCoreMonitoringSourceStore((method,args) async => savedPanelCatalog()..['panelSync']=syncSnapshot(),
      syncCheck:(request)=>pending.future);
    await store.load();final original=store.panelSync;
    final future=store.checkPanelSync(original:syncScope(),isCurrent:()=>current);
    final denial=expectLater(future,throwsA(isA<MonitoringFailure>()));
    current=false;pending.complete(syncSnapshot(nonce:syncNextNonce,server:'available',account:'enabled',minimum:'2.0.0'));
    await denial;expect(store.panelSync,same(original));
  });
  test('MON07 malformed diagnostic is optional and cannot invalidate old catalog or promote enabled account', () async {
    final catalog=savedPanelCatalog()..['panelSync']={'version':999,'ready':true,'PRIVATE':'not displayed'};
    final store=AppCoreMonitoringSourceStore((method,args) async => catalog,
      syncCheck:(request) async => syncSnapshot(server:'available',account:'enabled',minimum:'2.0.0'));
    expect(await store.load(),isNotEmpty);expect(store.readOnly,false);expect(store.panelSync,isNull);
    final value=await store.checkPanelSync(original:syncScope(),isCurrent:()=>true);
    expect(value.account,PanelSyncAccount.enabled);expect(value.accountFloor,'2.0.0');
    expect(value.readiness,PanelSyncReadiness.floorUndeclared);
  });
  test('MON07 newer catalog observation retires an unfinished explicit check without installing its stale result', () async {
    final pending=Completer<Json>();var nonce=syncNonce;
    final store=AppCoreMonitoringSourceStore((method,args) async => savedPanelCatalog()..['panelSync']=syncSnapshot(nonce:nonce),
      syncCheck:(request)=>pending.future);
    await store.load();final future=store.checkPanelSync(original:syncScope(),isCurrent:()=>true);
    final denial=expectLater(future,throwsA(isA<MonitoringFailure>()));
    nonce=syncNextNonce;await store.load();
    pending.complete(syncSnapshot(server:'unsupported'));await denial;
    expect(store.panelSync!.notificationScopeId,syncNextNonce);expect(store.panelSync!.server,PanelSyncServer.notChecked);
  });
}
