import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/db/db_connection_dialog.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_providers.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_database_connect.dart';
import 'package:consolecrypt/kubernetes/kube_relations_service.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

const _session = KubeSession(id: 'live-1', contextId: 'ctx-prod', serverVersion: 'v1.32', production: true);
const _pods = KubeResourceKind(group: '', version: 'v1', resource: 'pods', kind: 'Pod', namespaced: true);
const _resource = KubeResourceRef(api: _pods, namespace: 'shop', name: 'orders-db-5d8f7c9b4-x2k');
const _pod = <String, dynamic>{
  'metadata': {
    'name': 'orders-db-5d8f7c9b4-x2k',
    'labels': {'pod-template-hash': '5d8f7c9b4'},
    'ownerReferences': [
      {'kind': 'ReplicaSet', 'name': 'orders-db-5d8f7c9b4', 'controller': true},
    ],
  },
};
const _target = KubeDbTarget(engine: DbEngine.postgresql, remotePort: 5432, database: 'orders', user: 'app');

class _Relations implements KubeRelationsService {
  _Relations(this.services);
  final List<String> services;
  final asked = <String>[];
  @override
  Future<KubeJson> relatedObjects(String sessionId, KubeResourceRef resource) async {
    asked.add('$sessionId ${resource.id}');
    return {
      'items': [
        for (final name in services)
          {
            'relation': 'service',
            'reference': {'namespace': 'shop', 'name': name},
          },
        {
          'relation': 'owner',
          'reference': {'namespace': 'shop', 'name': 'orders-db-5d8f7c9b4'},
        },
      ],
    };
  }
}

Future<ProviderContainer> _pump(
  WidgetTester tester, {
  KubeRelationsService? relations,
  List<DbConnection> saved = const [],
}) async {
  final backend = testBackend();
  addTearDown(backend.dispose);
  for (final connection in saved) {
    await backend.services.databases!.saveConnection(DbConnectionDraft(metadata: connection));
  }
  tester.view.physicalSize = const Size(1400, 900);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  final router = GoRouter(
    routes: [
      GoRoute(
        path: '/',
        builder: (_, _) => Scaffold(
          body: Consumer(
            builder: (context, ref, _) => TextButton(
              key: const ValueKey('connect'),
              onPressed: () => connectKubeDatabase(
                context,
                ref,
                session: _session,
                resource: _resource,
                object: _pod,
                target: _target,
                relations: relations,
              ),
              child: const Text('Connect'),
            ),
          ),
        ),
      ),
      GoRoute(
        path: AppRoutes.databases,
        builder: (_, _) => const Scaffold(body: SizedBox(key: ValueKey('databases-page'))),
      ),
    ],
  );
  addTearDown(router.dispose);
  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        appServicesProvider.overrideWithValue(backend.services),
        dbScopeProvider.overrideWithValue(const DbScope('profile', true)),
      ],
      child: MaterialApp.router(
        routerConfig: router,
        locale: const Locale('en'),
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        supportedLocales: AppLocalizations.supportedLocales,
      ),
    ),
  );
  await settle(tester);
  return ProviderScope.containerOf(tester.element(find.byKey(const ValueKey('connect'))));
}

DbConnection _template(WidgetTester tester) =>
    tester.widget<DbConnectionDialog>(find.byType(DbConnectionDialog)).template!;

void main() {
  testWidgets('the form keeps a Kubernetes route to the Service, not a local port', (tester) async {
    final relations = _Relations(['orders-db']);
    await _pump(tester, relations: relations);
    await tapKey(tester, 'connect');
    expect(relations.asked, ['live-1 ${_resource.id}']);
    final template = _template(tester);
    expect((template.host, template.port), ('127.0.0.1', 5432));
    expect(template.route.toJson(), {
      'kind': 'kubernetes',
      'context_id': 'ctx-prod',
      'namespace': 'shop',
      'target_kind': 'service',
      'target_name': 'orders-db',
    });
    expect((template.name, template.database, template.username), ('shop/orders-db', 'orders', 'app'));
    expect(template.production, isTrue);
    expect(template.tags, ['kubernetes']);
    // The route section is open on the workload; the loopback host is implied.
    expect(find.byKey(const ValueKey('db-field-host')), findsNothing);
    expect(
      tester.widget<TextFormField>(find.byKey(const ValueKey('db-field-kubeTarget'))).controller!.text,
      'orders-db',
    );
    expect(find.text(tester.element(find.byType(DbConnectionDialog)).l10n.dbContainerPort), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('without a single Service the Deployment is saved', (tester) async {
    await _pump(tester, relations: _Relations(['orders-rw', 'orders-ro']));
    await tapKey(tester, 'connect');
    final route = _template(tester).route;
    expect((route.targetKind, route.targetName), (DbKubeTargetKind.deployment, 'orders-db'));
    expect(tester.takeException(), isNull);
  });

  testWidgets('a saved connection with the same route target opens directly', (tester) async {
    const route = DbRoute(
      kind: DbRouteKind.kubernetes,
      reference: 'ctx-prod',
      namespace: 'shop',
      targetKind: DbKubeTargetKind.deployment,
      targetName: 'orders-db',
    );
    DbConnection saved(String id, DbRoute route) => DbConnection(
      id: id,
      name: id,
      engine: DbEngine.postgresql,
      host: '127.0.0.1',
      port: 5432,
      route: route,
      options: const DbDriverOptions(engine: DbEngine.postgresql),
    );
    final container = await _pump(
      tester,
      saved: [
        // Same port, other workload or a plain loopback connection: no match.
        saved('direct', const DbRoute()),
        saved(
          'other',
          const DbRoute(
            kind: DbRouteKind.kubernetes,
            reference: 'ctx-prod',
            namespace: 'shop',
            targetKind: DbKubeTargetKind.deployment,
            targetName: 'billing-db',
          ),
        ),
        saved('orders', route),
      ],
    );
    await tapKey(tester, 'connect');
    expect(find.byType(DbConnectionDialog), findsNothing);
    final id = container.read(dbOpenRequestProvider);
    final connections = await container.read(appServicesProvider).databases!.connections();
    expect(connections.firstWhere((c) => c.id == id).name, 'orders');
    expect(find.byKey(const ValueKey('databases-page')), findsOneWidget);
    expect(tester.takeException(), isNull);
  });
}
