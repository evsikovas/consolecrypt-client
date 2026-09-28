import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/groups/group_tree.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('subtree counts include nested hosts once and exclude other branches', () {
    final root = Group.create('Production');
    final child = Group.create('Databases', parentId: root.id);
    final leaf = Group.create('Replica', parentId: child.id);
    final other = Group.create('Staging');
    final groups = {
      for (final g in [root, child, leaf, other]) g.id: g,
    };
    final hosts = [
      Host.create(name: 'direct', address: 'direct.test').withGroup(root.id),
      Host.create(name: 'nested', address: 'nested.test').withGroup(leaf.id),
      Host.create(name: 'other', address: 'other.test').withGroup(other.id),
      Host.create(name: 'ungrouped', address: 'ungrouped.test'),
    ];
    expect(groupSubtree(groups, child.id), {child.id, leaf.id});
    expect(groupHostCounts(groups, hosts), {root.id: 2, child.id: 1, leaf.id: 1, other.id: 1});
  });

  test('malformed cycle and orphan remain reachable and cannot loop counts or paths', () {
    final aId = ObjectId.generate();
    final bId = ObjectId.generate();
    final now = DateTime.now().toUtc();
    final a = Group(id: aId, name: 'A', parentId: bId, createdAt: now, updatedAt: now);
    final b = Group(id: bId, name: 'B', parentId: aId, createdAt: now, updatedAt: now);
    final orphan = Group.create('Orphan', parentId: ObjectId.generate());
    final groups = {
      for (final g in [b, a, orphan]) g.id: g,
    };
    final nodes = flattenGroupTree(groups.values.toList());
    expect(nodes.map((n) => n.group.id).toSet(), groups.keys.toSet());
    expect(nodes, hasLength(3));
    expect(groupSubtree(groups, aId), {aId, bId});
    expect(groupPathName(groups, aId), 'B › A');
    expect(groupHostCounts(groups, [Host.create(name: 'host', address: 'host.test').withGroup(aId)]), {aId: 1, bId: 1});
  });
}
