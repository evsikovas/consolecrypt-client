import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/db/db_connection_dialog.dart';
import 'package:consolecrypt/db/db_connections_screen.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_providers.dart';
import 'package:consolecrypt/db/db_query_service.dart';
import 'package:consolecrypt/db/db_service.dart';
import 'package:consolecrypt/db/db_sqlite_dialect.dart';
import 'package:consolecrypt/db/db_tree.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

const _file = DbConnection(
  id: 'notes',
  name: 'notes',
  engine: DbEngine.sqlite,
  host: 'localhost',
  port: 0,
  database: '/data/notes.db',
  production: false,
  options: DbDriverOptions(engine: DbEngine.sqlite),
);

class _Service implements DbService, DbSqliteFileService {
  final saved = <DbConnectionDraft>[];
  final created = <String>[];
  @override
  Stream<void> changes() => const Stream.empty();
  @override
  Future<List<DbConnection>> connections() async => const [_file];
  @override
  Future<DbConnectionChoices> connectionChoices() async => const DbConnectionChoices(
    routes: [
      DbRouteChoice(
        route: DbRoute(kind: DbRouteKind.ssh, reference: 'host-1'),
        label: 'app-server',
      ),
      DbRouteChoice(
        route: DbRoute(kind: DbRouteKind.kubernetes, reference: 'context-1'),
        label: 'prod-cluster',
      ),
    ],
  );
  @override
  Future<DbConnection> saveConnection(DbConnectionDraft draft) async {
    saved.add(draft);
    return draft.metadata;
  }

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
  @override
  Future<String?> pickSqliteFile() async => '/home/me/shop.sqlite';
  @override
  Future<String?> createSqliteFile() async {
    created.add('/home/me/new.sqlite');
    return created.last;
  }
}

/// The structure a SQLite session reports (see sqlite_introspect.rs).
class _Tree implements DbQueryService {
  static DbObjectRef ref(DbObjectKind kind, String name, {String? parent}) =>
      DbObjectRef(database: 'main', kind: kind, name: name, parentName: parent);
  static DbTreeNode node(DbObjectKind kind, String name, {String? parent, String? detail, bool children = false}) =>
      DbTreeNode(
        reference: ref(kind, name, parent: parent),
        hasChildren: children,
        detail: detail,
      );

  @override
  Future<DbTreePage> tree(String sessionId, {DbObjectRef? parent, String? cursor, bool refresh = false}) async =>
      DbTreePage(
        nodes: switch (parent?.kind) {
          null => [node(DbObjectKind.database, 'shop.db', children: true)],
          DbObjectKind.database => [
            node(DbObjectKind.table, 'orders', children: true),
            node(DbObjectKind.view, 'big_orders', children: true),
            node(DbObjectKind.indexObject, 'orders_by_customer', parent: 'orders', detail: 'orders'),
            node(DbObjectKind.sequence, 'orders', detail: '2'),
            node(DbObjectKind.trigger, 'orders_touch', parent: 'orders', detail: 'orders · AFTER UPDATE'),
            for (final type in DbSqliteDialect.storageClasses) node(DbObjectKind.dataType, type),
          ],
          _ => [
            const DbTreeNode(
              reference: DbObjectRef(database: 'main', kind: DbObjectKind.column, name: 'id', parentName: 'orders'),
              hasChildren: false,
              detail: 'INTEGER',
              primaryKey: true,
            ),
            node(DbObjectKind.column, 'total', parent: 'orders', detail: 'REAL'),
            node(DbObjectKind.key, 'PRIMARY KEY', parent: 'orders', detail: '(id)'),
            node(DbObjectKind.foreignKey, 'customer_id', parent: 'orders', detail: '→ customers(id)'),
            node(DbObjectKind.indexObject, 'orders_by_customer', parent: 'orders'),
            node(DbObjectKind.reference, 'refunds', parent: 'orders'),
            node(DbObjectKind.trigger, 'orders_touch', parent: 'orders', detail: 'AFTER UPDATE'),
          ],
        },
      );

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

Future<void> _pump(WidgetTester tester, _Service service, Widget child) async {
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
        home: Scaffold(body: child),
      ),
    ),
  );
  await settle(tester);
}

Future<void> _openDialog(WidgetTester tester, _Service service) async {
  await _pump(
    tester,
    service,
    Builder(
      builder: (context) => TextButton(
        key: const ValueKey('open'),
        onPressed: () => showAppDialog<void>(context, secure: true, builder: (_) => const DbConnectionDialog()),
        child: const Text('Open'),
      ),
    ),
  );
  await tester.tap(find.byKey(const ValueKey('open')));
  await settle(tester);
}

Future<void> _choose(WidgetTester tester, String key, String label) async {
  await tester.ensureVisible(find.byKey(ValueKey(key)));
  await tester.tap(find.byKey(ValueKey(key)));
  await settle(tester);
  await tester.tap(find.text(label).last);
  await settle(tester);
}

AppLocalizations _l(WidgetTester tester) => tester.element(find.byType(DbConnectionDialog)).l10n;

void main() {
  testWidgets('a local SQLite file is read-only by default, picked from disk and kept on this device', (tester) async {
    final service = _Service();
    await _openDialog(tester, service);
    await enterKey(tester, 'db-field-name', 'shop');
    await _choose(tester, 'db-engine', 'SQLite');
    final l = _l(tester);
    // No server fields: a path, the location and the local-only note.
    expect(find.byKey(const ValueKey('db-field-host')), findsNothing);
    expect(find.byKey(const ValueKey('db-field-password')), findsNothing);
    expect(find.text(l.dbSqlitePath), findsOneWidget);
    expect(find.byKey(const ValueKey('db-sqlite-local-only')), findsOneWidget);
    final readOnly = tester.widget<CheckboxListTile>(find.byKey(const ValueKey('db-sqlite-read-only')));
    expect(readOnly.value, isTrue);
    // The path is required.
    await tapKey(tester, 'db-save');
    expect(service.saved, isEmpty);
    await tapKey(tester, 'db-sqlite-choose');
    await tapKey(tester, 'db-save');
    final saved = service.saved.single.metadata;
    expect(saved.engine, DbEngine.sqlite);
    expect(saved.database, '/home/me/shop.sqlite');
    expect(saved.host, 'localhost');
    expect(saved.port, 0);
    expect(saved.route.kind, DbRouteKind.direct);
    expect(saved.options.readOnly, isTrue);
    expect(saved.toJson()['options'], {'kind': 'sqlite', 'read_only': true});
    expect(service.saved.single.password, isNull);
  });

  testWidgets('creating a new database is explicit; a remote file names its SSH host', (tester) async {
    final service = _Service();
    await _openDialog(tester, service);
    await enterKey(tester, 'db-field-name', 'remote');
    await _choose(tester, 'db-engine', 'SQLite');
    final l = _l(tester);
    await tapKey(tester, 'db-sqlite-create');
    expect(service.created, ['/home/me/new.sqlite']);
    await _choose(tester, 'db-sqlite-location', l.dbSqliteSshHost);
    // Remote files have no file chooser; a host must be picked.
    expect(find.byKey(const ValueKey('db-sqlite-choose')), findsNothing);
    expect(find.byKey(const ValueKey('db-sqlite-remote-help')), findsOneWidget);
    await enterKey(tester, 'db-field-database', '/srv/app/data.db');
    await tapKey(tester, 'db-save');
    expect(service.saved, isEmpty);
    await _choose(tester, 'db-sqlite-host', 'app-server');
    await tester.ensureVisible(find.byKey(const ValueKey('db-sqlite-read-only')));
    await tapKey(tester, 'db-sqlite-read-only');
    await tapKey(tester, 'db-save');
    final saved = service.saved.single.metadata;
    expect(saved.route.kind, DbRouteKind.ssh);
    expect(saved.route.reference, 'host-1');
    expect(saved.database, '/srv/app/data.db');
    expect(saved.options.readOnly, isFalse);
  });

  testWidgets('saved SQLite connections show the path and "This device only", and cannot be shared', (tester) async {
    final service = _Service();
    await _pump(tester, service, DbConnectionsScreen(onOpen: (_) {}, onShare: (_) {}));
    expect(find.byKey(const ValueKey('db-local-only-notes')), findsOneWidget);
    expect(find.textContaining('/data/notes.db'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('db-menu-notes')));
    await settle(tester);
    final l = tester.element(find.byType(DbConnectionsScreen)).l10n;
    expect(find.text(l.sharingTitle), findsNothing);
    expect(find.text(l.commonConnect), findsWidgets);
  });

  test('unknown engines of newer versions are recognized and skipped', () {
    expect(DbEngine.tryParse('sqlite'), DbEngine.sqlite);
    expect(DbEngine.tryParse('cassandra'), isNull);
    expect(DbEngine.sqlite.localOnly, isTrue);
    expect(DbEngine.postgresql.localOnly, isFalse);
    final json = _file.toJson();
    expect(DbConnection.fromJson(json).options.readOnly, isTrue);
    expect(
      DbObjectRef.fromJson({'database': 'main', 'kind': 'foreign_key', 'name': 'a'}).kind,
      DbObjectKind.foreignKey,
    );
  });

  test('the SQLite dialect quotes, writes literals and names affinities', () {
    expect(DbSqliteDialect.quoteIdentifier('a"b'), '"a""b"');
    expect(DbSqliteDialect.literal("it's"), "'it''s'");
    expect(DbSqliteDialect.literal(r'a\b'), r"'a\b'");
    expect(DbSqliteDialect.literal(true), '1');
    expect(DbSqliteDialect.literal(null), 'NULL');
    expect(DbSqliteDialect.literal([0, 255]), "X'00FF'");
    expect(DbSqliteDialect.affinity('VARCHAR(20)'), 'TEXT');
    expect(DbSqliteDialect.affinity('BIGINT'), 'INTEGER');
    expect(DbSqliteDialect.affinity('DOUBLE PRECISION'), 'REAL');
    expect(DbSqliteDialect.affinity(''), 'BLOB');
    expect(DbSqliteDialect.affinity('DECIMAL(10,2)'), 'NUMERIC');
  });

  testWidgets('the SQLite tree groups a file and its tables into the folders of the classic navigators', (
    tester,
  ) async {
    const session = DbSessionInfo(
      id: 's',
      connectionId: 'notes',
      engine: DbEngine.sqlite,
      serverVersion: '3.50.4',
      production: false,
      readOnly: true,
      serverReadOnly: DbServerReadOnly.enforced,
    );
    await _pump(
      tester,
      _Service(),
      DbObjectTree(session: session, service: _Tree(), onDraft: (_) {}, onInsert: (_) {}, onPreview: (_, _) {}),
    );
    final l = tester.element(find.byType(DbObjectTree)).l10n;
    await tester.tap(find.text('shop.db'));
    await settle(tester);
    for (final folder in [
      l.dbFolderTables,
      l.dbFolderViews,
      l.dbFolderIndexes,
      l.dbFolderSequences,
      l.dbFolderTableTriggers,
      l.dbFolderDataTypes,
    ]) {
      expect(find.text(folder), findsOneWidget, reason: folder);
    }
    final order = [
      l.dbFolderTables,
      l.dbFolderViews,
      l.dbFolderIndexes,
      l.dbFolderSequences,
      l.dbFolderTableTriggers,
      l.dbFolderDataTypes,
    ].map((text) => tester.getTopLeft(find.text(text)).dy).toList();
    expect(order, [...order]..sort());
    await tester.tap(find.text('orders').first);
    await settle(tester);
    for (final folder in [
      l.dbFolderColumns,
      l.dbFolderKeys,
      l.dbFolderForeignKeys,
      l.dbFolderReferences,
      l.dbFolderTriggers,
    ]) {
      expect(find.text(folder), findsOneWidget, reason: folder);
    }
    // Columns read «name (TYPE)» with a type icon; the key keeps its icon.
    expect(find.text('total (REAL)'), findsOneWidget);
    expect(find.text('id (INTEGER)'), findsOneWidget);
    expect(find.byIcon(Icons.onetwothree_rounded), findsOneWidget);
    expect(find.byIcon(Icons.key), findsOneWidget);
  });
}
