import 'agent_metrics_disk_fixture.dart' as disk;
import 'agent_metrics_fixture.dart' as legacy;

/// SYNTHETIC exact a216 runtime DTO tuples only. These bytes are not an actual
/// Node serializer capture, an ARM installation, an Actor ACK, or runtime proof.
Map<String, dynamic> runtime({bool arm = true}) => <String, dynamic>{
  'kind': 'vmagent',
  'upstreamTag': 'v1.156.0',
  'platform': arm ? 'linux-aarch64' : 'linux-x86_64',
  'binarySha256': arm
      ? '564346d8acd0979b62c2671ad586d63b243a925307bfa54d0d5f94bd50dda4f5'
      : '2039bc05b6ad5c96b34b72801908896bf8a6a7bcab8ae32120cbb8e6217ef6f6',
  'binaryBytes': arm ? 21299768 : 23209584,
};
Map<String, dynamic> manifest({bool arm = true}) => <String, dynamic>{
  ...legacy.manifest(),
  'schema_version': 2,
  'runtime': runtime(arm: arm),
};
Map<String, dynamic> collectorView({bool arm = true}) => <String, dynamic>{
  ...legacy.view(),
  'manifest': manifest(arm: arm),
};

/// SYNTHETIC public disk-v2 metadata. Not measured ELF bytes or platform proof.
Map<String, dynamic> diskRuntime({bool arm = false}) => <String, dynamic>{
  'kind': 'prometheusHardmode',
  'upstreamTag': '3.16.0',
  'platform': arm ? 'linux-aarch64' : 'linux-x86_64',
  'binarySha256': '9' * 64,
  'binaryBytes': 64,
};
Map<String, dynamic> diskCollectorView({bool arm = false}) {
  final result = disk.diskView();
  (result['manifest'] as Map<String, dynamic>)
    ..['schema_version'] = 2
    ..['runtime'] = diskRuntime(arm: arm);
  return result;
}
