import 'dart:convert';

import 'package:consolecrypt/core/bridge/rust_agent_collector_service.dart';

import 'agent_collector_service_fixture.dart' as v;
import 'agent_collector_service_flow_fixture.dart' as f;

const credentialId = '60000000-0000-4000-8000-000000000006';
Map<String, dynamic> choicesWire() => {
  'version': 1,
  'profileId': f.scope.profileId,
  'notificationScopeId': f.scope.nonce,
  'kind': 'finiteCollectorServiceCredentialChoices',
  'credentials': [
    {'credentialId': credentialId, 'credentialRevision': '7', 'name': 'Owned record'},
  ],
};
Map<String, dynamic> selectedWire({String phase = 'serviceReview'}) => {
  ...v.serviceWire(phase: phase),
  'kind': 'finiteCollectorServiceCredentials',
  'credentialSelections': [
    {
      'target': {'kind': 'scrape', 'targetId': f.catalog.manifest.targets.first.id},
      'credentialId': credentialId,
      'credentialRevision': '7',
    },
    {
      'target': {'kind': 'receiver'},
      'credentialId': credentialId,
      'credentialRevision': '7',
    },
  ],
};

/// Synthetic nonempty closed variant, not a real secret/Node handoff or FFI.
final class CredentialNative implements AgentCollectorServiceNativeHandle {
  final calls = <String>[];
  String phase = 'serviceReview';
  @override
  Future<String> details() async {
    calls.add('details');
    return jsonEncode(selectedWire());
  }

  @override
  Future<String> confirmConfiguration() async {
    calls.add('confirmConfiguration');
    phase = 'configurationRecorded';
    return jsonEncode(selectedWire(phase: phase));
  }

  @override
  void cancel() {
    calls.add('cancel');
    if (phase == 'serviceReview') phase = 'cancelled';
  }

  @override
  Future<String> joined() async {
    calls.add('joined');
    return f.joinedReply(phase: phase, mayRemain: phase == 'configurationRecorded');
  }

  @override
  void dispose() {
    calls.add('dispose');
  }
}
