import 'dart:async';

import 'package:consolecrypt/core/mock/mock_cloud.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/db/db_ai_service.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_query_service.dart';
import 'package:consolecrypt/db/db_service.dart';

// Demo data of the mock backend; nothing here is shown in release builds.
// l10n-ignore-file

/// In-memory databases for the demo/mock backend: one connection per engine,
/// a small shop schema, results, history and a canned assistant answer.
final class MockDbService
    implements
        DbService,
        DbQueryService,
        DbHistoryService,
        DbExportService,
        DbAiService,
        DbProductionWriteService,
        DbCellEditService,
        DbObjectInspectService {
  final _changes = StreamController<void>.broadcast();
  final _history = <DbHistoryEntry>[];
  var _next = 0;

  late final List<DbConnection> _connections = [
    _connection(
      'pg',
      'shop-prod',
      DbEngine.postgresql,
      'db.shop.internal',
      production: true,
      database: 'shop',
      route: const DbRoute(kind: DbRouteKind.ssh, reference: _bastion),
      color: '#E5484D',
    ),
    _connection('pg-dev', 'shop-dev', DbEngine.postgresql, 'localhost', production: false, database: 'shop'),
    _connection(
      'my',
      'billing',
      DbEngine.mysql,
      'billing.internal',
      production: true,
      database: 'billing',
      route: const DbRoute(kind: DbRouteKind.docker, reference: _dockerHost, container: 'billing-mysql-1'),
      color: '#F5A524',
    ),
    _connection(
      'ch',
      'analytics',
      DbEngine.clickhouse,
      'ch.internal',
      production: true,
      database: 'events',
      route: const DbRoute(
        kind: DbRouteKind.kubernetes,
        reference: _cluster,
        namespace: 'analytics',
        targetKind: DbKubeTargetKind.statefulSet,
        targetName: 'clickhouse',
      ),
      color: '#8E4EC6',
    ),
    _connection(
      'redis',
      'cache',
      DbEngine.redis,
      'redis.internal',
      production: false,
      database: '0',
      route: const DbRoute(kind: DbRouteKind.tunnel, reference: _redisTunnel),
    ),
    _connection(
      'mongo',
      'catalog',
      DbEngine.mongodb,
      'mongo.internal',
      production: false,
      database: 'catalog',
      route: const DbRoute(kind: DbRouteKind.jump, reference: _jump),
      color: '#30A46C',
    ),
  ];

  // Route targets offered by the connection form (demo names only).
  static const _bastion = 'mock-host-bastion-a';
  static const _dockerHost = 'mock-host-web-01';
  static const _jump = 'mock-jump-bastion';
  static const _cluster = 'mock-kube-demo';
  static const _redisTunnel = 'mock-tunnel-redis';

  static DbConnection _connection(
    String id,
    String name,
    DbEngine engine,
    String host, {
    required bool production,
    required String database,
    DbRoute route = const DbRoute(),
    String? color,
  }) => DbConnection(
    id: id,
    name: name,
    engine: engine,
    host: host,
    port: engine.port,
    database: database,
    username: 'app',
    // A saved (demo) password: the workspace opens without asking.
    auth: const DbAuth(passwordSecretId: 'mock-db-password'),
    route: route,
    production: production,
    options: DbDriverOptions(engine: engine),
    tags: [if (production) 'prod' else 'dev'],
    color: color,
  );

  DbConnection _byId(String id) => _connections.firstWhere((c) => c.id == id);
  final _sessions = <String, DbSessionInfo>{};

  // Connections.

  @override
  Stream<void> changes() => _changes.stream;

  @override
  Future<List<DbConnection>> connections() async => List.unmodifiable(_connections);

  @override
  Future<DbConnectionChoices> connectionChoices() async => const DbConnectionChoices(
    routes: [
      DbRouteChoice(
        route: DbRoute(kind: DbRouteKind.ssh, reference: _bastion),
        label: 'bastion-a',
      ),
      DbRouteChoice(
        route: DbRoute(kind: DbRouteKind.ssh, reference: _dockerHost),
        label: 'web-01',
      ),
      DbRouteChoice(
        route: DbRoute(kind: DbRouteKind.jump, reference: _jump),
        label: 'bastion-chain',
      ),
      DbRouteChoice(
        route: DbRoute(kind: DbRouteKind.kubernetes, reference: _cluster),
        label: 'demo-cluster',
      ),
      DbRouteChoice(
        route: DbRoute(kind: DbRouteKind.tunnel, reference: _redisTunnel),
        label: 'redis-cache',
        host: 'redis.internal',
        port: 6379,
        localPort: 16379,
      ),
    ],
  );

  @override
  Future<DbConnection> saveConnection(DbConnectionDraft draft) async {
    final json = draft.metadata.toJson();
    if ((json['id'] as String? ?? '').isEmpty) json['id'] = 'mock-${_next++}';
    final saved = DbConnection.fromJson(json);
    _connections
      ..removeWhere((c) => c.id == saved.id)
      ..add(saved);
    draft.wipe();
    _changes.add(null);
    return saved;
  }

  @override
  Future<void> deleteConnection(String id) async {
    _connections.removeWhere((c) => c.id == id);
    _changes.add(null);
  }

  @override
  Future<DbConnectionCheck> testConnection(DbConnectionDraft draft) async {
    draft.wipe();
    return const DbConnectionCheck(serverVersion: '17.2', serverReadOnly: DbServerReadOnly.enforced);
  }

  @override
  Future<DbImportReview> importConnectionString(SecretText uri) async {
    uri.wipe();
    return DbImportReview(
      ticket: 'import',
      expiresAt: DateTime.now().add(const Duration(minutes: 10)),
      entries: const [
        DbImportEntry(index: 0, name: 'orders', engine: DbEngine.postgresql, host: 'orders.internal', port: 5432),
      ],
    );
  }

  @override
  Future<DbImportReview> importPgpass(SecretText contents) => importConnectionString(contents);

  @override
  Future<void> confirmImport(String ticket, List<DbImportChoice> choices) async {}

  @override
  Future<void> discardImport(String ticket) async {}

  // Sessions.

  @override
  Future<DbSessionInfo> connect(String connectionId, {SecretText? temporaryPassword}) async {
    temporaryPassword?.wipe();
    final connection = _byId(connectionId);
    final session = DbSessionInfo(
      id: 'session-$connectionId',
      connectionId: connectionId,
      engine: connection.engine,
      serverVersion: switch (connection.engine) {
        DbEngine.postgresql => '17.2',
        DbEngine.mysql || DbEngine.mariadb => '8.4.3',
        DbEngine.clickhouse => '25.8.2',
        DbEngine.redis => '7.4.1',
        DbEngine.mongodb => '7.0.14',
      },
      production: connection.production,
      readOnly: connection.production,
      serverReadOnly: connection.production ? DbServerReadOnly.enforced : DbServerReadOnly.unverified,
      transportSecurity: DbTransportSecurity.expected(connection, loopback: false) ?? DbTransportSecurity.tunnel,
      canUnlockProduction: connection.production,
    );
    return _sessions[session.id] = session;
  }

  @override
  Future<void> disconnect(String sessionId) async => _sessions.remove(sessionId);

  @override
  Future<DbPrivilegeReport> checkPrivileges(String sessionId) async => DbPrivilegeReport(
    serverReadOnly: DbServerReadOnly.enforced,
    // The demo production role may write only while its window is open.
    hasWritePrivileges: _sessions[sessionId]?.productionWriteOpen ?? false,
  );

  DbSessionInfo _write(String sessionId, bool allow) {
    final s = _sessions[sessionId]!;
    return _sessions[sessionId] = DbSessionInfo(
      id: s.id,
      connectionId: s.connectionId,
      engine: s.engine,
      serverVersion: s.serverVersion,
      production: s.production,
      readOnly: s.production || !allow,
      serverReadOnly: s.serverReadOnly,
      transportSecurity: s.transportSecurity,
      canUnlockProduction: s.canUnlockProduction,
    );
  }

  DbSessionInfo _window(String sessionId, DateTime? until) {
    final s = _sessions[sessionId]!;
    return _sessions[sessionId] = DbSessionInfo(
      id: s.id,
      connectionId: s.connectionId,
      engine: s.engine,
      serverVersion: s.serverVersion,
      production: s.production,
      readOnly: until == null,
      serverReadOnly: s.serverReadOnly,
      transportSecurity: s.transportSecurity,
      canUnlockProduction: s.canUnlockProduction,
      productionWriteUntil: until,
    );
  }

  @override
  Future<DbSessionInfo> sessionState(String sessionId) async {
    final s = _sessions[sessionId]!;
    return s.production && !s.productionWriteOpen && !s.readOnly ? _window(sessionId, null) : s;
  }

  @override
  Future<DbSessionInfo> unlockProduction(
    String sessionId, {
    required SecretText passphrase,
    required int minutes,
  }) async {
    final s = _sessions[sessionId]!;
    final ok = passphrase.expose() == MockCloud.demoPassphrase;
    passphrase.wipe();
    if (!ok) throw const DbWrongPassphrase();
    if (!s.production || ![5, 15, 30].contains(minutes)) {
      throw StateError('Production unlock refused');
    }
    return _window(sessionId, DateTime.now().add(Duration(minutes: minutes)));
  }

  @override
  Future<DbSessionInfo> lockProduction(String sessionId) async => _window(sessionId, null);

  // In-grid edits.

  static String _literal(DbCell cell) => switch (cell.kind) {
    DbCellKind.nullValue => 'NULL',
    DbCellKind.intValue || DbCellKind.decimal => cell.text,
    _ => "'${cell.text.replaceAll("'", "''")}'",
  };

  @override
  Future<DbCellEditReview> reviewCellEdits(
    String sessionId,
    DbObjectRef table,
    List<DbCellEdit> edits,
  ) async => DbCellEditReview(
    ticket: 'edits-${_next++}',
    sql: SecretText(
      edits
          .map(
            (e) =>
                'UPDATE "${table.schema ?? 'public'}"."${table.name}" SET "${e.column}" = ${_literal(e.value)} '
                'WHERE ${e.originalPrimaryKey.entries.map((k) => '"${k.key}" = ${_literal(k.value)}').join(' AND ')};',
          )
          .join('\n'),
    ),
    expiresAt: DateTime.now().add(const Duration(minutes: 5)),
    rows: edits.length,
  );

  @override
  Future<void> commitCellEdits(String sessionId, String ticket) async {}

  @override
  Future<void> discardCellEdits(String sessionId, String ticket) async {}

  @override
  Future<DbSessionInfo> allowSessionWrite(String sessionId) async => _write(sessionId, true);

  @override
  Future<DbSessionInfo> revokeSessionWrite(String sessionId) async => _write(sessionId, false);

  static const _tables = {
    'customers': [
      ('id', 'bigint', true),
      ('email', 'text', false),
      ('name', 'text', false),
      ('created_at', 'timestamptz', false),
    ],
    'orders': [
      ('id', 'bigint', true),
      ('customer_id', 'bigint', false),
      ('total', 'numeric(12,2)', false),
      ('status', 'text', false),
      ('created_at', 'timestamptz', false),
    ],
    'order_items': [
      ('id', 'bigint', true),
      ('order_id', 'bigint', false),
      ('sku', 'text', false),
      ('quantity', 'integer', false),
    ],
    'products': [('sku', 'text', true), ('title', 'text', false), ('price', 'numeric(10,2)', false)],
  };

  @override
  Future<DbTreePage> tree(String sessionId, {DbObjectRef? parent, String? cursor, bool refresh = false}) async {
    final engine = _sessions[sessionId]?.engine ?? DbEngine.postgresql;
    DbTreeNode node(DbObjectRef ref, {bool children = true, String? detail, bool pk = false, DbObjectRef? fk}) =>
        DbTreeNode(reference: ref, hasChildren: children, detail: detail, primaryKey: pk, foreignKey: fk);
    if (engine == DbEngine.redis) {
      if (parent == null) {
        return DbTreePage(
          nodes: [
            for (var i = 0; i < 4; i++)
              node(
                DbObjectRef(database: '$i', kind: DbObjectKind.redisDatabase, name: 'db$i'),
                detail: '${[1284, 12, 0, 3][i]} keys',
              ),
          ],
        );
      }
      return DbTreePage(
        nodes: [
          node(
            DbObjectRef(database: parent.database, kind: DbObjectKind.redisDatabase, name: 'session:', parentName: ''),
            detail: '1 204',
          ),
          node(
            DbObjectRef(database: parent.database, kind: DbObjectKind.redisDatabase, name: 'rate:', parentName: ''),
            detail: '77',
          ),
          node(
            DbObjectRef(database: parent.database, kind: DbObjectKind.redisKey, name: 'config:flags', parentName: ''),
            children: false,
            detail: 'hash · no TTL',
          ),
          node(
            DbObjectRef(database: parent.database, kind: DbObjectKind.redisKey, name: 'queue:mail', parentName: ''),
            children: false,
            detail: 'list · no TTL',
          ),
        ],
      );
    }
    if (engine == DbEngine.mongodb) {
      if (parent == null) {
        return DbTreePage(
          nodes: [node(const DbObjectRef(database: 'catalog', kind: DbObjectKind.database, name: 'catalog'))],
        );
      }
      if (parent.kind == DbObjectKind.database) {
        return DbTreePage(
          nodes: [
            for (final c in ['products', 'reviews', 'categories'])
              node(
                DbObjectRef(database: 'catalog', kind: DbObjectKind.collection, name: c),
                detail: 'collection',
              ),
          ],
        );
      }
      return DbTreePage(
        nodes: [
          node(
            DbObjectRef(database: 'catalog', kind: DbObjectKind.indexObject, name: '_id_', parentName: parent.name),
            children: false,
            detail: '{ _id: 1 } unique',
          ),
          node(
            DbObjectRef(database: 'catalog', kind: DbObjectKind.indexObject, name: 'sku_1', parentName: parent.name),
            children: false,
            detail: '{ sku: 1 } unique',
          ),
        ],
      );
    }
    if (parent == null) {
      return DbTreePage(
        nodes: [
          for (final schema in ['public', 'analytics'])
            node(DbObjectRef(database: 'shop', schema: schema, kind: DbObjectKind.schema, name: schema)),
        ],
      );
    }
    if (parent.kind == DbObjectKind.schema || parent.kind == DbObjectKind.database) {
      return DbTreePage(
        nodes: [
          for (final table in _tables.keys)
            node(DbObjectRef(database: 'shop', schema: parent.name, kind: DbObjectKind.table, name: table)),
          node(DbObjectRef(database: 'shop', schema: parent.name, kind: DbObjectKind.view, name: 'daily_revenue')),
          node(
            DbObjectRef(database: 'shop', schema: parent.name, kind: DbObjectKind.function, name: 'order_total'),
            children: false,
          ),
        ],
      );
    }
    final columns = _tables[parent.name] ?? const [];
    return DbTreePage(
      nodes: [
        for (final (name, type, pk) in columns)
          node(
            DbObjectRef(
              database: 'shop',
              schema: parent.schema,
              kind: DbObjectKind.column,
              name: name,
              parentName: parent.name,
            ),
            children: false,
            detail: type,
            pk: pk,
            fk: name.endsWith('_id') && name != 'id'
                ? DbObjectRef(
                    database: 'shop',
                    schema: 'public',
                    kind: DbObjectKind.table,
                    name: '${name.replaceAll('_id', '')}s',
                  )
                : null,
          ),
        node(
          DbObjectRef(
            database: 'shop',
            schema: parent.schema,
            kind: DbObjectKind.indexObject,
            name: '${parent.name}_pkey',
            parentName: parent.name,
          ),
          children: false,
          detail: 'CREATE UNIQUE INDEX ${parent.name}_pkey ON public.${parent.name} USING btree (id)',
        ),
      ],
    );
  }

  @override
  Future<SecretText> ddl(String sessionId, DbObjectRef object) async {
    final name = object.name;
    final columns = _tables[name] ?? const [];
    final keys = [
      for (final c in columns)
        if (c.$1.endsWith('_id')) c.$1,
    ];
    final indexed = [if (columns.any((c) => c.$1 == 'created_at')) 'created_at', ...keys];
    final lines = [
      for (final (column, type, primary) in columns)
        '    $column $type${primary || column.endsWith('_id') ? ' NOT NULL' : ''}${column == 'created_at' ? ' DEFAULT now()' : ''}',
      '    CONSTRAINT ${name}_pkey PRIMARY KEY (${columns.firstWhere((c) => c.$3).$1})',
      for (final key in keys)
        '    CONSTRAINT ${name}_${key}_fkey\n        FOREIGN KEY ($key)\n        REFERENCES public.${key.replaceAll('_id', '')}s (id)',
    ];
    return SecretText(
      'CREATE TABLE "public"."$name" (\n${lines.join(',\n')}\n);\n\n'
      '${indexed.map((c) => 'CREATE INDEX ${name}_${c}_idx\n    ON public.$name ($c${c == 'created_at' ? ' DESC' : ''});\n').join()}'
      "\nCOMMENT ON TABLE public.$name IS 'Demo shop: $name';",
    );
  }

  /// Object details in the core's section format (PostgreSQL layout) with
  /// fixed demo figures.
  @override
  Future<DbJson> inspectObject(String sessionId, DbObjectRef object, {int? sampleDocuments}) async {
    final columns = _tables[object.name]?.length ?? 0;
    Map<String, dynamic> field(String id, String value, String kind) => {'id': id, 'value': value, 'kind': kind};
    return {
      'sections': [
        {
          'id': 'general',
          'fields': [
            field('kind', 'table', 'keyword'),
            field('schema', object.schema ?? 'public', 'code'),
            field('owner', 'shop_owner', 'code'),
            field('comment', 'Demo shop: ${object.name}', 'text'),
          ],
        },
        {
          'id': 'size',
          'fields': [
            field('rows', '184230', 'estimate'),
            field('live_rows', '184102', 'estimate'),
            field('dead_rows', '1287', 'estimate'),
            field('total_size', '48627712', 'bytes'),
            field('table_size', '31457280', 'bytes'),
            field('index_size', '16777216', 'bytes'),
          ],
        },
        {
          'id': 'structure',
          'fields': [
            field('columns', '$columns', 'count'),
            field('indexes', '3', 'count'),
            field('constraints', '2', 'count'),
            field('triggers', '0', 'count'),
          ],
        },
        {
          'id': 'maintenance',
          'fields': [
            field('last_autovacuum', '2026-10-22T03:14:00Z', 'time'),
            field('last_autoanalyze', '2026-10-22T03:15:00Z', 'time'),
          ],
        },
      ],
    };
  }

  @override
  Future<String> identifier(String sessionId, DbObjectRef object) async =>
      object.schema == null ? '"${object.name}"' : '"${object.schema}"."${object.name}"';

  @override
  Future<String> objectQuery(String sessionId, DbObjectRef object, {int limit = 100}) async =>
      'SELECT * FROM ${await identifier(sessionId, object)} LIMIT $limit';

  @override
  Future<DbPreparedQuery> prepareQuery(
    String sessionId,
    String text, {
    required DbRunMode mode,
    required int selectionStart,
    required int selectionEnd,
  }) async => DbPreparedQuery(text: text.trim());

  @override
  Future<String> formatQuery(DbEngine engine, String text) async => text
      .replaceAllMapped(
        RegExp(r'\s+(FROM|WHERE|GROUP BY|ORDER BY|LIMIT|JOIN|LEFT JOIN)\s', caseSensitive: false),
        (m) => '\n${m[1]!.toUpperCase()} ',
      )
      .trim();

  @override
  Future<List<DbCompletion>> completeQuery(String sessionId, String text, int cursor) async => [
    for (final table in _tables.keys) DbCompletion(insertText: table, label: table, detail: 'table · public'),
  ];

  @override
  Future<DbQueryReview> reviewQuery(String sessionId, DbQueryRequest request) async {
    final upper = request.text.trimLeft().toUpperCase();
    final read = ['SELECT', 'WITH', 'SHOW', 'EXPLAIN', 'GET', 'SCAN', 'HGETALL', 'DB.'].any(upper.startsWith);
    final session = _sessions[sessionId];
    final blocked = !read && (session?.production ?? true);
    return DbQueryReview(
      ticket: 'review-${_next++}',
      risk: read ? DbQueryRisk.read : (blocked ? DbQueryRisk.blocked : DbQueryRisk.write),
      expiresAt: DateTime.now().add(const Duration(minutes: 1)),
      suggestion: upper.startsWith('KEYS') ? 'SCAN 0 MATCH * COUNT 100' : null,
    );
  }

  final _queries = <String, DbQueryRequest>{};

  @override
  Future<String> execute(String sessionId, DbQueryRequest request) async {
    final id = 'query-${_next++}';
    _queries[id] = request;
    final session = _sessions[sessionId];
    if (session != null) {
      _history.insert(
        0,
        DbHistoryEntry(
          id: 'history-${_next++}',
          connectionId: session.connectionId,
          engine: session.engine,
          text: request.text,
          createdAt: DateTime.now(),
          elapsedMs: 14,
          rows: 8,
        ),
      );
    }
    return id;
  }

  static DbCell _text(String value) => DbCell(DbCellKind.text, value);
  static DbCell _int(int value) => DbCell(DbCellKind.intValue, '$value');

  @override
  Stream<DbQueryEvent> watchQuery(String sessionId, String queryId) async* {
    // A join reads customers too; a plain SELECT from orders is editable.
    final join = _queries[queryId]?.text.toUpperCase().contains('JOIN') ?? true;
    final columns = [
      const DbColumn(name: 'id', typeName: 'int8', nullable: false, primaryKey: true),
      if (join) ...const [DbColumn(name: 'customer', typeName: 'text'), DbColumn(name: 'email', typeName: 'text')] else
        const DbColumn(name: 'customer_id', typeName: 'int8', nullable: false),
      const DbColumn(name: 'total', typeName: 'numeric'),
      const DbColumn(name: 'status', typeName: 'text'),
      const DbColumn(name: 'created_at', typeName: 'timestamptz'),
    ];
    const names = [
      'Anna Petrova',
      'Ivan Smirnov',
      'Maria Kuznetsova',
      'Oleg Volkov',
      'Elena Sokolova',
      'Pavel Orlov',
      'Irina Lebedeva',
      'Sergey Kozlov',
    ];
    const statuses = ['paid', 'shipped', 'paid', 'refunded', 'paid', 'new', 'shipped', 'paid'];
    yield DbQueryEvent(
      kind: DbQueryEventKind.started,
      columns: columns,
      editableTable: join
          ? null
          : const DbObjectRef(database: 'shop', schema: 'public', kind: DbObjectKind.table, name: 'orders'),
    );
    yield DbQueryEvent(
      kind: DbQueryEventKind.rows,
      rows: [
        for (var i = 0; i < names.length; i++)
          [
            _int(10240 + i),
            if (join) ...[
              _text(names[i]),
              _text('${names[i].split(' ').first.toLowerCase()}@example.com'),
            ] else
              _int(501 + i * 7),
            DbCell(DbCellKind.decimal, (1290.5 + i * 317.25).toStringAsFixed(2)),
            _text(statuses[i]),
            DbCell(DbCellKind.timestamp, '2026-10-${(20 - i).toString().padLeft(2, '0')} 1${i % 10}:24:0${i % 10}+03'),
          ],
      ],
    );
    yield const DbQueryEvent(kind: DbQueryEventKind.statementDone, elapsedMs: 14);
    yield const DbQueryEvent(kind: DbQueryEventKind.done);
  }

  @override
  Future<void> loadMore(String sessionId, String queryId, {int rows = 1000}) async {}

  @override
  Future<void> cancelQuery(String sessionId, String queryId) async {}

  @override
  Future<void> releaseQuery(String sessionId, String queryId) async => _queries.remove(queryId);

  @override
  Future<DbExplainPlan> explain(String sessionId, DbQueryRequest request, {bool analyze = false}) async =>
      DbExplainPlan(
        engine: _sessions[sessionId]?.engine ?? DbEngine.postgresql,
        analyzed: analyze,
        document: {
          'plan': [
            {
              'Plan': {
                'Node Type': 'Hash Join',
                'Join Type': 'Inner',
                'Hash Cond': '(o.customer_id = c.id)',
                'Total Cost': 412.8,
                'Plan Rows': 1840,
                'Plans': [
                  {
                    'Node Type': 'Seq Scan',
                    'Relation Name': 'orders',
                    'Filter': "(status = 'paid'::text)",
                    'Total Cost': 288.1,
                    'Plan Rows': 1840,
                  },
                  {
                    'Node Type': 'Hash',
                    'Total Cost': 64.5,
                    'Plans': [
                      {
                        'Node Type': 'Index Scan',
                        'Relation Name': 'customers',
                        'Index Name': 'customers_pkey',
                        'Total Cost': 61.2,
                        'Plan Rows': 950,
                      },
                    ],
                  },
                ],
              },
            },
          ],
        },
      );

  // History.

  @override
  Future<DbHistoryPolicy> historyPolicy(String connectionId) async => DbHistoryPolicy.local;

  @override
  Future<void> setHistoryPolicy(String connectionId, DbHistoryPolicy policy) async {}

  @override
  Future<DbHistoryPage> history(
    String connectionId, {
    String query = '',
    bool? succeeded,
    bool favoritesOnly = false,
    String? cursor,
    int limit = 100,
  }) async => DbHistoryPage(
    entries: [
      for (final entry in _history)
        if (entry.connectionId == connectionId && entry.text.toLowerCase().contains(query.toLowerCase())) entry,
    ],
  );

  @override
  Future<void> setFavorite(String historyId, bool favorite) async {}

  @override
  Future<void> saveHistorySnippet(String historyId, String name) async {}

  @override
  Future<void> deleteHistory(String connectionId) async => _history.removeWhere((e) => e.connectionId == connectionId);

  // Export.

  @override
  Future<SecretText> copyResult(
    String sessionId,
    String queryId,
    int resultIndex,
    DbResultSelection selection,
    DbExportOptions options,
  ) async => SecretText('id,customer\n10240,Anna Petrova\n');

  @override
  Future<DbExportDestination?> chooseExportDestination(DbExportOptions options) async => null;

  @override
  Future<String> startExport(
    String sessionId,
    String queryId,
    int resultIndex,
    DbExportOptions options,
    DbExportDestination destination,
  ) async => 'export';

  @override
  Stream<DbExportProgress> watchExport(String sessionId, String exportId) =>
      Stream.value(const DbExportProgress(id: 'export', rows: 8, bytes: 512, done: true));

  @override
  Future<void> cancelExport(String sessionId, String exportId) async {}

  // Assistant.

  @override
  Future<List<DbAiProvider>> aiProviders() async => const [
    DbAiProvider(id: 'local', name: 'qwen3-4b (local)', external: false),
  ];

  /// Tickets whose question was asked in Russian: the canned answer follows.
  final _russianTickets = <String>{};

  @override
  Future<DbAiReview> prepareAi(String sessionId, String providerId, DbAiRequest request) async {
    final ticket = 'ai-${_next++}';
    if (RegExp('[А-Яа-яЁё]').hasMatch(request.question)) _russianTickets.add(ticket);
    return DbAiReview(
      ticket: ticket,
      provider: const DbAiProvider(id: 'local', name: 'qwen3-4b (local)', external: false),
      connectionName: _byId(_sessions[sessionId]?.connectionId ?? 'pg').name,
      expiresAt: DateTime.now().add(const Duration(minutes: 2)),
      schemaObjects: const ['public.orders', 'public.customers'],
    );
  }

  @override
  Stream<DbAiEvent> askAi(String sessionId, String ticket, DbAiConsent consent) async* {
    yield DbAiEvent(kind: DbAiEventKind.context, content: SecretText('public.orders, public.customers'));
    yield DbAiEvent(
      kind: DbAiEventKind.draft,
      content: SecretText(
        'SELECT c.name, sum(o.total) AS revenue\nFROM orders o\nJOIN customers c ON c.id = o.customer_id\n'
        "WHERE o.status = 'paid' AND o.created_at >= now() - interval '30 days'\nGROUP BY c.name\nORDER BY revenue DESC\nLIMIT 10;",
      ),
    );
    yield DbAiEvent(
      kind: DbAiEventKind.explanation,
      content: SecretText(
        _russianTickets.remove(ticket)
            ? 'Десять клиентов с наибольшей оплаченной выручкой за последние 30 дней.'
            : 'Top 10 customers by paid revenue over the last 30 days.',
      ),
    );
    yield const DbAiEvent(kind: DbAiEventKind.done);
  }

  @override
  Future<void> cancelAi(String sessionId, String ticket) async {}

  @override
  Future<void> revokeAiConsent(String sessionId) async {}
}
