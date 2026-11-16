import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_panel_sync_consent.dart';

import '../helpers/monitoring_sync_fixture.dart';
export '../helpers/monitoring_sync_fixture.dart';

const account = '55555555-5555-4555-8555-555555555555',
    vault = '66666666-6666-4666-8666-666666666666',
    settings = '77777777-7777-4777-8777-777777777777',
    epoch = 'a8888888-8888-4888-8888-888888888888';
Json policy({bool enabled = false, bool confirm = false}) => {
  'version': 1,
  'profileId': syncProfile,
  'notificationScopeId': syncNonce,
  'kind': confirm
      ? 'ownerConsent'
      : enabled
      ? 'checkpointRecapture'
      : 'blocked',
  'scope': 'allPersonalVaults',
  'accountId': account,
  'accountState': enabled ? 'enabled' : 'off',
  'policyRevision': enabled ? '9223372036854775807' : '0',
  'rosterDigest': 'a' * 64,
  'releasedMinimumClientVersion': confirm ? '2.0.0' : null,
  'offeredMinimumClientVersion': '2.0.0',
  'minimumClientVersion': enabled ? '2.0.0' : null,
  'checkpoint': {'vaultId': vault, 'vaultEpoch': epoch, 'vaultSequence': '9223372036854775807'},
  'catalog': {
    'settingsId': settings,
    'objectRevision': '9223372036854775807',
    'schema': 3,
    'revision': '18446744073709551615',
  },
  'readiness': {
    'state': confirm ? 'serverAdmissionUnverified' : 'floorUndeclared',
    'checkedActiveDevices': confirm ? 1 : 0,
    'blockedDevices': <Json>[],
  },
  'remainingMs': 300000,
  'canConfirm': confirm,
  'canRecheck': enabled,
};
String result({bool recheck = false}) => jsonEncode({
  'version': 1,
  'profileId': syncProfile,
  'notificationScopeId': syncNonce,
  'scope': 'allPersonalVaults',
  'action': recheck ? 'checkpointRecaptured' : 'accountPolicyConfirmed',
  'accountId': account,
  'policyRevision': recheck ? '9223372036854775807' : '1',
  if (recheck) ...{'checkpoint': policy()['checkpoint'], 'catalog': policy()['catalog']},
});

class Handle implements MonitoringPanelSyncConsentHandle {
  Handle(this.value);
  final Json value;
  int detailsCalls = 0, confirms = 0, rechecks = 0, cancels = 0;
  Completer<String>? pendingDetails, pendingAction;
  bool lose = false;
  @override
  Future<String> details() async {
    detailsCalls++;
    return pendingDetails?.future ?? jsonEncode(value);
  }

  @override
  Future<String> confirm() async {
    confirms++;
    if (lose) throw StateError('PRIVATE_NEVER_RENDER');
    return pendingAction?.future ?? result();
  }

  @override
  Future<String> recheckCheckpoint() async {
    rechecks++;
    if (lose) throw StateError('PRIVATE_NEVER_RENDER');
    return pendingAction?.future ?? result(recheck: true);
  }

  @override
  void cancel() {
    cancels++;
  }
}

class Port implements MonitoringPanelSyncConsentPort {
  Port({Json? value}) : handle = Handle(value ?? policy());
  final Handle handle;
  final requests = <Json>[];
  Completer<MonitoringPanelSyncConsentHandle>? pending;
  @override
  Future<MonitoringPanelSyncConsentHandle> prepare(String raw) async {
    requests.add(object(jsonDecode(raw)));
    return pending?.future ?? handle;
  }
}
