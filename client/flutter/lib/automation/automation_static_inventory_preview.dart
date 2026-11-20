import 'dart:collection';

import 'package:yaml/yaml.dart';

final _bracketedHost = RegExp(r'^\[[a-zA-Z0-9_.:%-]{1,110}\](?::[0-9]{1,5})?$');
final _inventoryName = RegExp(r'^[a-zA-Z0-9_][a-zA-Z0-9_.:@%\[\]-]{0,127}$');

/// Extract literal names. This preview never evaluates variables, runs
/// inventory plugins, expands host ranges or resolves SSH options.
Map<String, List<String>> previewStaticInventory(Map<String, String> documents) {
  final groups = <String, Set<String>>{};
  final yaml = <String>[];
  final ini = <String>[];
  for (final MapEntry(:key, :value) in documents.entries) {
    (key.toLowerCase().endsWith('.ini') ? ini : yaml).add(value);
  }
  for (final parsed in [previewStaticYamlInventory(yaml), _previewStaticIniInventory(ini)]) {
    for (final MapEntry(:key, :value) in parsed.entries) {
      (groups[key] ??= <String>{}).addAll(value);
    }
  }
  return {for (final key in groups.keys.toList()..sort()) key: groups[key]!.toList()..sort()};
}

Map<String, Set<String>> _previewStaticIniInventory(List<String> documents) {
  final groups = <String, Set<String>>{};
  final header = RegExp(r'^\[([A-Za-z0-9_.-]{1,96})(?::(children|vars))?\]\s*(?:[#;].*)?$');
  final host = RegExp(r'''^(?:"([^"]+)"|'([^']+)'|([^\s#;]+))(?:\s|[#;]|$)''');
  var lines = 0;
  for (final document in documents) {
    Set<String>? members;
    var mode = '';
    for (final raw in document.split('\n')) {
      if (++lines > 12000) throw const FormatException();
      final line = raw.trim();
      if (line.isEmpty || line.startsWith('#') || line.startsWith(';')) continue;
      if (header.firstMatch(line) case final section?) {
        members = groups.putIfAbsent(section[1]!, () => <String>{});
        mode = section[2] ?? '';
        continue;
      }
      if (mode == 'vars') continue;
      final token = host.firstMatch(line);
      final name = token?[1] ?? token?[2] ?? token?[3];
      if (name == null || (!_inventoryName.hasMatch(name) && !_bracketedHost.hasMatch(name)))
        throw const FormatException();
      if (mode == 'children') {
        groups.putIfAbsent(name, () => <String>{});
      } else {
        (members ??= groups.putIfAbsent('ungrouped', () => <String>{})).add(name);
      }
    }
  }
  return groups;
}

/// Quick view of the literal names in static YAML. Ansible resolves effective
/// hosts, variables, inheritance and connection details separately.
Map<String, List<String>> previewStaticYamlInventory(List<String> documents) {
  final groups = <String, Set<String>>{};
  var nodes = 0;
  for (final document in documents) {
    final yaml = loadYaml(document);
    final visited = HashSet<Object>.identity();
    void readGroups(Map<dynamic, dynamic> values, int depth) {
      if (depth > 16 || !visited.add(values)) return;
      for (final MapEntry(:key, :value) in values.entries) {
        if (++nodes > 4000) throw const FormatException();
        if (key is! String || !_inventoryName.hasMatch(key) || (value != null && value is! Map)) {
          throw const FormatException();
        }
        final members = groups.putIfAbsent(key, () => <String>{});
        if (value is! Map) continue;
        if (value.keys.any((k) => !const ['hosts', 'children', 'vars'].contains(k))) {
          throw const FormatException();
        }
        final hosts = value['hosts'];
        if (hosts is Map) {
          for (final host in hosts.keys) {
            if (++nodes > 4000) throw const FormatException();
            if (host is String && _inventoryName.hasMatch(host)) members.add(host);
          }
        } else if (hosts != null) {
          throw const FormatException();
        }
        final children = value['children'];
        if (children is Map) {
          readGroups(children, depth + 1);
        } else if (children != null) {
          throw const FormatException();
        }
      }
    }

    if (yaml is! Map || yaml.containsKey('plugin')) throw const FormatException();
    readGroups(yaml, 0);
  }
  return {for (final key in groups.keys.toList()..sort()) key: groups[key]!.toList()..sort()};
}

/// Mirrors the native group name shown in the generated Ansible inventory.
String inventoryGroupName(String label, String id) {
  final value = label.trim().replaceAll(RegExp('[^a-zA-Z0-9_]', unicode: true), '_');
  final name = value.length > 92 ? value.substring(0, 92) : value;
  if (!RegExp('[a-zA-Z0-9]').hasMatch(name)) return 'group_${id.replaceAll('-', '')}';
  if (RegExp('^[0-9]').hasMatch(name) || const {'all', 'ungrouped', '_meta'}.contains(name)) return 'cc_$name';
  return name;
}
