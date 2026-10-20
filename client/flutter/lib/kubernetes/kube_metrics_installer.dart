import 'dart:convert';

import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_manifest_service.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:flutter/services.dart';

/// Bundled basic metrics-server (see assets/licenses/METRICS-SERVER-NOTICE.txt).
final class MetricsServerManifest {
  const MetricsServerManifest(this.version, this.objects);
  final String version;
  final List<KubeJson> objects;

  static Future<MetricsServerManifest> load() async {
    final json = jsonDecode(await rootBundle.loadString('assets/kubernetes/metrics-server.json')) as KubeJson;
    return MetricsServerManifest(json['version'] as String, (json['objects'] as List).cast<KubeJson>());
  }

  /// Objects to create; [insecureTls] adds `--kubelet-insecure-tls` for
  /// local clusters whose kubelets use self-signed certificates.
  List<KubeJson> build({required bool insecureTls}) => [
    for (final object in objects)
      if (insecureTls && object['kind'] == 'Deployment') _withInsecureTls(object) else object,
  ];

  static KubeJson _withInsecureTls(KubeJson deployment) {
    final copy = jsonDecode(jsonEncode(deployment)) as KubeJson;
    final containers = ((copy['spec'] as Map)['template'] as Map)['spec'] as Map;
    for (final container in (containers['containers'] as List).cast<Map<String, dynamic>>()) {
      (container['args'] as List).add('--kubelet-insecure-tls');
    }
    return copy;
  }
}

typedef MetricsInstallResult = ({int created, int existing, String? error});

/// Creates the objects one by one in a safe order (service account and
/// RBAC first, the APIService last). Existing objects are kept as they are.
Future<MetricsInstallResult> installMetricsServer({
  required AppLocalizations l10n,
  required KubeManifestService service,
  required KubeSession session,
  required List<KubeResourceKind> kinds,
  required List<KubeJson> objects,
}) async {
  var created = 0, existing = 0;
  for (final object in objects) {
    final apiVersion = object['apiVersion'] as String;
    final kindName = object['kind'] as String;
    final metadata = object['metadata'] as Map;
    final name = metadata['name'] as String;
    final kind = kinds.where((k) => k.apiVersion == apiVersion && k.kind == kindName).firstOrNull;
    if (kind == null) {
      return (created: created, existing: existing, error: l10n.kubeMetricsUnknownKind(kindName, apiVersion));
    }
    final manifest = SecretText(jsonEncode(object));
    try {
      await service.createResource(
        session.id,
        KubeResourceRef(api: kind, name: name, namespace: metadata['namespace'] as String?),
        manifest,
        dryRun: false,
      );
      created++;
    } on AppException catch (error) {
      final rule = error.args['rule'];
      if (rule == 'conflict') {
        existing++;
        continue;
      }
      final denied = rule == 'forbidden' || rule == 'unauthorized';
      return (
        created: created,
        existing: existing,
        error: denied ? l10n.kubeMetricsInstallDenied(kindName, name) : l10n.kubeMetricsInstallFailed(kindName, name),
      );
    } catch (_) {
      return (created: created, existing: existing, error: l10n.kubeMetricsInstallFailed(kindName, name));
    } finally {
      manifest.wipe();
    }
  }
  return (created: created, existing: existing, error: null);
}
