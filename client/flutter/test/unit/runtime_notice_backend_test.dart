import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/app/critical_notice_owner.dart';
import 'package:consolecrypt/core/bridge/rust_backend.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/src/rust/api/app.dart';
import 'package:consolecrypt/src/rust/frb_generated.dart';
import 'package:flutter_test/flutter_test.dart';

const profile = '11111111-1111-4111-8111-111111111111';
const other = '22222222-2222-4222-8222-222222222222';
const scopeA = '44444444-4444-4444-8444-444444444444';
const scopeB = '55555555-5555-4555-8555-555555555555';
const job = '33333333-3333-4333-8333-333333333333';
String profiles(String id, String state, [String? nonce = scopeA]) => jsonEncode([
  {
    'id': id,
    'active': true,
    'display_name': 'Owned Native fixture',
    'kind': 'local',
    'created_at_ms': 1,
    'vault_state': state,
    'notification_scope_id': ?nonce,
  },
]);
Future<void> flush() async {
  for (var i = 0; i < 150; i++) {
    await Future<void>.value();
  }
}

class _Native implements RustLibApi {
  // Both streams are closed in tearDown, after backend subscriptions are cancelled.
  // ignore: close_sinks
  final events = StreamController<String>.broadcast();
  // Both controllers are closed in tearDown after cancelling hub subscriptions.
  // ignore: close_sinks
  final prompts = StreamController<String>.broadcast();
  int eventSubscriptions = 0;
  String current = profiles(profile, 'unlocked');
  Completer<String>? pendingProfiles;
  @override
  Stream<String> crateApiAppCoreEvents() {
    eventSubscriptions++;
    return events.stream;
  }

  @override
  Stream<String> crateApiAppCorePrompts() => prompts.stream;
  @override
  Future<String> crateApiProfilesProfilesList() {
    final pending = pendingProfiles;
    pendingProfiles = null;
    return pending?.future ?? Future.value(current);
  }

  @override
  Future<String?> crateApiAppUiStoreGet({required String key}) async => null;
  @override
  void crateApiAppOsAuthReport({OsAuthKindChoice? kind, required bool notEnrolled}) {}
  @override
  dynamic noSuchMethod(Invocation invocation) {
    if (invocation.memberName == #crateApiInventoryVaultSettingsGet) {
      return Future<String>.value(jsonEncode({'id': job, 'vault_name': 'Owned fixture vault'}));
    }
    if (const {
      #crateApiProfilesVaultDeviceUnlockInfo,
      #crateApiProfilesVaultSecurityKeyInfo,
    }.contains(invocation.memberName)) {
      return Future<String>.value('{}');
    }
    if (const {
      #crateApiInventoryHostsList,
      #crateApiInventoryGroupsList,
      #crateApiInventoryJumpProfilesList,
      #crateApiCredentialsCredentialsList,
      #crateApiInventoryKnownHostsList,
      #crateApiInventorySnippetsList,
      #crateApiInventoryTunnelsList,
      #crateApiInventoryAiProvidersList,
      #crateApiSshTunnelStatuses,
    }.contains(invocation.memberName)) {
      return Future<String>.value('[]');
    }
    throw StateError('Unexpected Native call: ${invocation.memberName}');
  }
}

String runtimeEvent({String profileId = profile, String nonce = scopeA, int revision = 1, String state = 'failed'}) =>
    jsonEncode({
      'type': 'critical_state_changed',
      'profile_id': profileId,
      'scope_id': nonce,
      'record_id': job,
      'revision': revision,
      'change': {'domain': 'terminal', 'state': state},
    });
void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  late _Native api;
  late RustBackend backend;
  late StreamSubscription<RuntimeNoticeFrame> subscription;
  final frames = <RuntimeNoticeFrame>[];
  setUp(() async {
    api = _Native();
    RustLib.initMock(api: api);
    frames.clear();
    backend = RustBackend(
      const CoreInfo(
        dataDir: '/unused-owned-fixture',
        platform: 'windows',
        coreVersion: 'fixture',
        alreadyInitialized: false,
      ),
    );
    subscription = backend.runtimeNoticeEvents.watchRuntimeNotices().listen(frames.add);
    await backend.start();
    await flush();
    frames.clear();
  });
  tearDown(() async {
    await subscription.cancel();
    await backend.dispose();
    await api.events.close();
    await api.prompts.close();
    RustLib.dispose();
  });
  test('same actual Native stream fans out typed runtime metadata without extra subscription or mutation', () async {
    api.events.add(runtimeEvent());
    await flush();
    expect(api.eventSubscriptions, 1);
    expect(frames.single.change!.state, RuntimeNoticeState.failed);
    expect(frames.single.scope!.nonce, scopeA);
    expect(frames.single.change!.recordId, job);
  });
  test('authoritative Native nonce refresh advances owner before delayed legacy boundary', () async {
    final oldEpoch = backend.runtimeNoticeEvents.epoch;
    api.current = profiles(profile, 'unlocked', scopeB);
    await backend.refreshState();
    await flush();
    expect(backend.runtimeNoticeEvents.epoch, greaterThan(oldEpoch));
    expect(frames.where((e) => e.reset).last.scope!.nonce, scopeB);
    frames.clear();
    api.events.add(runtimeEvent());
    api.events.add(runtimeEvent(nonce: scopeB, revision: 2));
    await flush();
    expect(frames.where((e) => e.change != null).single.change!.revision, 2);
  });
  test('lock fences original pending profiles read and old-owner events', () async {
    final pending = api.pendingProfiles = Completer<String>();
    final refresh = backend.refreshState();
    api.current = profiles(profile, 'locked');
    api.events.add(jsonEncode({'type': 'vault_locked', 'profile_id': profile}));
    api.events.add(runtimeEvent());
    await flush();
    expect(frames.where((e) => e.change != null), isEmpty);
    pending.complete(profiles(profile, 'unlocked'));
    await refresh;
    await flush();
    expect(backend.runtimeNoticeEvents.currentScope.scope, isNull);
    api.events.add(runtimeEvent(revision: 2));
    await flush();
    expect(frames.where((e) => e.change != null), isEmpty);
  });
  test('foreign profile and old nonce do not become current owner metadata', () async {
    api.current = profiles(other, 'unlocked', scopeB);
    api.events.add(jsonEncode({'type': 'profiles_changed'}));
    await flush();
    frames.clear();
    api.events.add(runtimeEvent());
    api.events.add(runtimeEvent(profileId: other));
    api.events.add(runtimeEvent(profileId: other, nonce: scopeB));
    await flush();
    expect(frames.where((e) => e.change != null).single.change!.profileId, other);
    expect(api.eventSubscriptions, 1);
  });
  test('older backend without ephemeral scope is explicitly unsupported', () async {
    api.current = profiles(profile, 'unlocked', null);
    await backend.refreshState();
    await flush();
    expect(backend.runtimeNoticeEvents.currentScope.unsupported, isTrue);
    frames.clear();
    api.events.add(runtimeEvent());
    await flush();
    expect(frames.where((e) => e.change != null), isEmpty);
  });
  test('matching malformed metadata reports a gap without retaining raw errors', () async {
    final malformed = jsonDecode(runtimeEvent()) as Map<String, dynamic>;
    malformed['raw_error'] = 'synthetic sensitive text';
    api.events.add(jsonEncode(malformed));
    await flush();
    expect(frames.single.incomplete, isTrue);
    expect(frames.single.change, isNull);
    await backend.refreshState();
    await flush();
    expect(backend.runtimeNoticeEvents.currentScope.incomplete, isTrue);
  });
  test('closed feed stays unavailable through fresh scope and never resubscribes', () async {
    await api.events.close();
    await flush();
    expect(backend.runtimeNoticeEvents.currentScope.streamUnavailable, isTrue);
    api.current = profiles(profile, 'unlocked', scopeB);
    await backend.refreshState();
    await flush();
    expect(backend.runtimeNoticeEvents.currentScope.streamUnavailable, isTrue);
    expect(api.eventSubscriptions, 1);
    expect(frames.where((e) => e.change != null), isEmpty);
  });
  test('paused job listener cannot leave runtime owner alive during hub disposal', () async {
    final paused = backend.jobNoticeEvents.watchJobNotices().listen((_) {});
    paused.pause();
    final pending = api.pendingProfiles = Completer<String>();
    final refresh = backend.refreshState();
    final disposing = backend.dispose();
    await flush();
    expect(backend.runtimeNoticeEvents.currentScope.scope, isNull);
    pending.complete(profiles(profile, 'unlocked', scopeB));
    await refresh;
    await flush();
    expect(backend.runtimeNoticeEvents.currentScope.scope, isNull);
    paused.resume();
    await paused.cancel();
    await disposing;
  });
  test('existing single coreEvents subscription delivers all fixed legacy domains without extra Native calls',() async {
    const changes={
      'security':['integrity_warning','server_rollback','secret_conflict','conflict_copy'],
      'device':['registered','approval_requested','revoked','recovery_changed','key_conflict'],
      'backup':['failed','completed'], 'team':['invalid_log','log_rollback','removed'],
      'edit':['error','conflict','recovery_available','synced'], 'transfer':['failed','completed'],
    };
    var revision=0;
    for(final entry in changes.entries){for(final state in entry.value){
      api.events.add(jsonEncode({'type':'critical_state_changed','profile_id':profile,'scope_id':scopeA,
        'record_id':job,'revision':++revision,'change':{'domain':entry.key,'state':state}}));
    }}
    await flush();
    expect(api.eventSubscriptions,1);
    expect(frames.where((frame)=>frame.change!=null),hasLength(20));
    expect(frames.where((frame)=>frame.change!=null).every((frame)=>frame.change!.scopeId==scopeA),isTrue);
  });
  test('legacy scoped producer cannot relabel an old unlock while profiles refresh is pending',() async {
    final pending=Completer<String>();api.pendingProfiles=pending;
    api.events.add(jsonEncode({'type':'vault_locked','profile_id':profile}));await flush();
    api.events.add(jsonEncode({'type':'critical_state_changed','profile_id':profile,'scope_id':scopeA,
      'record_id':job,'revision':1,'change':{'domain':'security','state':'integrity_warning'}}));await flush();
    expect(frames.where((frame)=>frame.change!=null),isEmpty);
    pending.complete(profiles(profile,'unlocked',scopeB));await flush();
    api.events.add(jsonEncode({'type':'critical_state_changed','profile_id':profile,'scope_id':scopeA,
      'record_id':job,'revision':99,'change':{'domain':'device','state':'registered'}}));
    api.events.add(jsonEncode({'type':'critical_state_changed','profile_id':profile,'scope_id':scopeB,
      'record_id':job,'revision':1,'change':{'domain':'team','state':'invalid_log'}}));await flush();
    final changes=frames.where((frame)=>frame.change!=null).toList();expect(changes,hasLength(1));
    expect(changes.single.change!.scopeId,scopeB);expect(changes.single.change!.domain.name,'team');
    expect(api.eventSubscriptions,1);
  });

  Map<String,dynamic> startupChange(String domain,String state,{String nonce=scopeB,String owner=profile,
      int revision=1,String record=job}) => {
    'type':'critical_state_changed','profile_id':owner,'scope_id':nonce,'record_id':record,'revision':revision,
    'change':{'domain':domain,'state':state},
  };
  CriticalNoticeOwner lateOwner() {
    final owner=CriticalNoticeOwner(scope:Object(),profileId:profile,isCurrent:()=>true,
      runtimeEvents:backend.runtimeNoticeEvents);
    addTearDown(owner.dispose);return owner;
  }
  test('unlock startup metadata reaches late common owner ONLY after original actual profiles proof',() async {
    final pending=Completer<String>();api.pendingProfiles=pending;
    api.events.add(jsonEncode({'type':'vault_unlocked','profile_id':profile}));await flush();
    api.events.add(jsonEncode(startupChange('edit','recovery_available')));
    api.events.add(jsonEncode(startupChange('security','integrity_warning',revision:2)));await flush();
    expect(frames.where((frame)=>frame.change!=null),isEmpty);
    expect(backend.runtimeNoticeEvents.currentScope.scope,isNull);
    pending.complete(profiles(profile,'unlocked',scopeB));await flush();
    final owner=lateOwner();
    expect(owner.state.rows,hasLength(2));
    expect(owner.state.rows.map((row)=>row.runtime!.value.domain.name),containsAll(['edit','security']));
    expect(backend.runtimeNoticeEvents.currentScope.scope!.nonce,scopeB);
    expect(api.eventSubscriptions,1);
  });
  test('late startup owner uses real completion instead of replaying an outdated early failure',() async {
    final pending=Completer<String>();api.pendingProfiles=pending;
    api.events.add(jsonEncode({'type':'vault_unlocked','profile_id':profile}));await flush();
    api.events.add(jsonEncode(startupChange('edit','error')));
    api.events.add(jsonEncode(startupChange('security','server_rollback',revision:2)));await flush();
    pending.complete(profiles(profile,'unlocked',scopeB));await flush();
    api.events.add(jsonEncode(startupChange('edit','synced',revision:3)));await flush();
    final owner=lateOwner();expect(owner.state.rows,hasLength(1));
    expect(owner.state.rows.single.runtime!.value.domain.name,'security');
  });
  test('ordinary profiles refresh has NO startup buffer even when eventual nonce matches',() async {
    final pending=Completer<String>();api.pendingProfiles=pending;
    api.events.add(jsonEncode({'type':'profiles_changed'}));await flush();
    api.events.add(jsonEncode(startupChange('backup','failed')));await flush();
    pending.complete(profiles(profile,'unlocked',scopeB));await flush();
    final owner=lateOwner();expect(owner.state.rows,isEmpty);
    api.events.add(jsonEncode(startupChange('backup','failed',revision:2)));await flush();
    expect(owner.state.rows,hasLength(1));expect(api.eventSubscriptions,1);
  });
  test('lock after early unlock metadata drops pending and denies the stale refresh result',() async {
    final pending=Completer<String>();api.pendingProfiles=pending;
    api.events.add(jsonEncode({'type':'vault_unlocked','profile_id':profile}));await flush();
    api.events.add(jsonEncode(startupChange('device','registered')));await flush();
    api.current=profiles(profile,'locked',null);
    api.events.add(jsonEncode({'type':'vault_locked','profile_id':profile}));await flush();
    pending.complete(profiles(profile,'unlocked',scopeB));await flush();
    expect(backend.runtimeNoticeEvents.currentScope.scope,isNull);
    final second=Completer<String>();api.pendingProfiles=second;
    api.events.add(jsonEncode({'type':'vault_unlocked','profile_id':profile}));await flush();
    second.complete(profiles(profile,'unlocked',scopeA));await flush();
    final owner=lateOwner();expect(owner.state.rows,isEmpty);
    api.events.add(jsonEncode(startupChange('device','registered',revision:99)));await flush();
    expect(owner.state.rows,isEmpty);expect(api.eventSubscriptions,1);
  });
  test('event supplied profile and nonce never establish or replace the actual Native unlock proof',() async {
    final pending=Completer<String>();api.pendingProfiles=pending;
    api.events.add(jsonEncode({'type':'vault_unlocked','profile_id':profile}));await flush();
    api.events.add(jsonEncode(startupChange('team','removed',owner:other)));
    api.events.add(jsonEncode(startupChange('security','secret_conflict',nonce:scopeA)));
    api.events.add(jsonEncode(startupChange('transfer','failed',revision:2)));await flush();
    expect(backend.runtimeNoticeEvents.currentScope.scope,isNull);
    pending.complete(profiles(profile,'unlocked',scopeB));await flush();
    final owner=lateOwner();expect(owner.state.rows,hasLength(1));
    expect(owner.state.rows.single.runtime!.value.domain.name,'transfer');
  });
  test('unlock pending metadata is bounded64 and overflow is incomplete rather than invented recovery',() async {
    final pending=Completer<String>();api.pendingProfiles=pending;
    api.events.add(jsonEncode({'type':'vault_unlocked','profile_id':profile}));await flush();
    for(var i=1;i<=80;i++){
      api.events.add(jsonEncode(startupChange('transfer','failed',revision:i,
        record:'88888888-8888-4888-8888-${i.toString().padLeft(12,'0')}')));
    }
    await flush();expect(frames.where((frame)=>frame.change!=null),isEmpty);
    pending.complete(profiles(profile,'unlocked',scopeB));await flush();
    final owner=lateOwner();expect(owner.state.rows,hasLength(64));expect(owner.state.runtimeIncomplete,isTrue);
    owner.clear();await owner.dispose();
    final replacement=lateOwner();expect(replacement.state.rows,isEmpty);
    expect(api.eventSubscriptions,1);
  });
  test('malformed private startup payload becomes only matching-origin incompleteness without retaining data',() async {
    final pending=Completer<String>();api.pendingProfiles=pending;
    api.events.add(jsonEncode({'type':'vault_unlocked','profile_id':profile}));await flush();
    api.events.add(jsonEncode({...startupChange('security','integrity_warning'),'raw':'SYNTHETIC_PRIVATE_PATH'}));
    api.events.add(jsonEncode({'type':'critical_notice_gap','profile_id':other,'scope_id':scopeB}));await flush();
    pending.complete(profiles(profile,'unlocked',scopeB));await flush();
    final owner=lateOwner();expect(owner.state.rows,isEmpty);expect(owner.state.runtimeIncomplete,isTrue);
    expect(frames.where((frame)=>frame.change!=null),isEmpty);
  });

  test('already subscribed common owner drains startup only after the matching authoritative reset',() async {
    final owner=CriticalNoticeOwner(scope:Object(),profileId:profile,isCurrent:()=>true,
      events:backend.jobNoticeEvents,runtimeEvents:backend.runtimeNoticeEvents);addTearDown(owner.dispose);
    final pending=Completer<String>();api.pendingProfiles=pending;
    api.events.add(jsonEncode({'type':'vault_unlocked','profile_id':profile}));await flush();
    api.events.add(jsonEncode(startupChange('security','secret_conflict')));await flush();
    expect(owner.state.rows,isEmpty);
    api.current=profiles(profile,'unlocked',scopeB);pending.complete(api.current);await flush();
    expect(owner.state.rows,hasLength(1));expect(owner.state.rows.single.runtime!.value.state.wire,'secret_conflict');
    final accepted=owner.state.rows.single;owner.markReadRow(accepted);
    expect(owner.state.unread,0);
    await backend.refreshState();await flush();
    expect(owner.state.rows,hasLength(1));expect(owner.state.unread,0);expect(api.eventSubscriptions,1);
  });

  test('old Alice owner cannot consume Bob startup delivery before Bob common owner mounts',() async {
    final alice=CriticalNoticeOwner(scope:Object(),profileId:profile,isCurrent:()=>true,
      runtimeEvents:backend.runtimeNoticeEvents);addTearDown(alice.dispose);
    final pending=Completer<String>();api.pendingProfiles=pending;
    api.events.add(jsonEncode({'type':'vault_unlocked','profile_id':other}));await flush();
    api.events.add(jsonEncode(startupChange('security','integrity_warning',owner:other)));await flush();
    expect(alice.state.rows,isEmpty);
    api.current=profiles(other,'unlocked',scopeB);pending.complete(api.current);await flush();
    // Alice remains subscribed/current deliberately while the UI provider catches up.
    expect(alice.state.rows,isEmpty);
    final bob=CriticalNoticeOwner(scope:Object(),profileId:other,isCurrent:()=>true,
      runtimeEvents:backend.runtimeNoticeEvents);addTearDown(bob.dispose);
    expect(bob.state.rows,hasLength(1));expect(bob.state.rows.single.runtime!.value.state.wire,'integrity_warning');
    bob.dismissRow(bob.state.rows.single);
    await backend.refreshState();await flush();expect(bob.state.rows,isEmpty);
    expect(api.eventSubscriptions,1);
  });

  Map<String,dynamic> nativeSnapshot(List<Map<String,dynamic>> records,{String nonce=scopeB,
      String owner=profile,bool incomplete=false}) => {
    'version':1,'profile_id':owner,'scope_id':nonce,'records':records,'incomplete':incomplete,
  };
  String snapshotProfiles(Object snapshot,{String nonce=scopeB,String owner=profile}) {
    final rows=jsonDecode(profiles(owner,'unlocked',nonce)) as List;
    (rows.single as Map)['critical_notice_snapshot']=snapshot;
    return jsonEncode(rows);
  }
  test('premarker Native facts reach late common owner only through actual profiles snapshot proof',() async {
    final pending=Completer<String>();api.pendingProfiles=pending;
    final refresh=backend.refreshState();await flush();
    expect(frames.where((frame)=>frame.change!=null),isEmpty);
    final literal=nativeSnapshot([
      startupChange('security','integrity_warning',revision:2),
      startupChange('team','invalid_log',revision:1),
    ]);
    api.current=snapshotProfiles(literal);pending.complete(api.current);await refresh;await flush();
    final owner=lateOwner();expect(owner.state.rows,hasLength(2));
    expect(owner.state.rows.map((row)=>row.runtime!.value.domain.name),containsAll(['security','team']));
    expect(api.eventSubscriptions,1);
  });
  test('snapshot merges pending live metadata by original record revision without duplicate or old failure',() async {
    final pending=Completer<String>();api.pendingProfiles=pending;
    api.events.add(jsonEncode({'type':'vault_unlocked','profile_id':profile}));await flush();
    api.events.add(jsonEncode(startupChange('edit','synced',revision:5)));
    api.events.add(jsonEncode(startupChange('security','secret_conflict',revision:6)));await flush();
    api.current=snapshotProfiles(nativeSnapshot([
      startupChange('edit','error',revision:4),
      startupChange('security','integrity_warning',revision:2),
    ]));pending.complete(api.current);await flush();
    final owner=lateOwner();expect(owner.state.rows,hasLength(1));
    expect(owner.state.rows.single.runtime!.value.state.wire,'secret_conflict');
  });
  test('same Native owner snapshot is one-time and cannot resurrect dismissed rows after observer resets',() async {
    api.current=snapshotProfiles(nativeSnapshot([startupChange('security','integrity_warning')]));
    await backend.refreshState();await flush();final owner=lateOwner();
    expect(owner.state.rows,hasLength(1));owner.dismissRow(owner.state.rows.single);
    await backend.refreshState();await flush();expect(owner.state.rows,isEmpty);
    api.events.add(jsonEncode({'type':'profiles_changed'}));await flush();expect(owner.state.rows,isEmpty);
    await owner.dispose();final replacement=lateOwner();
    await backend.refreshState();await flush();expect(replacement.state.rows,isEmpty);
    expect(api.eventSubscriptions,1);
  });
  test('snapshot foreign owner/schema/private extras fail closed despite an otherwise valid profiles proof',() async {
    final snapshots=<Map<String,dynamic>>[
      nativeSnapshot([startupChange('security','integrity_warning')],owner:other),
      {...nativeSnapshot([startupChange('security','integrity_warning')]),'version':2},
      nativeSnapshot([{...startupChange('security','integrity_warning'),'raw':'SYNTHETIC_PRIVATE'}]),
      nativeSnapshot([startupChange('security','integrity_warning'),startupChange('security','secret_conflict')]),
    ];
    var index=1;
    for(final malformed in snapshots){
      final nonce='99999999-9999-4999-8999-${(index++).toString().padLeft(12,'0')}';
      malformed['scope_id']=nonce;
      for(final row in malformed['records'] as List){(row as Map)['scope_id']=nonce;}
      api.current=snapshotProfiles(malformed,nonce:nonce);await backend.refreshState();await flush();
      final owner=lateOwner();expect(owner.state.rows,isEmpty);expect(owner.state.runtimeIncomplete,isTrue);
      await owner.dispose();
    }
  });
  test('stale original refresh cannot admit its snapshot after a real lock boundary',() async {
    final pending=Completer<String>();api.pendingProfiles=pending;
    final old=backend.refreshState();await flush();
    api.current=profiles(profile,'locked',null);
    api.events.add(jsonEncode({'type':'vault_locked','profile_id':profile}));await flush();
    pending.complete(snapshotProfiles(nativeSnapshot([startupChange('backup','failed')])));
    await old;await flush();expect(backend.runtimeNoticeEvents.currentScope.scope,isNull);
    api.current=profiles(profile,'unlocked',scopeA);
    api.events.add(jsonEncode({'type':'vault_unlocked','profile_id':profile}));await flush();
    final owner=lateOwner();expect(owner.state.rows,isEmpty);
  });
  test('snapshot plus live pending has one bounded64 latest metadata view with conservative loss',() async {
    final pending=Completer<String>();api.pendingProfiles=pending;
    api.events.add(jsonEncode({'type':'vault_unlocked','profile_id':profile}));await flush();
    for(var i=65;i<=84;i++){
      api.events.add(jsonEncode(startupChange('transfer','failed',revision:i,
        record:'77777777-7777-4777-8777-${i.toString().padLeft(12,'0')}')));
    }
    await flush();
    final records=[for(var i=64;i>=1;i--)startupChange('transfer','failed',revision:i,
      record:'77777777-7777-4777-8777-${i.toString().padLeft(12,'0')}')];
    api.current=snapshotProfiles(nativeSnapshot(records));pending.complete(api.current);await flush();
    final owner=lateOwner();expect(owner.state.rows,hasLength(64));expect(owner.state.runtimeIncomplete,isTrue);
    expect(owner.state.rows.where((row)=>row.runtime!.value.revision>=65),hasLength(20));
  });

}
