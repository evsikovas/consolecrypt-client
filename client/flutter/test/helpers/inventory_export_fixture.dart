// Synthetic metadata only; these IDs/endpoints are not user data.
const inventoryExportProjectId = '11111111-1111-4111-8111-111111111111';
const inventoryExportHostId = '22222222-2222-4222-8222-222222222222';
const inventoryExportOtherId = '33333333-3333-4333-8333-333333333333';

Map<String, dynamic> inventoryExportConfiguration({List<String>? selected, String? mode}) => {
  'version': 1,
  'project_id': inventoryExportProjectId,
  'project_revision': 7,
  'saved_inventory': {
    'version': selected == null ? 1 : 2,
    'selected_hosts': ?selected,
    'groups': {
      'web': [inventoryExportHostId, inventoryExportOtherId],
      'empty': <String>[],
    },
  },
  'configuration': mode == null
      ? null
      : {
          'version': 1,
          'mode': mode,
          'bindings': {
            'outside': {'host_id': inventoryExportOtherId},
          },
        },
};

Map<String, dynamic> inventoryExportSnapshot({
  List<String> hostIds = const [inventoryExportHostId],
  List<String> groups = const ['web'],
}) {
  String alias(String id) => 'cc_${id.replaceAll('-', '')}';
  final members = groups.contains('web') ? hostIds.map(alias).toList() : <String>[];
  Map<String, dynamic> group(List<String> hosts, [List<String> children = const []]) => {
    'hosts': hosts,
    'children': children,
    'vars': <String, dynamic>{},
  };
  return {
    'format': 'consolecrypt.inventory.snapshot',
    'version': 1,
    'snapshotId': '44444444-4444-4444-8444-444444444444',
    'issuedAtUnixSeconds': 1794390000,
    'origin': {'kind': 'native', 'projectId': inventoryExportProjectId, 'projectRevision': 7, 'workingVersion': 0},
    'selection': {'hostIds': hostIds, 'groupNames': groups},
    'inventory': {
      'all': group(hostIds.map(alias).toList(), groups),
      'ungrouped': group(groups.contains('web') ? [] : hostIds.map(alias).toList()),
      for (final name in groups) name: group(name == 'web' ? members : []),
      '_meta': {
        'hostvars': {
          for (final id in hostIds)
            alias(id): {'ansible_host': '192.0.2.10', 'ansible_port': 2222, 'ansible_user': 'fixture'},
        },
      },
    },
  };
}
