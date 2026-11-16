import 'agent_metrics_fixture.dart' as ram;

/// Synthetic shape from staged Native project_profile/protocol producers.
/// No actual Actor, disk IO, serializer capture, or delivery acceptance.
Map<String, dynamic> diskView({String phase = 'collectorReview'}) {
  final v = ram.view(phase: phase)
    ..remove('crashReplay')
    ..['profile'] = 'maintained-prometheus-disk-512mib-2h-v1'
    ..['bufferMode'] = 'persistentNativeWal'
    ..['diskBudget'] = <String, dynamic>{
      'maxChargedBytes': 536870912,
      'maxResidenceSeconds': 7200,
      'minBavailBytes': 134217728,
      'operationalWalBytes': 67108864,
      'checkpointBytes': 67108864,
      'reservedBytes': 134217728,
      'maxRootEntries': 256,
    }
    ..['automaticRecovery'] = false
    ..['requiresFreshGrantForReplay'] = true
    ..['manifestSha256'] = '1' * 64;
  final manifest = v['manifest'] as Map<String, dynamic>;
  manifest['profile'] = 'maintained-prometheus-disk-512mib-2h-v1';
  manifest['bufferMode'] = 'persistentNativeWal';
  manifest['buffer'] = {'max_bytes': 536870912, 'max_age_seconds': 7200};
  return v;
}
