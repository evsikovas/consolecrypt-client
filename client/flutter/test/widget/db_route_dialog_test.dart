import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/mock/mock_databases.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/db/db_connection_dialog.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_providers.dart';
import 'package:consolecrypt/db/db_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

/// Mock databases with an SSH host, a Kubernetes context and a saved tunnel
/// (local port 15432 → db.internal:5432) to route through.
class _Databases implements DbService {
  final _mock = MockDbService();
  final saved = <DbConnection>[];
  @override
  Future<DbConnectionChoices> connectionChoices() async => const DbConnectionChoices(
    routes: [
      DbRouteChoice(
        route: DbRoute(kind: DbRouteKind.ssh, reference: 'ssh-web'),
        label: 'web-01',
      ),
      DbRouteChoice(
        route: DbRoute(kind: DbRouteKind.kubernetes, reference: 'ctx-prod'),
        label: 'prod',
      ),
      DbRouteChoice(
        route: DbRoute(kind: DbRouteKind.tunnel, reference: 't-orders'),
        label: 'orders-db',
        host: 'db.internal',
        port: 5432,
        localPort: 15432,
      ),
    ],
  );

  @override
  Future<DbConnection> saveConnection(DbConnectionDraft draft) async {
    final connection = await _mock.saveConnection(draft);
    saved.add(connection);
    return connection;
  }

  @override
  Stream<void> changes() => _mock.changes();
  @override
  Future<List<DbConnection>> connections() => _mock.connections();
  @override
  Future<void> deleteConnection(String id) => _mock.deleteConnection(id);
  @override
  Future<DbConnectionCheck> testConnection(DbConnectionDraft draft) => _mock.testConnection(draft);
  @override
  Future<DbImportReview> importConnectionString(SecretText uri) => _mock.importConnectionString(uri);
  @override
  Future<DbImportReview> importPgpass(SecretText contents) => _mock.importPgpass(contents);
  @override
  Future<void> confirmImport(String ticket, List<DbImportChoice> choices) => _mock.confirmImport(ticket, choices);
  @override
  Future<void> discardImport(String ticket) => _mock.discardImport(ticket);
}

Future<void> _pump(WidgetTester tester, _Databases service, {DbConnection? connection}) async {
  final backend = testBackend();
  addTearDown(backend.dispose);
  tester.view.physicalSize = const Size(1100, 1400);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        appServicesProvider.overrideWithValue(backend.services),
        dbServiceProvider.overrideWithValue(service),
        dbScopeProvider.overrideWithValue(const DbScope('profile', true)),
      ],
      child: MaterialApp(
        locale: const Locale('en'),
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        supportedLocales: AppLocalizations.supportedLocales,
        home: Scaffold(
          body: Builder(
            builder: (context) => TextButton(
              key: const ValueKey('open'),
              onPressed: () => showAppDialog<void>(
                context,
                secure: true,
                builder: (_) => DbConnectionDialog(connection: connection),
              ),
              child: const Text('Open'),
            ),
          ),
        ),
      ),
    ),
  );
  await tester.tap(find.byKey(const ValueKey('open')));
  await settle(tester);
}

Future<void> _openRoutes(WidgetTester tester, AppLocalizations l) async {
  if (find.byKey(const ValueKey('db-route')).evaluate().isNotEmpty) return;
  await tester.ensureVisible(find.text(l.dbRoute));
  await tester.tap(find.text(l.dbRoute));
  await settle(tester);
}

Future<void> _choose(WidgetTester tester, String key, String label) async {
  await tester.ensureVisible(find.byKey(ValueKey(key)));
  await tester.tap(find.byKey(ValueKey(key)));
  await settle(tester);
  await tester.tap(find.text(label).last);
  await settle(tester);
}

DbConnection _connection(DbRoute route, {String host = '127.0.0.1', int port = 5432}) => DbConnection(
  id: 'orders',
  name: 'orders',
  engine: DbEngine.postgresql,
  host: host,
  port: port,
  route: route,
  production: false,
  options: const DbDriverOptions(engine: DbEngine.postgresql),
);

AppLocalizations _l(WidgetTester tester) => tester.element(find.byType(DbConnectionDialog)).l10n;

void main() {
  testWidgets('a Kubernetes route is shown with its workload and saved as edited', (tester) async {
    final service = _Databases();
    await _pump(
      tester,
      service,
      connection: _connection(
        const DbRoute(
          kind: DbRouteKind.kubernetes,
          reference: 'ctx-prod',
          namespace: 'shop',
          targetKind: DbKubeTargetKind.service,
          targetName: 'orders-db',
        ),
      ),
    );
    final l = _l(tester);
    // The route section is open; the loopback host is implied.
    expect(find.byKey(const ValueKey('db-field-host')), findsNothing);
    expect(find.text(l.dbRouteKubernetes('prod')), findsOneWidget);
    await enterKey(tester, 'db-field-kubeTarget', 'Orders DB');
    await tapKey(tester, 'db-save');
    expect(service.saved, isEmpty, reason: 'invalid object names are refused');
    await enterKey(tester, 'db-field-kubeTarget', 'orders-0');
    await _choose(tester, 'db-kube-kind', DbKubeTargetKind.pod.label);
    await tapKey(tester, 'db-save');
    expect(service.saved.single.route.toJson(), {
      'kind': 'kubernetes',
      'context_id': 'ctx-prod',
      'namespace': 'shop',
      'target_kind': 'pod',
      'target_name': 'orders-0',
    });
    expect(service.saved.single.host, '127.0.0.1');
    expect(tester.takeException(), isNull);
  });

  testWidgets('switching a Kubernetes route to direct brings the host back', (tester) async {
    final service = _Databases();
    await _pump(
      tester,
      service,
      connection: _connection(
        const DbRoute(kind: DbRouteKind.kubernetes, reference: 'ctx-prod', namespace: 'shop', targetName: 'pg-0'),
      ),
    );
    await _choose(tester, 'db-route', _l(tester).dbDirect);
    expect(find.byKey(const ValueKey('db-field-host')), findsOneWidget);
    expect(find.byKey(const ValueKey('db-field-kubeTarget')), findsNothing);
    await tapKey(tester, 'db-save');
    expect(service.saved.single.route.kind, DbRouteKind.direct);
    expect(tester.takeException(), isNull);
  });

  testWidgets('a loopback address matching a saved tunnel offers its route', (tester) async {
    final service = _Databases();
    await _pump(tester, service);
    final l = _l(tester);
    await enterKey(tester, 'db-field-name', 'orders');
    await enterKey(tester, 'db-field-host', '127.0.0.1');
    await enterKey(tester, 'db-field-port', '15433');
    expect(find.byKey(const ValueKey('db-use-tunnel')), findsNothing);
    await enterKey(tester, 'db-field-port', '15432');
    expect(find.text(l.dbUseTunnel('orders-db')), findsOneWidget);
    await tapKey(tester, 'db-use-tunnel');
    // Address and port now come from the tunnel target.
    expect(find.byKey(const ValueKey('db-field-host')), findsNothing);
    expect(find.byKey(const ValueKey('db-field-port')), findsNothing);
    await _openRoutes(tester, l);
    expect(find.text(l.dbTunnelRouteHelp), findsOneWidget);
    await tapKey(tester, 'db-save');
    final saved = service.saved.single;
    expect(saved.route.toJson(), {'kind': 'tunnel', 'tunnel_id': 't-orders'});
    expect((saved.host, saved.port), ('db.internal', 5432));
    expect(tester.takeException(), isNull);
  });

  testWidgets('a Docker route picks the engine and the container by name', (tester) async {
    final service = _Databases();
    await _pump(tester, service);
    final l = _l(tester);
    await enterKey(tester, 'db-field-name', 'shop');
    await _openRoutes(tester, l);
    await _choose(tester, 'db-route', l.dbRouteDocker);
    expect(find.byKey(const ValueKey('db-field-host')), findsNothing);
    expect(find.text(l.dbContainerPort), findsOneWidget);
    await _choose(tester, 'db-docker-engine', 'web-01');
    await enterKey(tester, 'db-field-dockerContainer', 'shop-postgres-1');
    await tapKey(tester, 'db-save');
    final saved = service.saved.single;
    expect(saved.route.toJson(), {'kind': 'docker', 'host_id': 'ssh-web', 'container': 'shop-postgres-1'});
    expect((saved.host, saved.port, saved.tls.mode), ('127.0.0.1', 5432, DbTlsMode.prefer));
    expect(tester.takeException(), isNull);
  });
}
