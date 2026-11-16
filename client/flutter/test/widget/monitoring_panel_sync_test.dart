import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/app/app.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/core/services/runtime_notice_adapter.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_providers.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/monitoring_saved_panel_fakes.dart';
import '../helpers/monitoring_sync_fixture.dart';
import '../helpers/test_app.dart';

class _Backend implements MonitoringBackend {
  _Backend(this.store,this.service);
  final AppCoreMonitoringSourceStore store;
  final SavedPanelTestService service;
  @override MonitoringService createService()=>service;
  @override MonitoringSourceStore createSourceStore()=>store;
}
class _Fixture {
  late String profile;
  String nonce=syncNonce;
  String storage='synced';
  String remoteServer='unsupported';
  String? remoteAccount,remoteMinimum;
  int checks=0,saves=0;
  bool capability=true;
  Json? acceptedDefinition;
  final requests=<Json>[];
  Completer<Json>? pending;
  final service=SavedPanelTestService();
  final events=RuntimeNoticeEventAdapter();
  late AppCoreMonitoringSourceStore store;
  Json snapshot({bool remote=false})=>syncSnapshot(profile:profile,nonce:nonce,storage:storage,
    server:remote ? remoteServer : 'notChecked',account:remote ? remoteAccount : null,minimum:remote ? remoteMinimum : null);
  Json catalog() => savedPanelCatalog(panels:acceptedDefinition == null ? [] : [{
    'id':savedPanelId,'revision':1,'definition':acceptedDefinition,'state':'current',
  }])..['panelSync']=snapshot();
  void establish() => events.acceptScope(originalEpoch:events.epoch,nativeProfile:{
    'id':profile,'active':true,'vault_state':'unlocked','notification_scope_id':nonce,
  });
  Future<ProviderContainer> open(WidgetTester tester,{bool establishScope=true,AppLocale locale=AppLocale.en}) async {
    final backend=testBackend();addTearDown(backend.dispose);addTearDown(events.dispose);
    await backend.debugCreateUnlockedLocalProfile(name:'Owned compatibility profile');
    profile=backend.services.profiles.currentProfiles.active!.id.value;
    if (establishScope) establish();
    store=AppCoreMonitoringSourceStore((method,args) async {
      if (method=='monitoring_panel_create') {saves++;acceptedDefinition=object((jsonDecode(args['requestJson'] as String) as Map)['definition']);}
      return catalog();
    },syncCheck:capability ? (request) async {checks++;requests.add(request);return pending?.future ?? snapshot(remote:true);} : null);
    final s=backend.services;
    final app=AppServices(profiles:s.profiles,auth:s.auth,vault:s.vault,inventory:s.inventory,terminal:s.terminal,
      sftp:s.sftp,tunnels:s.tunnels,snippets:s.snippets,ai:s.ai,devices:s.devices,sync:s.sync,settings:s.settings,
      backups:s.backups,files:s.files,runtimeNotices:events,monitoring:_Backend(store,service));
    setTestLocale(backend,locale);tester.view.physicalSize=const Size(580,1000);tester.view.devicePixelRatio=1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(ProviderScope(overrides:[appServicesProvider.overrideWithValue(app)],child:const ConsoleCryptApp()));
    await settle(tester);
    final container=ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    container.read(routerProvider).go(AppRoutes.monitoring);await settle(tester);return container;
  }
}
Future<void> reveal(WidgetTester tester,String key) async {
  final scroll=find.descendant(of:find.byKey(const ValueKey('monitoring-workspace-scroll')),matching:find.byType(Scrollable)).first;
  await tester.scrollUntilVisible(find.byKey(ValueKey(key)),300,scrollable:scroll);await settle(tester);
}
void main() {
  testWidgets('MON07 old server is observed only after explicit check and never becomes Ready or permits synced Save', (tester) async {
    final f=_Fixture();await f.open(tester);expect(f.checks,0);expect(f.service.queries,0);
    await reveal(tester,'panel-sync-check');await tapKey(tester,'panel-sync-check');
    expect(f.checks,1);expect(find.text('This server does not support the panel compatibility check.'),findsOneWidget);
    expect(find.text('A released minimum client version for preserving panels has not been declared.'),findsOneWidget);
    await tapKey(tester,'saved-panel-new');await enterKey(tester,'saved-panel-title','Owned draft');
    await enterKey(tester,'saved-panel-query','up');
    expect(tester.widget<FilledButton>(find.byKey(const ValueKey('saved-panel-save'))).onPressed,isNull);
    expect(f.saves,0);expect(f.service.queries,0);expect(tester.takeException(),isNull);
  });
  testWidgets('MON07 local snapshot keeps actual local Save available and performs no compatibility request', (tester) async {
    final f=_Fixture()..storage='local';await f.open(tester);
    expect(find.byKey(const ValueKey('panel-sync-local')),findsOneWidget);expect(f.checks,0);
    await tapKey(tester,'saved-panel-new');await enterKey(tester,'saved-panel-title','Owned local draft');
    await enterKey(tester,'saved-panel-query','up');
    expect(tester.widget<FilledButton>(find.byKey(const ValueKey('saved-panel-save'))).onPressed,isNotNull);
    await tapKey(tester,'saved-panel-save');expect(f.saves,1);expect(f.checks,0);expect(f.service.queries,0);
    expect(tester.takeException(),isNull);
  });
  testWidgets('MON07 original Native nonce retirement discards pending check before replacement profile scope is displayed', (tester) async {
    final f=_Fixture()..pending=Completer<Json>();final container=await f.open(tester);
    await reveal(tester,'panel-sync-check');await tapKey(tester,'panel-sync-check');expect(f.checks,1);
    final original=container.read(monitoringScopeProvider);final old=f.snapshot(remote:true);
    f.events.boundary();expect(original.current,false);
    f.nonce=syncNextNonce;f.establish();f.pending!.complete(old);await settle(tester);
    expect(find.text('This server does not support the panel compatibility check.'),findsNothing);
    expect(f.store.panelSync?.notificationScopeId,syncNextNonce);expect(f.checks,1);expect(f.service.queries,0);
    expect(tester.takeException(),isNull);
  });
  testWidgets('MON07 absent optional capability remains unavailable without fallback', (tester) async {
    final f=_Fixture()..capability=false;await f.open(tester);
    await reveal(tester,'panel-sync-check');
    expect(tester.widget<TextButton>(find.byKey(const ValueKey('panel-sync-check'))).onPressed,isNull);
    expect(f.checks,0);expect(f.saves,0);expect(tester.takeException(),isNull);
  });
  testWidgets('MON07 enabled account policy remains pending with synced Save disabled in narrow Russian UI', (tester) async {
    final f=_Fixture()..remoteServer='available'..remoteAccount='enabled'..remoteMinimum='2.0.0';
    await f.open(tester,locale:AppLocale.ru);await reveal(tester,'panel-sync-check');await tapKey(tester,'panel-sync-check');
    expect(find.textContaining('Разрешение синхронизации ещё не подтверждено'),findsOneWidget);
    expect(find.textContaining('Минимальная версия приложения для аккаунта: 2.0.0'),findsOneWidget);
    await tapKey(tester,'saved-panel-new');
    expect(tester.widget<FilledButton>(find.byKey(const ValueKey('saved-panel-save'))).onPressed,isNull);
    expect(f.checks,1);expect(f.saves,0);expect(f.service.queries,0);expect(tester.takeException(),isNull);
  });
  testWidgets('MON07 authoritative profile B nonce cannot be used while UI still publishes profile A', (tester) async {
    final f=_Fixture();final container=await f.open(tester);
    final original=container.read(monitoringScopeProvider);
    f.events.acceptScope(originalEpoch:f.events.epoch,nativeProfile:{
      'id':'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa','active':true,'vault_state':'unlocked','notification_scope_id':syncNextNonce,
    });
    expect(original.current,false);
    await settle(tester);
    final replacement=container.read(monitoringScopeProvider);
    expect(replacement.current,false);expect(replacement.nativeScope,isNull);expect(f.checks,0);
    expect(tester.takeException(),isNull);
  });

}
