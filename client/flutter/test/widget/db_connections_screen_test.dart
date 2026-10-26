import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/db/db_connections_screen.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_providers.dart';
import 'package:consolecrypt/db/db_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

const _prod = DbConnection(
  id: 'prod',
  name: 'umami prod',
  engine: DbEngine.postgresql,
  host: '127.0.0.1',
  port: 12432,
  database: 'umami',
  color: '#E5484D',
  route: DbRoute(kind: DbRouteKind.ssh, reference: 'host'),
  options: DbDriverOptions(engine: DbEngine.postgresql),
);
const _cache = DbConnection(
  id: 'cache',
  name: 'cache',
  engine: DbEngine.redis,
  host: 'redis.local',
  port: 6379,
  production: false,
  tags: ['staging'],
  options: DbDriverOptions(engine: DbEngine.redis),
);

const _ring = DbConnection(
  id: 'ring',
  name: 'events ring',
  engine: DbEngine.cassandra,
  host: 'cass-1.internal',
  port: 9042,
  database: 'events',
  production: false,
  options: DbDriverOptions(engine: DbEngine.cassandra, contactPoints: ['cass-2.internal'], localDatacenter: 'dc1'),
  deviceOnly: true,
);

class _Service implements DbService {
  _Service([this.items = const [_prod, _cache]]);
  final List<DbConnection> items;
  @override
  Stream<void> changes() => const Stream.empty();
  @override
  Future<List<DbConnection>> connections() async => items;
  @override
  Future<DbConnectionChoices> connectionChoices() async => const DbConnectionChoices();
  @override
  Future<DbConnection> saveConnection(DbConnectionDraft draft) async => draft.metadata;
  @override
  Future<void> deleteConnection(String id) async {}
  @override
  Future<DbConnectionCheck> testConnection(DbConnectionDraft draft) async => throw UnimplementedError();
  @override
  Future<DbImportReview> importConnectionString(SecretText uri) async => throw UnimplementedError();
  @override
  Future<DbImportReview> importPgpass(SecretText contents) async => throw UnimplementedError();
  @override
  Future<void> confirmImport(String ticket, List<DbImportChoice> choices) async {}
  @override
  Future<void> discardImport(String ticket) async {}
}

Future<List<DbConnection>> _pump(WidgetTester tester, MockBackend backend, [_Service? service]) async {
  final opened = <DbConnection>[];
  tester.view.physicalSize = const Size(1100, 900);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        appServicesProvider.overrideWithValue(backend.services),
        dbServiceProvider.overrideWithValue(service ?? _Service()),
        dbScopeProvider.overrideWithValue(const DbScope('profile', true)),
      ],
      child: MaterialApp(
        locale: const Locale('en'),
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        supportedLocales: AppLocalizations.supportedLocales,
        home: Scaffold(body: DbConnectionsScreen(onOpen: opened.add)),
      ),
    ),
  );
  await settle(tester);
  return opened;
}

void main() {
  test('databases view preference round-trips through local settings JSON', () {
    const list = LocalSettings(databasesView: CollectionView.list);
    expect(localSettingsFromJson(localSettingsToJson(list)).databasesView, CollectionView.list);
    expect(localSettingsFromJson({'databases_view': 'unknown'}).databasesView, CollectionView.cards);
    expect(localSettingsFromJson({}).databasesView, CollectionView.cards);
  });

  testWidgets('connections render as compact cards with route hint and PROD badge', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    final opened = await _pump(tester, backend);
    expect(find.byKey(const ValueKey('db-connection-prod')), findsOneWidget);
    expect(find.text('PostgreSQL · 127.0.0.1:12432 · umami'), findsOneWidget);
    expect(find.text('Redis · redis.local:6379'), findsOneWidget);
    expect(find.widgetWithText(GlassBadge, 'PROD'), findsOneWidget);
    expect(find.byKey(const ValueKey('db-route-prod')), findsOneWidget);
    expect(find.byKey(const ValueKey('db-route-cache')), findsNothing);
    // Two cards share a row in the grid.
    expect(
      tester.getTopLeft(find.byKey(const ValueKey('db-connection-cache'))).dy,
      tester.getTopLeft(find.byKey(const ValueKey('db-connection-prod'))).dy,
    );
    await tapKey(tester, 'db-connection-prod');
    expect(opened.single.id, 'prod');

    await tapKey(tester, 'db-menu-cache');
    expect(find.text('Edit'), findsOneWidget);
    expect(find.text('Delete'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('view toggle switches to list rows and persists the choice', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await _pump(tester, backend);
    expect(tester.widget<GlassIconButton>(find.byKey(const ValueKey('db-view-cards'))).selected, isTrue);
    final cardHeight = tester.getSize(find.byKey(const ValueKey('db-connection-prod'))).height;

    await tapKey(tester, 'db-view-list');
    expect(tester.widget<GlassIconButton>(find.byKey(const ValueKey('db-view-list'))).selected, isTrue);
    expect(backend.services.settings.currentLocal.databasesView, CollectionView.list);
    final cache = tester.getTopLeft(find.byKey(const ValueKey('db-connection-cache')));
    final prod = tester.getTopLeft(find.byKey(const ValueKey('db-connection-prod')));
    expect(prod.dy, greaterThan(cache.dy), reason: 'list mode stacks one connection per row');
    expect(tester.getSize(find.byKey(const ValueKey('db-connection-prod'))).height, lessThan(cardHeight));

    // A fresh screen starts from the saved preference.
    await tester.pumpWidget(const SizedBox());
    await _pump(tester, backend);
    expect(tester.widget<GlassIconButton>(find.byKey(const ValueKey('db-view-list'))).selected, isTrue);

    await tapKey(tester, 'db-view-cards');
    expect(backend.services.settings.currentLocal.databasesView, CollectionView.cards);
    expect(tester.takeException(), isNull);
  });

  testWidgets('search and tag filters still narrow the connections', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await _pump(tester, backend);
    await enterKey(tester, 'db-search', 'redis');
    expect(find.byKey(const ValueKey('db-connection-cache')), findsOneWidget);
    expect(find.byKey(const ValueKey('db-connection-prod')), findsNothing);
    await enterKey(tester, 'db-search', '');
    await tester.tap(find.widgetWithText(FilterChip, 'staging'));
    await settle(tester);
    expect(find.byKey(const ValueKey('db-connection-prod')), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('Cassandra connections are marked "This device only" and open the CQL options', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await _pump(tester, backend, _Service(const [_ring]));
    expect(find.byKey(const ValueKey('db-local-only-ring')), findsOneWidget);
    expect(find.text('Cassandra / ScyllaDB · cass-1.internal:9042 · events'), findsOneWidget);
    await tapKey(tester, 'db-menu-ring');
    await tester.tap(find.text('Edit'));
    await settle(tester);
    expect(find.byKey(const ValueKey('db-local-only')), findsOneWidget);
    expect(find.text('Keyspace'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('a device-only Cassandra connection moves into the vault once all devices are up to date', (
    tester,
  ) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    final service = _MoveService();
    await _pump(tester, backend, service);
    final l = lookupAppLocalizations(const Locale('en'));
    // An old device on the account: the move waits.
    service.readiness = const DbSyncReadiness(
      ready: false,
      outdatedDevices: [('Old laptop', '0.9.1'), ('Phone', null)],
    );
    await tapKey(tester, 'db-menu-ring');
    await tester.tap(find.text(l.dbMoveToVault));
    await settle(tester);
    expect(find.byKey(const ValueKey('db-move-outdated')), findsOneWidget);
    expect(find.text('• Old laptop — 0.9.1'), findsOneWidget);
    expect(find.text('• Phone — ${l.dbMoveVersionUnknown}'), findsOneWidget);
    await tester.tap(find.text(l.commonClose));
    await settle(tester);
    expect(service.moved, isEmpty);

    service.readiness = const DbSyncReadiness(ready: true);
    await tapKey(tester, 'db-menu-ring');
    await tester.tap(find.text(l.dbMoveToVault));
    await settle(tester);
    await tester.tap(find.text(l.dbMoveToVaultConfirm));
    await settle(tester);
    expect(service.moved, ['ring']);
    expect(find.text(l.dbMovedToVault), findsOneWidget);
    expect(find.byKey(const ValueKey('db-local-only-ring')), findsNothing);
    // Once in the vault, there is nothing to move.
    await tapKey(tester, 'db-menu-ring');
    expect(find.text(l.dbMoveToVault), findsNothing);
    expect(tester.takeException(), isNull);
  });
}

/// Moves `ring` into the "vault" when asked.
class _MoveService extends _Service implements DbVaultMove {
  _MoveService() : super([_ring]);

  DbSyncReadiness readiness = const DbSyncReadiness(ready: true);
  final moved = <String>[];

  @override
  Future<List<DbConnection>> connections() async => [
    for (final c in items) moved.contains(c.id) ? DbConnection.fromJson(c.toJson()..['device_only'] = false) : c,
  ];

  @override
  Future<DbSyncReadiness> syncReadiness() async => readiness;

  @override
  Future<void> moveToVault(String connectionId) async => moved.add(connectionId);
}
