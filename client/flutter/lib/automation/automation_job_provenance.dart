/// Validates original admitted job facts before suggesting an Ansible repair.
Set<String>? automationApprovedJobHostIds(
  Map<Object?, Object?> row, {
  required String profileId,
  required String jobId,
  required int revision,
  required String projectId,
  required String manifestDigest,
}) {
  final uuid = RegExp(r'^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$');
  if (![profileId, jobId, projectId].every(uuid.hasMatch) ||
      revision < 1 ||
      revision > 9007199254740991 ||
      !RegExp(r'^[0-9a-f]{64}$').hasMatch(manifestDigest)) {
    return null;
  }
  final manifest = row['manifest'];
  if (row['version'] is! int ||
      row['version'] != 1 ||
      row['job_id'] != jobId ||
      row['notification_revision'] is! int ||
      row['notification_revision'] != revision ||
      row['manifest_digest'] != manifestDigest ||
      row['project_id'] != projectId ||
      !const {'running', 'failed', 'cancelled', 'interrupted', 'unknown', 'acknowledged'}.contains(row['state']) ||
      manifest is! Map ||
      manifest['version'] is! int ||
      manifest['version'] != 1 ||
      manifest['job_id'] != jobId ||
      manifest['tenant_id'] != profileId ||
      manifest['project_id'] != projectId ||
      row['host_ids'] is! List ||
      manifest['targets'] is! List) {
    return null;
  }
  final hosts = row['host_ids'] as List, targets = manifest['targets'] as List;
  if (hosts.isEmpty ||
      hosts.length > 10000 ||
      targets.length != hosts.length ||
      hosts.any((host) => host is! String || !uuid.hasMatch(host))) {
    return null;
  }
  final declared = hosts.toSet(), approved = <String>{};
  for (final target in targets) {
    if (target is! Map ||
        target['host_id'] is! String ||
        !uuid.hasMatch(target['host_id'] as String) ||
        !approved.add(target['host_id'] as String)) {
      return null;
    }
  }
  return declared.length == hosts.length && declared.length == approved.length && declared.containsAll(approved)
      ? Set.unmodifiable(approved)
      : null;
}
