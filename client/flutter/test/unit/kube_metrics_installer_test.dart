import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_manifest_service.dart';
import 'package:consolecrypt/kubernetes/kube_metrics_installer.dart';
import 'package:consolecrypt/l10n/app_localizations_en.dart';
import 'package:flutter_test/flutter_test.dart';

final class _Manifests implements KubeManifestService {
  final created = <String>[];
  final existing = <String>{'ServiceAccount'};
  @override
  Future<KubeResourceDetail> createResource(
    String sessionId,
    KubeResourceRef resource,
    SecretText manifest, {
    bool dryRun = true,
  }) async {
    if (existing.contains(resource.api.kind)) {
      throw const AppException(AppErrorCode.validation, 'exists', args: {'field': 'kubernetes', 'rule': 'conflict'});
    }
    created.add('${resource.api.kind}/${resource.name}');
    return KubeResourceDetail(object: const {}, yaml: manifest.expose());
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

KubeResourceKind _kind(String apiVersion, String kind, String resource, {bool namespaced = true}) {
  final parts = apiVersion.split('/');
  return KubeResourceKind(
    group: parts.length == 2 ? parts.first : '',
    version: parts.last,
    resource: resource,
    kind: kind,
    namespaced: namespaced,
    verbs: const ['create', 'list'],
  );
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  test('bundled metrics-server installs in order, skips existing objects, and adds insecure TLS', () async {
    final manifest = await MetricsServerManifest.load();
    final objects = manifest.build(insecureTls: true);
    final deployment = objects.singleWhere((o) => o['kind'] == 'Deployment');
    final containers = (((deployment['spec'] as Map)['template'] as Map)['spec'] as Map)['containers'] as List;
    final args = (containers.first as Map)['args'];
    expect(args, contains('--kubelet-insecure-tls'));
    expect(manifest.build(insecureTls: false).singleWhere((o) => o['kind'] == 'Deployment'), isNot(contains('--kubelet-insecure-tls')));
    expect(objects.last['kind'], 'APIService');
    final kinds = [
      _kind('v1', 'ServiceAccount', 'serviceaccounts'),
      _kind('rbac.authorization.k8s.io/v1', 'ClusterRole', 'clusterroles', namespaced: false),
      _kind('rbac.authorization.k8s.io/v1', 'RoleBinding', 'rolebindings'),
      _kind('rbac.authorization.k8s.io/v1', 'ClusterRoleBinding', 'clusterrolebindings', namespaced: false),
      _kind('v1', 'Service', 'services'),
      _kind('apps/v1', 'Deployment', 'deployments'),
      _kind('apiregistration.k8s.io/v1', 'APIService', 'apiservices', namespaced: false),
    ];
    final service = _Manifests();
    final AppLocalizations l = AppLocalizationsEn();
    const session = KubeSession(id: 's', contextId: 'c', serverVersion: 'v1.31', production: false);
    final result = await installMetricsServer(l10n: l, service: service, session: session, kinds: kinds, objects: objects);
    expect(result.error, isNull);
    expect(result.existing, 1);
    expect(result.created, objects.length - 1);
    expect(service.created.last, 'APIService/v1beta1.metrics.k8s.io');
    final missing = await installMetricsServer(
      l10n: l,
      service: _Manifests(),
      session: session,
      kinds: kinds.where((k) => k.kind != 'APIService').toList(),
      objects: objects,
    );
    expect(missing.error, contains('APIService'));
  });
}
