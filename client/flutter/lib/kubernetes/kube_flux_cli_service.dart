import 'dart:convert';
import 'dart:typed_data';

import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

abstract interface class KubeFluxCliService {
  Future<FluxCliDiscovery> discoverFlux({String? manualPath});
  Future<FluxCliVersion> probeFlux(String path);
  Future<FluxInstallReview> reviewFlux(String version, String method, String operation);

  /// Returns a terminal ID for package-manager operations, null for direct releases.
  Future<String?> approveFlux(FluxInstallReview review);
  Future<void> cancelFlux(String ticket);
  Future<FluxCliOutput> runFlux(String sessionId, String path, KubeJson action);
  Future<String> openFluxTerminal(String sessionId, String path);
}

final kubeFluxCliServiceProvider = Provider<KubeFluxCliService?>((ref) {
  final service = ref.watch(kubeServiceProvider);
  return service is KubeFluxCliService ? service as KubeFluxCliService : null;
});

final class FluxCliDiscovery {
  const FluxCliDiscovery({
    required this.supported,
    this.candidates = const [],
    this.methods = const [],
    this.cosign,
    this.destination,
  });
  final bool supported;
  final List<String> candidates, methods;
  final String? cosign, destination;
  factory FluxCliDiscovery.fromJson(KubeJson json) => FluxCliDiscovery(
    supported: json['supported'] as bool,
    candidates: (json['candidates'] as List? ?? []).map((e) => (e as KubeJson)['path'] as String).toList(),
    methods: (json['package_managers'] as List? ?? []).cast<String>(),
    cosign: json['cosign'] as String?,
    destination: json['destination'] as String?,
  );
}

final class FluxCliVersion {
  const FluxCliVersion({required this.path, required this.version, required this.sha256});
  final String path, version, sha256;
  factory FluxCliVersion.fromJson(KubeJson json) => FluxCliVersion(
    path: json['path'] as String,
    version: json['version'] as String,
    sha256: json['sha256'] as String,
  );
}

final class FluxInstallReview {
  const FluxInstallReview({
    required this.ticket,
    required this.method,
    required this.operation,
    required this.version,
    required this.source,
    required this.destination,
    this.executable,
    this.arguments = const [],
    this.verifier,
    this.sha256,
  });
  final String ticket, method, operation, version, source, destination;
  final String? executable, verifier, sha256;
  final List<String> arguments;
  factory FluxInstallReview.fromJson(KubeJson json) => FluxInstallReview(
    ticket: json['ticket'] as String,
    method: json['method'] as String,
    operation: json['operation'] as String,
    version: json['version'] as String,
    source: json['source'] as String,
    destination: json['destination'] as String,
    executable: json['executable'] as String?,
    arguments: (json['arguments'] as List? ?? []).cast<String>(),
    verifier: json['verifier'] as String?,
    sha256: json['executable_sha256'] as String?,
  );
}

final class FluxCliOutput {
  FluxCliOutput({required this.exitCode, required this.stdout, required this.stderr});
  final int exitCode;
  String stdout, stderr;
  factory FluxCliOutput.fromBytes(Uint8List bytes) {
    try {
      final json = jsonDecode(utf8.decode(bytes)) as KubeJson;
      return FluxCliOutput(
        exitCode: json['exit_code'] as int,
        stdout: json['stdout'] as String,
        stderr: json['stderr'] as String,
      );
    } finally {
      bytes.fillRange(0, bytes.length, 0);
    }
  }
  void dispose() {
    stdout = '';
    stderr = '';
  }
}
