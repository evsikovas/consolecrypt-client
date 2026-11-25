import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/models/monitoring_preferences.dart';

class MonitoringInventory {
  MonitoringInventory(List<Host> hosts, List<Group> groups)
    : hosts = hosts.where((h) => !h.isRdp).toList()..sort((a, b) => a.name.compareTo(b.name)),
      groups = {for (final g in groups) g.id: g};

  final List<Host> hosts;
  final Map<ObjectId, Group> groups;

  Iterable<Group> ancestors(ObjectId? id) sync* {
    final visited = <ObjectId>{};
    while (id != null && visited.add(id)) {
      final group = groups[id];
      if (group == null) break;
      yield group;
      id = group.parentId;
    }
  }

  String path(Group group) => ancestors(group.id).map((g) => g.name).toList().reversed.join(' / ');

  bool includedByGroup(Host host, Set<String> selected) =>
      ancestors(host.groupId).any((g) => selected.contains(g.id.value));

  List<Host> resolve(MonitoringPreferences selection) =>
      hosts.where((h) => selection.hosts.contains(h.id.value) || includedByGroup(h, selection.groups)).toList();
}
