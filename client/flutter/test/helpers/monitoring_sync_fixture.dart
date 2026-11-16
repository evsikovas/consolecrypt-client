import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';

const syncProfile = '11111111-1111-4111-8111-111111111111';
const syncNonce = '22222222-2222-4222-8222-222222222222';
const syncNextNonce = '33333333-3333-4333-8333-333333333333';
RuntimeNoticeScope syncScope({String profile = syncProfile,String nonce = syncNonce}) => RuntimeNoticeScope.fromProfile({
  'id':profile,'active':true,'vault_state':'unlocked','notification_scope_id':nonce,
})!;
Json syncSnapshot({String profile = syncProfile,String nonce = syncNonce,String storage = 'synced',String server = 'notChecked',
  String? account,String? minimum}) => {
  'version':1,'profileId':profile,'notificationScopeId':nonce,'storage':storage,'releasedMinimumClientVersion':null,
  'readiness':{'state':storage == 'local' ? 'localOnly' : 'floorUndeclared','checkedActiveDevices':0,'blockedDevices':<Json>[]},
  'server':server,'account':account == null ? null : {'state':account,'minimumClientVersion':minimum},
};
