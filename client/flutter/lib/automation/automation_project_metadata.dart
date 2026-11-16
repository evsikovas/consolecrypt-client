import 'dart:convert';

import 'package:consolecrypt/automation/automation_project_credentials.dart';
import 'package:consolecrypt/automation/automation_service.dart';

/// Optional read/publish metadata capability. No local-root, run or secret API.
abstract interface class AutomationProjectMetadataService {
  Future<AutomationJson> projectMetadataContext(String projectId);
  Future<AutomationJson> projectMetadataCatalog();
  Future<AutomationJson> publishProjectMetadata(AutomationJson originalFrame);
}

Never _bad() => throw const FormatException('automation_metadata_changed_or_unavailable');
final _uuid = RegExp(r'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$');
final _digest = RegExp(r'^[0-9a-f]{64}$');
String _id(Object? value) =>
    value is String && _uuid.hasMatch(value) && value != '00000000-0000-0000-0000-000000000000' ? value : _bad();
int _integer(Object? value, {int minimum = 0, int maximum = 9007199254740991}) =>
    value is int && value >= minimum && value <= maximum ? value : _bad();
String _sha(Object? value) => value is String && _digest.hasMatch(value) ? value : _bad();
bool _text(String value, int bytes) =>
    value.isNotEmpty &&
    utf8.encode(value).length <= bytes &&
    utf8.decode(utf8.encode(value)) == value &&
    !value.runes.any((c) => c < 32 || c >= 127 && c <= 159 || c >= 0xd800 && c <= 0xdfff);
AutomationJson _closed(Object? value, Set<String> required, [Set<String> optional = const <String>{}]) {
  if (value is! AutomationJson ||
      !required.every(value.containsKey) ||
      value.keys.any((key) => !required.contains(key) && !optional.contains(key))) {
    _bad();
  }
  return value;
}

void _v1(Object? value) {
  if (value is! int || value != 1) _bad();
}

void _budget(Object? value, [int bytes = 524288]) {
  if (utf8.encode(jsonEncode(value)).length > bytes) _bad();
}

List<String> _ids(Object? value, {int maximum = 10000}) {
  if (value is! List || value.length > maximum) _bad();
  final ids = value.map(_id).toList(growable: false);
  if (ids.toSet().length != ids.length) _bad();
  return List.unmodifiable(ids);
}

final class ProjectMetadataGitReference {
  ProjectMetadataGitReference._(this.url, this.reference, this.commit);
  final String url, reference, commit;
  factory ProjectMetadataGitReference.parse(Object? value) {
    final row = _closed(value, {'url', 'reference', 'commit'});
    final url = row['url'], reference = row['reference'], commit = row['commit'];
    if (url is! String ||
        !_text(url, 2048) ||
        url.contains(RegExp(r'[\s\\]')) ||
        reference is! String ||
        !_text(reference, 240) ||
        commit is! String ||
        !RegExp(r'^(?:[0-9a-f]{40}|[0-9a-f]{64})$').hasMatch(commit)) {
      _bad();
    }
    final parsed = Uri.tryParse(url);
    if (parsed == null ||
        parsed.scheme != 'https' ||
        parsed.host.isEmpty ||
        parsed.userInfo.isNotEmpty ||
        parsed.hasQuery ||
        parsed.hasFragment ||
        parsed.hasPort && parsed.port == 0) {
      _bad();
    }
    if (reference.startsWith('-') ||
        reference.startsWith('/') ||
        reference.endsWith('/') ||
        reference.endsWith('.') ||
        reference.endsWith('.lock') ||
        reference.contains('..') ||
        reference.contains('//')) {
      _bad();
    }
    return ProjectMetadataGitReference._(url, reference, commit);
  }
}

/// Inert metadata only. No field can be converted to a runnable local project.
final class ProjectMetadataRecord {
  ProjectMetadataRecord._(
    this.id,
    this.revision,
    this.name,
    this.sourceKind,
    this.revisionDigest,
    this.files,
    this.bytes,
    this.git,
    this.hostReferences,
    this.credentials,
    this.unresolvedReferences,
  );
  final String id, name, sourceKind, revisionDigest;
  final int revision, files, bytes;
  final ProjectMetadataGitReference? git;
  final Set<String> hostReferences;
  final ProjectCredentialsPolicy? credentials;
  final List<String> unresolvedReferences;
  bool get readOnly => true;
  bool get localRootBound => false;
  bool get runnable => false;
  factory ProjectMetadataRecord.parse(Object? value) {
    final row = _closed(value, {
      'id',
      'metadata',
      'read_only',
      'unresolved_references',
      'local_root_bound',
      'runnable',
    });
    if (row['read_only'] != true || row['local_root_bound'] != false || row['runnable'] != false) _bad();
    final id = _id(row['id']);
    final metadata = _closed(row['metadata'], {
      'version',
      'revision',
      'name',
      'source_kind',
      'revision_digest',
      'files',
      'bytes',
      'git',
      'inventory',
      'execution_credentials',
    });
    _v1(metadata['version']);
    final name = metadata['name'], kind = metadata['source_kind'];
    if (name is! String ||
        !_text(name, 480) ||
        name.runes.length > 120 ||
        name.trim() != name ||
        kind is! String ||
        !{'snapshot', 'git'}.contains(kind)) {
      _bad();
    }
    final git = metadata['git'] == null ? null : ProjectMetadataGitReference.parse(metadata['git']);
    if ((kind == 'git') != (git != null)) _bad();
    final inventory = _closed(metadata['inventory'], {'version', 'groups'}, {'selected_hosts'}),
        groups = inventory['groups'];
    _integer(inventory['version'], minimum: 1, maximum: 2);
    if (groups is! AutomationJson ||
        groups.length > 64 ||
        !((inventory['version'] == 1 && !inventory.containsKey('selected_hosts')) ||
            (inventory['version'] == 2 && inventory.containsKey('selected_hosts')))) {
      _bad();
    }
    final hosts = <String>{};
    var members = 0;
    for (final entry in groups.entries) {
      if (!RegExp(r'^[A-Za-z_][A-Za-z0-9_-]{0,95}$').hasMatch(entry.key) ||
          {'all', 'ungrouped', '_meta'}.contains(entry.key)) {
        _bad();
      }
      final values = _ids(entry.value);
      members += values.length;
      hosts.addAll(values);
    }
    if (members > 10000) _bad();
    if (inventory.containsKey('selected_hosts')) hosts.addAll(_ids(inventory['selected_hosts']));
    final credentials = ProjectCredentialsPolicy.parse(metadata['execution_credentials']);
    final all = {...hosts};
    for (final environment in credentials?.environments ?? const <DeploymentEnvironment>[]) {
      for (final reference in environment.references) {
        hosts.add(reference.hostId);
        all.add(reference.hostId);
        all.add(reference.credentialId);
      }
    }
    if (all.length > 512) _bad();
    final unresolved = _ids(row['unresolved_references'], maximum: 512);
    if (!unresolved.every(all.contains)) _bad();
    return ProjectMetadataRecord._(
      id,
      _integer(metadata['revision'], minimum: 1),
      name,
      kind,
      _sha(metadata['revision_digest']),
      _integer(metadata['files'], maximum: 10000),
      _integer(metadata['bytes'], maximum: 104857600),
      git,
      Set.unmodifiable(hosts),
      credentials,
      unresolved,
    );
  }
}

final class ProjectMetadataCatalog {
  ProjectMetadataCatalog._(this.revision, this.readOnly, this.records, this.releasedFloor, this.reconciliationRequired);
  final int? revision;
  final bool readOnly, reconciliationRequired;
  final List<ProjectMetadataRecord> records;
  final String? releasedFloor;
  bool get schemaSupported => revision != null;
  ProjectMetadataRecord? find(String id) {
    for (final row in records) {
      if (row.id == id) return row;
    }
    return null;
  }

  factory ProjectMetadataCatalog.parse(Object? value) {
    _budget(value);
    final row = _closed(
      value,
      {'version', 'read_only', 'records', 'files_transferred'},
      {'revision', 'released_preservation_floor', 'sync_state', 'reason'},
    );
    _v1(row['version']);
    if (row['read_only'] is! bool || row['files_transferred'] != false || row['records'] is! List) _bad();
    final values = row['records'] as List;
    if (values.length > 128) _bad();
    final revision = row.containsKey('revision') ? _integer(row['revision']) : null;
    if (revision == null &&
        (row['read_only'] != true || values.isNotEmpty || row.containsKey('released_preservation_floor'))) {
      _bad();
    }
    if (revision != null && !row.containsKey('released_preservation_floor')) _bad();
    final floor = row['released_preservation_floor'];
    if (floor != null &&
        (floor is! String ||
            floor.length > 64 ||
            !RegExp(r'^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$').hasMatch(floor))) {
      _bad();
    }
    final reconciliation = row.containsKey('sync_state') || row.containsKey('reason');
    if (reconciliation &&
        (row['read_only'] != true ||
            row['sync_state'] != 'reconciliation_required' ||
            row['reason'] != 'settings_catalog_conflict_requires_reconciliation')) {
      _bad();
    }
    final records = values.map(ProjectMetadataRecord.parse).toList(growable: false);
    if (records.map((record) => record.id).toSet().length != records.length) _bad();
    return ProjectMetadataCatalog._(
      revision,
      row['read_only'] as bool,
      List.unmodifiable(records),
      floor as String?,
      reconciliation,
    );
  }
}

final class ProjectMetadataFrame {
  ProjectMetadataFrame._(
    this.profileId,
    this.nonce,
    this.projectId,
    this.projectRevision,
    this.settingsId,
    this.settingsRevision,
    this.catalogRevision,
  );
  final String profileId, nonce, projectId, settingsId;
  final int projectRevision, settingsRevision;
  final int? catalogRevision;
  factory ProjectMetadataFrame.parse(
    Object? value, {
    required String profileId,
    required String nonce,
    required AutomationProject project,
  }) {
    final row = _closed(value, {
      'version',
      'profile_id',
      'notification_scope_id',
      'project_id',
      'project_revision',
      'settings_id',
      'settings_revision',
      'catalog_revision',
    });
    _v1(row['version']);
    if (_id(row['profile_id']) != _id(profileId) ||
        !_text(nonce, 256) ||
        row['notification_scope_id'] != nonce ||
        _id(row['project_id']) != _id(project.id) ||
        row['project_revision'] != project.revision ||
        project.readOnly) {
      _bad();
    }
    return ProjectMetadataFrame._(
      profileId,
      nonce,
      project.id,
      _integer(row['project_revision'], minimum: 1),
      _id(row['settings_id']),
      _integer(row['settings_revision'], minimum: 1),
      row['catalog_revision'] == null ? null : _integer(row['catalog_revision']),
    );
  }
  AutomationJson publishRequest(int expectedReferenceRevision) {
    if (catalogRevision == null) _bad();
    return {
      'version': 1,
      'profile_id': profileId,
      'notification_scope_id': nonce,
      'project_id': projectId,
      'project_revision': projectRevision,
      'settings_id': settingsId,
      'settings_revision': settingsRevision,
      'catalog_revision': catalogRevision,
      'expected_reference_revision': _integer(expectedReferenceRevision),
    };
  }
}

final class ProjectMetadataContext {
  ProjectMetadataContext._(this.frame, this.catalog, this.syncState);
  final ProjectMetadataFrame frame;
  final ProjectMetadataCatalog catalog;
  final String syncState;
  bool get canRequestPublish =>
      syncState == 'explicit_admission_required' && catalog.schemaSupported && !catalog.readOnly;
  factory ProjectMetadataContext.parse(
    AutomationJson value, {
    required String profileId,
    required String nonce,
    required AutomationProject project,
  }) {
    _budget(value, 526336);
    final row = _closed(value, {'version', 'frame', 'catalog', 'sync_state'});
    _v1(row['version']);
    final frame = ProjectMetadataFrame.parse(row['frame'], profileId: profileId, nonce: nonce, project: project);
    final catalog = ProjectMetadataCatalog.parse(row['catalog']);
    final state = row['sync_state'];
    if (frame.catalogRevision != catalog.revision ||
        state is! String ||
        !{'local_only', 'floor_undeclared', 'explicit_admission_required', 'reconciliation_required'}.contains(state)) {
      _bad();
    }
    if (state == 'floor_undeclared' && catalog.releasedFloor != null ||
        state == 'explicit_admission_required' && catalog.releasedFloor == null ||
        (state == 'reconciliation_required') != catalog.reconciliationRequired) {
      _bad();
    }
    return ProjectMetadataContext._(frame, catalog, state);
  }
}

ProjectMetadataCatalog checkedProjectMetadataReply(
  AutomationJson value, {
  required String profileId,
  required String nonce,
  String? projectId,
}) {
  _budget(value, 526336);
  final row = _closed(value, {
    'version',
    'profile_id',
    'notification_scope_id',
    'catalog',
    if (projectId != null) 'project_id',
  });
  _v1(row['version']);
  if (_id(row['profile_id']) != _id(profileId) ||
      !_text(nonce, 256) ||
      row['notification_scope_id'] != nonce ||
      projectId != null && _id(row['project_id']) != _id(projectId)) {
    _bad();
  }
  return ProjectMetadataCatalog.parse(row['catalog']);
}
