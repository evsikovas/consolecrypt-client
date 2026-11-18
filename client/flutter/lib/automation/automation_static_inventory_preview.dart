import 'dart:collection';

import 'package:yaml/yaml.dart';

/// Quick view of the literal names in static YAML. Ansible resolves effective
/// hosts, variables, inheritance and connection details separately.
Map<String, List<String>> previewStaticYamlInventory(List<String> documents) {
  final groups = <String, Set<String>>{};
  final name = RegExp(r'^[a-zA-Z0-9_][a-zA-Z0-9_.:@%\[\]-]{0,127}$');
  var nodes = 0;
  for (final document in documents) {
    final yaml = loadYaml(document);
    final visited = HashSet<Object>.identity();
    void readGroups(Map values, int depth) {
      if (depth > 16 || !visited.add(values)) return;
      for (final MapEntry(:key, :value) in values.entries) {
        if (++nodes > 4000) throw const FormatException();
        if (key is! String || !name.hasMatch(key) || (value != null && value is! Map)) {
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
            if (host is String && name.hasMatch(host)) members.add(host);
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
