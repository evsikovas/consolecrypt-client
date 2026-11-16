import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/bridge/rust_backend.dart';
import 'package:consolecrypt/core/services/job_notice_events.dart';
import 'package:consolecrypt/src/rust/api/app.dart';
import 'package:consolecrypt/src/rust/frb_generated.dart';
import 'package:flutter_test/flutter_test.dart';

const profile = '11111111-1111-4111-8111-111111111111';
const other = '22222222-2222-4222-8222-222222222222';
const job = '33333333-3333-4333-8333-333333333333';
String profiles(String id, String state) => jsonEncode([
  {
    'id': id,
    'active': true,
    'display_name': 'Owned Native fixture',
    'kind': 'local',
    'created_at_ms': 1,
    'vault_state': state,
  },
]);
String changed(String id, int revision) => jsonEncode({
  'type': 'automation_job_changed',
  'profile_id': id,
  'job_id': job,
  'revision': revision,
  'state': 'failed',
});
Future<void> flush() async {
  for (var i = 0; i < 150; i++) {
    await Future<void>.value();
  }
}

class _Native implements RustLibApi {
  // Both streams are closed in tearDown, after backend subscriptions are cancelled.
  // ignore: close_sinks
  final events = StreamController<String>.broadcast(), prompts = StreamController<String>.broadcast();
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

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  late _Native api;
  late RustBackend backend;
  late StreamSubscription<JobNoticeFrame> subscription;
  final frames = <JobNoticeFrame>[];
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
    subscription = backend.jobNoticeEvents.watchJobNotices().listen(frames.add);
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
  test(
    'existing global Native core stream delivers metadata with one subscription and no job read/execute call',
    () async {
      api.events.add(changed(profile, 1));
      await flush();
      expect(api.eventSubscriptions, 1);
      expect(frames.single.change!.state, JobNoticeState.failed);
      expect(frames.single.change!.profileId, profile);
    },
  );
  test('lock event blocks a job immediately even while an original unlocked profiles read completes late', () async {
    final pending = api.pendingProfiles = Completer<String>();
    final oldRefresh = backend.refreshState();
    api.current = profiles(profile, 'locked');
    api.events.add(jsonEncode({'type': 'vault_locked', 'profile_id': profile}));
    api.events.add(changed(profile, 1));
    await flush();
    expect(frames.where((frame) => frame.change != null), isEmpty);
    pending.complete(profiles(profile, 'unlocked'));
    await oldRefresh;
    await flush();
    expect(backend.isUnlocked, isFalse);
    api.events.add(changed(profile, 2));
    await flush();
    expect(frames.where((frame) => frame.change != null), isEmpty);
    expect(backend.jobNoticeEvents.currentScope.profileId, isNull);
  });
  test(
    'profile switch keeps current owner only and rejects a late old-profile event on the actual bridge adapter',
    () async {
      api.current = profiles(other, 'unlocked');
      api.events.add(jsonEncode({'type': 'profiles_changed'}));
      await flush();
      frames.clear();
      api.events.add(changed(profile, 1));
      api.events.add(changed(other, 1));
      await flush();
      expect(frames.where((frame) => frame.change != null), hasLength(1));
      expect(frames.single.change!.profileId, other);
      expect(api.eventSubscriptions, 1);
    },
  );
  test('existing sync, SSH, edit and AI event callbacks are still forwarded by the same hub', () async {
    final observed = <String>[];
    backend.eventListeners.add((event) {
      observed.add(event['type'] as String);
    });
    for (final type in ['terminal_status', 'sync_status', 'edit_status', 'ai_index_status', 'transfer_update']) {
      api.events.add(jsonEncode({'type': type}));
    }
    await flush();
    expect(
      observed,
      containsAll(['terminal_status', 'sync_status', 'edit_status', 'ai_index_status', 'transfer_update']),
    );
    expect(frames.where((frame) => frame.change != null), isEmpty);
    expect(api.eventSubscriptions, 1);
  });
  test('actual lagged/closed core stream reports refresh-needed without a fake completion or restart', () async {
    api.events.add(jsonEncode({'type': 'lagged'}));
    await flush();
    expect(frames.any((frame) => frame.refreshNeeded), isTrue);
    expect(frames.where((frame) => frame.change != null), isEmpty);
    frames.clear();
    await api.events.close();
    await flush();
    expect(frames.single.streamUnavailable, isTrue);
    expect(api.eventSubscriptions, 1);
  });
}
