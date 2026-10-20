import 'package:consolecrypt/kubernetes/kube_flux_cli_service.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';

abstract interface class KubeKubectlService {
  Future<KubectlDiscovery> discoverKubectl();
  Future<FluxCliVersion> probeKubectl(String path);
  Future<FluxInstallReview> reviewKubectl(String method);
}

final class KubectlDiscovery {
  const KubectlDiscovery({
    required this.supported,
    required this.documentation,
    this.candidates = const [],
    this.methods = const [],
  });
  final bool supported;
  final String documentation;
  final List<String> candidates, methods;
  factory KubectlDiscovery.fromJson(KubeJson json) => KubectlDiscovery(
    supported: json['supported'] as bool,
    documentation: json['documentation'] as String,
    candidates: (json['candidates'] as List).map((c) => (c as KubeJson)['path'] as String).toList(),
    methods: (json['package_managers'] as List).cast<String>(),
  );
}
