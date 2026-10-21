import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/mock/mock_docker.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/db/db_connection_dialog.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_providers.dart';
import 'package:consolecrypt/docker/docker_container_list.dart';
import 'package:consolecrypt/docker/docker_providers.dart';
import 'package:consolecrypt/docker/docker_screen.dart';
import 'package:consolecrypt/docker/docker_service.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

/// Records forwards; [published] maps container names to a published
/// host port for 5432.
class _Docker extends MockDockerService {
  _Docker({this.published = const {}});
  final Map<String, int> published;
  final forwardsStarted = <({String containerId, int remotePort, int localPort})>[];

  @override
  Future<List<DockerContainer>> containers(String sessionId, {bool all = true}) async => [
    for (final c in await super.containers(sessionId, all: all))
      if (published[c.name] case final port?)
        DockerContainer(
          id: c.id,
          name: c.name,
          image: c.image,
          state: c.state,
          status: c.status,
          ports: [DockerPort(privatePort: 5432, publicPort: port, protocol: 'tcp', ip: '0.0.0.0')],
          composeProject: c.composeProject,
          composeService: c.composeService,
          networks: c.networks,
        )
      else
        c,
  ];

  @override
  Future<DockerForward> startForward(
    String sessionId,
    String containerId, {
    required int remotePort,
    int localPort = 0,
  }) {
    forwardsStarted.add((containerId: containerId, remotePort: remotePort, localPort: localPort));
    return super.startForward(sessionId, containerId, remotePort: remotePort, localPort: localPort);
  }
}

Future<ProviderContainer> _pump(WidgetTester tester, _Docker service, {List<DbConnection> saved = const []}) async {
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
        builder: (_, _) => const Scaffold(body: DockerScreen()),
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
        dockerServiceProvider.overrideWithValue(service),
        dockerScopeProvider.overrideWithValue(const DockerScope('profile', true)),
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
  return ProviderScope.containerOf(tester.element(find.byType(DockerScreen)));
}

Future<void> _connect(WidgetTester tester, String hostId) async {
  await tester.tap(find.byKey(ValueKey('docker-connect-$hostId')));
  await settle(tester);
}

String _id(WidgetTester tester, String name) => tester
    .widget<DockerContainerRow>(find.ancestor(of: find.text(name), matching: find.byType(DockerContainerRow)).first)
    .container
    .id;

DbConnection _template(WidgetTester tester) =>
    tester.widget<DbConnectionDialog>(find.byType(DbConnectionDialog)).template!;

void main() {
  testWidgets('the button is shown for database ports only, in the row, menu and details', (tester) async {
    await _pump(tester, _Docker());
    await _connect(tester, DockerHost.localId);
    final postgres = _id(tester, 'shop-postgres-1');
    final redis = _id(tester, 'shop-redis-1');
    final web = _id(tester, 'shop-web-1');
    expect(find.byKey(ValueKey('docker-database-$postgres')), findsOneWidget);
    expect(find.byKey(ValueKey('docker-database-$redis')), findsOneWidget);
    expect(find.byKey(ValueKey('docker-database-$web')), findsNothing);
    expect(find.byKey(ValueKey('docker-database-${_id(tester, 'shop-api-1')}')), findsNothing);

    await tapKey(tester, 'docker-menu-button-$web');
    expect(find.byKey(const ValueKey('docker-menu-database')), findsNothing);
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await settle(tester);
    await tapKey(tester, 'docker-menu-button-$postgres');
    expect(find.byKey(const ValueKey('docker-menu-database')), findsOneWidget);
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await settle(tester);

    await tester.tap(find.byKey(ValueKey('docker-container-$postgres')));
    await settle(tester);
    expect(find.byKey(const ValueKey('docker-details-database')), findsOneWidget);
    await tester.tap(find.byKey(ValueKey('docker-container-$web')));
    await settle(tester);
    expect(find.byKey(const ValueKey('docker-details-forward')), findsOneWidget);
    expect(find.byKey(const ValueKey('docker-details-database')), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('local, unpublished: the form keeps a Docker route restored on connect', (tester) async {
    final service = _Docker();
    await _pump(tester, service);
    await _connect(tester, DockerHost.localId);
    final postgres = _id(tester, 'shop-postgres-1');
    await tapKey(tester, 'docker-database-$postgres');
    // No temporary forward: the core starts (or reuses) it on every connect.
    expect(service.forwardsStarted, isEmpty);
    final template = _template(tester);
    expect(template.name, 'shop-postgres-1');
    expect(template.engine, DbEngine.postgresql);
    expect(template.host, '127.0.0.1');
    expect(template.port, 5432);
    expect(template.route.kind, DbRouteKind.docker);
    expect(template.route.reference, isNull);
    expect(template.route.container, 'shop-postgres-1');
    expect(template.route.toJson(), {'kind': 'docker', 'host_id': null, 'container': 'shop-postgres-1'});
    // The route section shows the container, not a host field.
    expect(find.byKey(const ValueKey('db-field-host')), findsNothing);
    expect(find.byKey(const ValueKey('db-field-dockerContainer')), findsOneWidget);
    expect(template.tags, ['docker']);
    expect(template.production, isFalse);
    expect(tester.takeException(), isNull);
  });

  testWidgets('local, published: the host port is used and a saved match opens in Databases', (tester) async {
    final service = _Docker(published: {'shop-postgres-1': 15433});
    const saved = DbConnection(
      id: 'docker-pg',
      name: 'shop-postgres-1',
      engine: DbEngine.postgresql,
      host: '127.0.0.1',
      port: 15433,
      production: false,
      options: DbDriverOptions(engine: DbEngine.postgresql),
    );
    final container = await _pump(tester, service, saved: [saved]);
    await _connect(tester, DockerHost.localId);
    final postgres = _id(tester, 'shop-postgres-1');
    await tester.tap(find.byKey(ValueKey('docker-container-$postgres')));
    await settle(tester);
    await tapKey(tester, 'docker-details-database');
    expect(service.forwardsStarted, isEmpty);
    expect(find.byType(DbConnectionDialog), findsNothing);
    expect(container.read(dbOpenRequestProvider), 'docker-pg');
    expect(find.byKey(const ValueKey('databases-page')), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('over SSH the connection is routed through the saved host', (tester) async {
    final service = _Docker(published: {'shop-postgres-1': 15433});
    await _pump(tester, service);
    await _connect(tester, 'ssh-web');
    // Published: the host port on the Docker host's loopback.
    await tapKey(tester, 'docker-database-${_id(tester, 'shop-postgres-1')}');
    var template = _template(tester);
    expect((template.host, template.port), ('127.0.0.1', 15433));
    expect((template.route.kind, template.route.reference), (DbRouteKind.ssh, 'ssh-web'));
    expect(template.production, isTrue);
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await settle(tester);
    expect(find.byType(DbConnectionDialog), findsNothing);

    // Unpublished: the container IP on its Docker network.
    await tapKey(tester, 'docker-database-${_id(tester, 'shop-redis-1')}');
    template = _template(tester);
    expect(template.engine, DbEngine.redis);
    expect((template.host, template.port), ('172.18.0.3', 6379));
    expect((template.route.kind, template.route.reference), (DbRouteKind.ssh, 'ssh-web'));
    expect(service.forwardsStarted, isEmpty);
    expect(tester.takeException(), isNull);
  });
}
