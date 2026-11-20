import 'dart:convert';
import 'dart:typed_data';

enum InventoryExportReason {
  invalidRequest,
  contextChanged,
  hostNotSelected,
  unknownGroup,
  invalidGroups,
  unsupportedRoute,
  unsupportedEndpoint,
  aliasConflict,
  size,
  invalidResponse,
  failed,
}

final class InventoryExportFailure implements Exception {
  const InventoryExportFailure(this.reason);
  final InventoryExportReason reason;
}

InventoryExportReason? inventoryExportNativeReason(Object? field, Object? rule) {
  if (field != 'automation_inventory_export') return null;
  return switch (rule) {
    'inventory_export_invalid_request' => InventoryExportReason.invalidRequest,
    'inventory_export_context_changed' => InventoryExportReason.contextChanged,
    'inventory_export_host_not_selected' => InventoryExportReason.hostNotSelected,
    'inventory_export_unknown_group' => InventoryExportReason.unknownGroup,
    'inventory_export_invalid_groups' => InventoryExportReason.invalidGroups,
    'inventory_export_unsupported_route' => InventoryExportReason.unsupportedRoute,
    'inventory_export_unsupported_endpoint' => InventoryExportReason.unsupportedEndpoint,
    'inventory_export_alias_conflict' => InventoryExportReason.aliasConflict,
    'inventory_export_size' => InventoryExportReason.size,
    _ => InventoryExportReason.failed,
  };
}

bool _id(Object? value) =>
    value is String &&
    value != '00000000-0000-0000-0000-000000000000' &&
    RegExp(r'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$').hasMatch(value);
bool _group(Object? value) =>
    value is String &&
    RegExp(r'^[A-Za-z_][A-Za-z0-9_-]{0,95}$').hasMatch(value) &&
    !const {'all', 'ungrouped', '_meta'}.contains(value);
Never _invalid() => throw const InventoryExportFailure(InventoryExportReason.invalidResponse);
Map<String, dynamic> _map(Object? value) => value is Map<String, dynamic> ? value : _invalid();
int _positive(Object? value) => value is int && value > 0 ? value : _invalid();
List<String> _strings(Object? value, {int? max}) {
  if (value is! List || value.any((v) => v is! String) || (max != null && value.length > max)) {
    _invalid();
  }
  final result = value.cast<String>();
  if (result.toSet().length != result.length) _invalid();
  return List.unmodifiable(result);
}

void _keys(Map<String, dynamic> value, Set<String> expected) {
  if (value.length != expected.length || !value.keys.every(expected.contains)) {
    _invalid();
  }
}

bool _same(Iterable<String> a, Iterable<String> b) {
  final left = a.toSet(), right = b.toSet();
  return left.length == right.length && left.containsAll(right);
}

final class InventoryExportRequest {
  InventoryExportRequest({
    required this.projectId,
    required this.expectedRevision,
    required Iterable<String> hostIds,
    required Iterable<String> groupNames,
  }) : hostIds = List.unmodifiable(hostIds),
       groupNames = List.unmodifiable(groupNames) {
    if (!_id(projectId) ||
        expectedRevision < 1 ||
        this.hostIds.isEmpty ||
        this.hostIds.length > 32 ||
        this.groupNames.length > 64 ||
        this.hostIds.toSet().length != this.hostIds.length ||
        this.groupNames.toSet().length != this.groupNames.length ||
        !this.hostIds.every(_id) ||
        !this.groupNames.every(_group)) {
      throw const InventoryExportFailure(InventoryExportReason.invalidRequest);
    }
  }
  final String projectId;
  final int expectedRevision;
  final List<String> hostIds, groupNames;
  Map<String, dynamic> toJson() => {
    'version': 1,
    'projectId': projectId,
    'expectedRevision': expectedRevision,
    'hostIds': hostIds,
    'groupNames': groupNames,
  };
}

/// Metadata-only projection of Native's existing inventoryConfiguration view.
/// It restricts displayed selection; it never grants export permission.
final class InventoryExportContext {
  InventoryExportContext._(
    this.projectId,
    this.revision,
    this.groups,
    this.selectedHosts,
    this.boundHosts,
    this.externalOnly,
  );
  factory InventoryExportContext.fromConfiguration(String projectId, Map<String, dynamic> value) {
    if (!_id(projectId) || value['version'] != 1 || value['project_id'] != projectId) {
      _invalid();
    }
    final revision = _positive(value['project_revision']);
    final inventory = _map(value['saved_inventory']);
    final version = inventory['version'];
    if (version != 1 && version != 2) _invalid();
    final groups = _map(inventory['groups']);
    if (groups.length > 64 || !groups.keys.every(_group)) _invalid();
    for (final members in groups.values) {
      if (!_strings(members).every(_id)) _invalid();
    }
    final selected = version == 2 ? _strings(inventory['selected_hosts']) : null;
    if (version == 1 && inventory.containsKey('selected_hosts') || selected != null && !selected.every(_id)) {
      _invalid();
    }
    final bound = <String>{};
    var externalOnly = false;
    if (value['configuration'] != null) {
      final configuration = _map(value['configuration']);
      if (configuration['version'] != 1 || !const {'mixed', 'external_only'}.contains(configuration['mode'])) {
        _invalid();
      }
      externalOnly = configuration['mode'] == 'external_only';
      for (final binding in _map(configuration['bindings']).values) {
        final id = _map(binding)['host_id'];
        if (!_id(id)) _invalid();
        bound.add(id as String);
      }
    }
    final names = groups.keys.toList()..sort();
    return InventoryExportContext._(
      projectId,
      revision,
      List.unmodifiable(names),
      selected == null ? null : Set.unmodifiable(selected),
      Set.unmodifiable(bound),
      externalOnly,
    );
  }
  final String projectId;
  final int revision;
  final List<String> groups;
  final Set<String>? selectedHosts;
  final Set<String> boundHosts;
  final bool externalOnly;
  bool permits(String id) =>
      !externalOnly && (selectedHosts == null || selectedHosts!.contains(id) || boundHosts.contains(id));
  bool allows(InventoryExportRequest request) =>
      projectId == request.projectId &&
      revision == request.expectedRevision &&
      request.hostIds.every(permits) &&
      request.groupNames.every(groups.contains);
}

/// Only constructible by validating a fresh Native response for an exact request.
/// The original text is saved unchanged; JSON decoding is never used to emit data.
final class IssuedInventorySnapshot {
  IssuedInventorySnapshot._(
    this.nativeJson,
    this.snapshotId,
    this.issuedAtUnixSeconds,
    this.hostCount,
    this.groupCount,
  );
  factory IssuedInventorySnapshot.fromNative(String text, InventoryExportRequest request) {
    if (utf8.encode(text).length > 256 * 1024) {
      throw const InventoryExportFailure(InventoryExportReason.size);
    }
    final value = _strictObject(text);
    _keys(value, const {'format', 'version', 'snapshotId', 'issuedAtUnixSeconds', 'origin', 'selection', 'inventory'});
    if (value['format'] != 'consolecrypt.inventory.snapshot' || value['version'] != 1 || !_id(value['snapshotId'])) {
      _invalid();
    }
    final issued = _positive(value['issuedAtUnixSeconds']);
    final origin = _map(value['origin']);
    _keys(origin, const {'kind', 'projectId', 'projectRevision', 'workingVersion'});
    if (origin['kind'] != 'native' ||
        origin['projectId'] != request.projectId ||
        origin['projectRevision'] != request.expectedRevision) {
      _invalid();
    }
    if (origin['workingVersion'] is! int || (origin['workingVersion'] as int) < 0) {
      _invalid();
    }
    final selection = _map(value['selection']);
    _keys(selection, const {'hostIds', 'groupNames'});
    if (!_same(_strings(selection['hostIds'], max: 32), request.hostIds) ||
        !_same(_strings(selection['groupNames'], max: 64), request.groupNames)) {
      _invalid();
    }
    final aliases = {for (final id in request.hostIds) 'cc_${id.replaceAll('-', '')}'};
    final inventory = _map(value['inventory']);
    _keys(inventory, {'all', 'ungrouped', '_meta', ...request.groupNames});
    List<String> hosts(String name, Iterable<String> children) {
      final group = _map(inventory[name]);
      _keys(group, const {'hosts', 'children', 'vars'});
      final rows = _strings(group['hosts'], max: 32);
      if (!aliases.containsAll(rows) ||
          !_same(_strings(group['children'], max: 64), children) ||
          _map(group['vars']).isNotEmpty) {
        _invalid();
      }
      return rows;
    }

    if (!_same(hosts('all', request.groupNames), aliases)) _invalid();
    final grouped = {for (final group in request.groupNames) ...hosts(group, const [])};
    if (!_same(hosts('ungrouped', const []), aliases.difference(grouped))) {
      _invalid();
    }
    final meta = _map(inventory['_meta']);
    _keys(meta, const {'hostvars'});
    final hostvars = _map(meta['hostvars']);
    _keys(hostvars, aliases);
    for (final raw in hostvars.values) {
      final vars = _map(raw);
      _keys(vars, const {'ansible_host', 'ansible_port', 'ansible_user'});
      final address = vars['ansible_host'], username = vars['ansible_user'], port = vars['ansible_port'];
      if (address is! String ||
          address.isEmpty ||
          address.length > 253 ||
          !RegExp(r'^[A-Za-z0-9:.\-]+$').hasMatch(address) ||
          username is! String ||
          username.isEmpty ||
          username.length > 128 ||
          username.startsWith('-') ||
          !RegExp(r'^[A-Za-z0-9_.@\\-]+$').hasMatch(username) ||
          port is! int ||
          port < 1 ||
          port > 65535) {
        _invalid();
      }
    }
    return IssuedInventorySnapshot._(
      text,
      value['snapshotId'] as String,
      issued,
      request.hostIds.length,
      request.groupNames.length,
    );
  }
  final String nativeJson, snapshotId;
  final int issuedAtUnixSeconds, hostCount, groupCount;
  Uint8List get bytes => Uint8List.fromList(utf8.encode(nativeJson));
}

Map<String, dynamic> _strictObject(String text) {
  final Object? value;
  try {
    value = jsonDecode(text);
  } on FormatException {
    _invalid();
  }
  final keys = <Set<String>?>[], expectingKey = <bool>[];
  for (final match in RegExp(r'"(?:[^"\\]|\\.)*"|[{}\[\],:]').allMatches(text)) {
    final token = match[0]!;
    if (token == '{' || token == '[') {
      keys.add(token == '{' ? <String>{} : null);
      expectingKey.add(token == '{');
    } else if (token == '}' || token == ']') {
      keys.removeLast();
      expectingKey.removeLast();
    } else if (keys.isNotEmpty && keys.last != null) {
      if (token == ',') expectingKey[expectingKey.length - 1] = true;
      if (token == ':') expectingKey[expectingKey.length - 1] = false;
      if (token.startsWith('"') && expectingKey.last && !keys.last!.add(jsonDecode(token) as String)) {
        _invalid();
      }
    }
  }
  return _map(value);
}

abstract interface class AutomationInventoryExportService {
  Future<InventoryExportContext> inventoryExportContext(String projectId);
  Future<IssuedInventorySnapshot> issueInventoryExport(InventoryExportRequest request);
}

/// Callback seam maps only to existing metadata read and new Native issuance.
final class AutomationInventoryExportClient implements AutomationInventoryExportService {
  const AutomationInventoryExportClient({required this.configuration, required this.issue});
  final Future<Map<String, dynamic>> Function(String projectId) configuration;
  final Future<String> Function(String requestJson) issue;
  @override
  Future<InventoryExportContext> inventoryExportContext(String projectId) async =>
      InventoryExportContext.fromConfiguration(projectId, await configuration(projectId));
  @override
  Future<IssuedInventorySnapshot> issueInventoryExport(InventoryExportRequest request) async =>
      IssuedInventorySnapshot.fromNative(await issue(jsonEncode(request.toJson())), request);
}
