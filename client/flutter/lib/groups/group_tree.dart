import 'package:consolecrypt/core/models/models.dart';

/// "Production › Databases" for a group id.
String groupPathName(Map<ObjectId, Group> groups, ObjectId? groupId) {
  final names = <String>[];
  var cursor = groupId;
  final seen = <ObjectId>{};
  while (cursor != null && seen.add(cursor)) {
    final g = groups[cursor];
    if (g == null) break;
    names.insert(0, g.name);
    cursor = g.parentId;
  }
  return names.join(' › ');
}

typedef GroupNode = ({Group group, int depth});

/// Stable pre-order traversal. Orphans and malformed cycles stay reachable.
List<GroupNode> flattenGroupTree(List<Group> groups) {
  final byParent = <ObjectId?, List<Group>>{};
  final ids = {for (final g in groups) g.id};
  final sorted = [...groups]..sort((a, b) => a.name.toLowerCase().compareTo(b.name.toLowerCase()));
  for (final g in sorted) {
    byParent.putIfAbsent(ids.contains(g.parentId) ? g.parentId : null, () => []).add(g);
  }
  final out = <GroupNode>[];
  final seen = <ObjectId>{};
  void visit(Group group, int depth) {
    if (!seen.add(group.id)) return;
    out.add((group: group, depth: depth));
    for (final child in byParent[group.id] ?? <Group>[]) {
      visit(child, depth + 1);
    }
  }

  for (final root in byParent[null] ?? <Group>[]) {
    visit(root, 0);
  }
  for (final group in sorted) {
    if (!seen.contains(group.id)) visit(group, 0);
  }
  return out;
}

/// Includes the selected group so counts and filtering use the same scope.
Set<ObjectId> groupSubtree(Map<ObjectId, Group> groups, ObjectId groupId) {
  final ids = <ObjectId>{groupId};
  var grew = true;
  while (grew) {
    grew = false;
    for (final group in groups.values) {
      if (ids.contains(group.parentId) && ids.add(group.id)) grew = true;
    }
  }
  return ids;
}

/// Counts each host once per ancestor; missing parents and cycles cannot loop.
Map<ObjectId, int> groupHostCounts(Map<ObjectId, Group> groups, List<Host> hosts) {
  final counts = <ObjectId, int>{};
  for (final host in hosts) {
    var cursor = host.groupId;
    final seen = <ObjectId>{};
    while (cursor != null && groups.containsKey(cursor) && seen.add(cursor)) {
      counts.update(cursor, (count) => count + 1, ifAbsent: () => 1);
      cursor = groups[cursor]?.parentId;
    }
  }
  return counts;
}
