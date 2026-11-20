import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/automation/automation_project_metadata.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:flutter_test/flutter_test.dart';

const profile = '11111111-1111-4111-8111-111111111111';
const projectId = '22222222-2222-4222-8222-222222222222';
const settingsId = '33333333-3333-4333-8333-333333333333';
const host = '44444444-4444-4444-8444-444444444444';
const nonce = 'opaque-native-scope/owned-1';
const project = AutomationProject(id: projectId, name: 'Owned local source', readOnly: false, revision: 1);
AutomationJson clone(AutomationJson value) => jsonDecode(jsonEncode(value)) as AutomationJson;
AutomationJson record() => {
  'id': projectId,
  'read_only': true,
  'local_root_bound': false,
  'runnable': false,
  'unresolved_references': <String>[],
  'metadata': {
    'version': 1,
    'revision': 1,
    'name': 'Owned local source',
    'source_kind': 'snapshot',
    'revision_digest': 'a' * 64,
    'files': 2,
    'bytes': 100,
    'git': null,
    'execution_credentials': null,
    'inventory': {
      'version': 2,
      'groups': {
        'owned': [host],
      },
      'selected_hosts': [host],
    },
  },
};
AutomationJson catalog({int revision = 0, String? floor, bool withRecord = false}) => {
  'version': 1,
  'revision': revision,
  'read_only': false,
  'records': [if (withRecord) record()],
  'files_transferred': false,
  'released_preservation_floor': floor,
};
AutomationJson context({bool candidate = false}) => {
  'version': 1,
  'frame': <String, dynamic>{
    'version': 1,
    'profile_id': profile,
    'notification_scope_id': nonce,
    'project_id': projectId,
    'project_revision': 1,
    'settings_id': settingsId,
    'settings_revision': 1,
    'catalog_revision': 0,
  },
  'catalog': catalog(floor: candidate ? '2.0.0' : null),
  // Synthetic candidate exercises UI request construction, not a released floor.
  'sync_state': candidate ? 'explicit_admission_required' : 'floor_undeclared',
};
AutomationJson reply({bool published = false}) => {
  'version': 1,
  'profile_id': profile,
  'notification_scope_id': nonce,
  if (published) 'project_id': projectId,
  'catalog': catalog(revision: published ? 1 : 0, floor: '2.0.0', withRecord: published),
};

class MetadataFake implements AutomationProjectMetadataService {
  int reads = 0, catalogs = 0, publishes = 0;
  bool candidate = false;
  Completer<AutomationJson>? pending;
  AutomationJson? request;
  AutomationJson? published;
  Completer<AutomationJson>? publishing;
  @override
  Future<AutomationJson> projectMetadataContext(String id) async {
    expect(id, projectId);
    reads++;
    if (pending != null) return await pending!.future;
    return context(candidate: candidate);
  }

  @override
  Future<AutomationJson> projectMetadataCatalog() async {
    catalogs++;
    return reply();
  }

  @override
  Future<AutomationJson> publishProjectMetadata(AutomationJson value) async {
    publishes++;
    request = clone(value);
    if (publishing != null) return await publishing!.future;
    return published ?? reply(published: true);
  }
}
