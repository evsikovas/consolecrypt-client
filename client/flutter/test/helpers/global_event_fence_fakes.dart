import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/src/rust/api/app.dart';
import 'package:consolecrypt/src/rust/frb_generated.dart';

const globalEventProfile = '11111111-1111-4111-8111-111111111111';
const globalEventScopeA = '44444444-4444-4444-8444-444444444444';
const globalEventScopeB = '55555555-5555-4555-8555-555555555555';
const globalEventJob = '33333333-3333-4333-8333-333333333333';
String globalEventProfiles(String id, String state, [String nonce = globalEventScopeA]) => jsonEncode([
  {
    'id': id,
    'active': true,
    'display_name': 'Owned event fixture',
    'kind': 'local',
    'created_at_ms': 1,
    'vault_state': state,
    'notification_scope_id': nonce,
  },
]);
Future<void> flushGlobalEvents() async {
  for (var i = 0; i < 150; i++) {
    await Future<void>.value();
  }
}

// Synthetic Native API only: no real profile, provider, RPC, file or transport.
// A synchronous feed makes the before-refresh admission race deterministic.
class GlobalEventNativeFixture implements RustLibApi {
  // Both streams are closed in tearDown, after backend subscriptions are cancelled.
  // ignore: close_sinks
  final events = StreamController<String>.broadcast(sync: true);
  // Both controllers are closed in tearDown after cancelling hub subscriptions.
  // ignore: close_sinks
  final prompts = StreamController<String>.broadcast(sync: true);
  int eventSubscriptions = 0, profileReads = 0;
  String current = globalEventProfiles(globalEventProfile, 'unlocked');
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
    profileReads++;
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
      return Future<String>.value(jsonEncode({'id': globalEventJob, 'vault_name': 'Owned fixture vault'}));
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
