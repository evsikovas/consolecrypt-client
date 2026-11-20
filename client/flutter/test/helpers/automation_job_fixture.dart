String uuid(int value) => '00000000-0000-0000-0000-${value.toRadixString(16).padLeft(12, '0')}';
final profile = uuid(1), host = uuid(2), source = uuid(3), binding = uuid(4), project = uuid(5);
final digest = List.filled(64, 'a').join();
final at = DateTime.utc(2026, 11, 12, 12);
int seconds(DateTime value) => value.millisecondsSinceEpoch ~/ 1000;
Map<String, dynamic> row({int id = 100, List<String>? hosts, int revision = 1, String state = 'succeeded'}) {
  final ids = hosts ?? [host];
  return {
    'version': 1,
    'job_id': uuid(id),
    'project_id': project,
    'notification_revision': revision,
    'state': state,
    'host_ids': [...ids],
    'manifest_digest': digest,
    'accepted_at': seconds(at),
    'updated_at': seconds(at.add(const Duration(minutes: 1))),
    'manifest': {
      'version': 1,
      'job_id': uuid(id),
      'tenant_id': profile,
      'project_id': project,
      'revision_digest': digest,
      'targets': [
        for (final id in ids) {'host_id': id, 'address': 'never-retain.example', 'username': 'secret-sentinel'},
      ],
    },
    'runner_result': {
      'version': 1,
      'job_id': uuid(id),
      'manifest_digest': digest,
      'accepted_at': '2026-11-12T12:00:10.123456Z',
      'updated_at': '2026-11-12T12:00:50Z',
      'output': 'secret-sentinel',
    },
  };
}

Map<String, dynamic> catalog(List<Object?> rows) => {'schema_version': 1, 'read_only': false, 'jobs': rows};
