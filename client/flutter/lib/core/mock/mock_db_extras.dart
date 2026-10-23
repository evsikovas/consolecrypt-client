import 'package:consolecrypt/db/db_csv_import.dart';
import 'package:consolecrypt/db/db_designer_models.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_result_edits.dart';

// Demo data of the mock backend for the engines and tools added in 0.9.2
// (SQLite, Cassandra / ScyllaDB, table designer, row changes, CSV import);
// nothing here is shown in release builds. The real statements, plans and
// structures come from the core.
// l10n-ignore-file

DbTreeNode _node(
  DbObjectRef ref, {
  bool children = false,
  String? detail,
  bool pk = false,
  bool? nullable,
  String? defaultValue,
  DbObjectRef? fk,
}) => DbTreeNode(
  reference: ref,
  hasChildren: children,
  detail: detail,
  primaryKey: pk,
  nullable: nullable,
  defaultValue: defaultValue,
  foreignKey: fk,
);

// SQLite: one file with a small warehouse inventory -------------------------

const _sqliteMain = 'main';

DbObjectRef _sqliteRef(DbObjectKind kind, String name, [String? parent]) =>
    DbObjectRef(database: _sqliteMain, kind: kind, name: name, parentName: parent);

/// Columns of the demo SQLite tables: name, type, primary key, not null.
const mockSqliteTables = {
  'items': [
    ('id', 'INTEGER', true, true),
    ('sku', 'TEXT', false, true),
    ('title', 'TEXT', false, true),
    ('supplier_id', 'INTEGER', false, false),
    ('price', 'NUMERIC', false, false),
  ],
  'locations': [('id', 'INTEGER', true, true), ('code', 'TEXT', false, true), ('zone', 'TEXT', false, false)],
  'stock_moves': [
    ('id', 'INTEGER', true, true),
    ('item_id', 'INTEGER', false, true),
    ('location_id', 'INTEGER', false, true),
    ('quantity', 'INTEGER', false, true),
    ('moved_at', 'TEXT', false, true),
  ],
  'suppliers': [('id', 'INTEGER', true, true), ('name', 'TEXT', false, true), ('email', 'TEXT', false, false)],
};

/// The structure the core returns for a SQLite file: the file, then its
/// tables, views, indexes, triggers, AUTOINCREMENT counters and type
/// affinities, then per table its columns, keys, foreign keys, indexes,
/// references and triggers.
DbTreePage mockSqliteTree(DbObjectRef? parent, {required String file}) {
  if (parent == null) {
    return DbTreePage(
      nodes: [
        _node(
          DbObjectRef(database: _sqliteMain, kind: DbObjectKind.database, name: file),
          children: true,
          detail: 'SQLite 3.50.4',
        ),
      ],
    );
  }
  if (parent.kind == DbObjectKind.database) {
    return DbTreePage(
      nodes: [
        for (final table in mockSqliteTables.keys)
          _node(
            _sqliteRef(DbObjectKind.table, table),
            children: true,
            detail: table == 'stock_moves' ? 'STRICT' : null,
          ),
        _node(_sqliteRef(DbObjectKind.view, 'low_stock'), children: true),
        _node(_sqliteRef(DbObjectKind.indexObject, 'items_sku_idx', 'items'), detail: 'items'),
        _node(_sqliteRef(DbObjectKind.indexObject, 'stock_moves_item_idx', 'stock_moves'), detail: 'stock_moves'),
        _node(_sqliteRef(DbObjectKind.trigger, 'items_touch_updated', 'items'), detail: 'items · AFTER UPDATE'),
        _node(
          _sqliteRef(DbObjectKind.trigger, 'stock_moves_touch', 'stock_moves'),
          detail: 'stock_moves · AFTER INSERT',
        ),
        _node(_sqliteRef(DbObjectKind.sequence, 'stock_moves'), detail: '18342'),
        _node(_sqliteRef(DbObjectKind.sequence, 'items'), detail: '1204'),
        for (final (name, detail) in const [
          ('INTEGER', 'INT, INTEGER, TINYINT, SMALLINT, MEDIUMINT, BIGINT, INT8'),
          ('TEXT', 'TEXT, CHAR(n), VARCHAR(n), NCHAR(n), CLOB'),
          ('REAL', 'REAL, DOUBLE, DOUBLE PRECISION, FLOAT'),
          ('NUMERIC', 'NUMERIC, DECIMAL(p,s), BOOLEAN, DATE, DATETIME'),
          ('BLOB', 'BLOB'),
        ])
          _node(_sqliteRef(DbObjectKind.dataType, name), detail: detail),
      ],
    );
  }
  final table = parent.name;
  final columns = mockSqliteTables[table] ?? const [];
  final fks = <String, (String, String)>{
    if (table == 'items') 'supplier_id': ('suppliers', 'id'),
    if (table == 'stock_moves') ...{'item_id': ('items', 'id'), 'location_id': ('locations', 'id')},
  };
  final references = <(String, String)>[
    if (table == 'items') ('stock_moves', 'item_id'),
    if (table == 'locations') ('stock_moves', 'location_id'),
    if (table == 'suppliers') ('items', 'supplier_id'),
  ];
  return DbTreePage(
    nodes: [
      for (final (name, type, pk, notNull) in columns)
        _node(
          _sqliteRef(DbObjectKind.column, name, table),
          detail: type,
          pk: pk,
          nullable: !notNull,
          defaultValue: name == 'moved_at' ? "strftime('%Y-%m-%dT%H:%M:%fZ')" : null,
          fk: fks[name] == null ? null : _sqliteRef(DbObjectKind.table, fks[name]!.$1),
        ),
      if (columns.isNotEmpty) _node(_sqliteRef(DbObjectKind.key, 'PRIMARY KEY', table), detail: '(id)', pk: true),
      if (table == 'items')
        _node(_sqliteRef(DbObjectKind.key, 'sqlite_autoindex_items_1', table), detail: 'UNIQUE (sku)'),
      for (final MapEntry(key: column, value: (target, to)) in fks.entries)
        _node(
          _sqliteRef(DbObjectKind.foreignKey, column, table),
          detail: '→ $target($to)${target == 'items' ? ' ON DELETE CASCADE' : ''}',
          fk: _sqliteRef(DbObjectKind.table, target),
        ),
      if (table == 'items') _node(_sqliteRef(DbObjectKind.indexObject, 'items_sku_idx', table), detail: 'UNIQUE (sku)'),
      if (table == 'stock_moves')
        _node(_sqliteRef(DbObjectKind.indexObject, 'stock_moves_item_idx', table), detail: '(item_id, moved_at)'),
      for (final (other, column) in references)
        _node(
          _sqliteRef(DbObjectKind.reference, other, table),
          detail: '$other($column) → (id)',
          fk: _sqliteRef(DbObjectKind.table, other),
        ),
      if (table == 'stock_moves')
        _node(_sqliteRef(DbObjectKind.trigger, 'stock_moves_touch', table), detail: 'AFTER INSERT'),
      if (table == 'items')
        _node(_sqliteRef(DbObjectKind.trigger, 'items_touch_updated', table), detail: 'AFTER UPDATE'),
    ],
  );
}

String mockSqliteDdl(DbObjectRef object) {
  final columns = mockSqliteTables[object.name];
  if (columns == null) return '-- ${object.name}';
  final lines = [
    for (final (name, type, pk, notNull) in columns)
      '    $name $type${pk ? ' PRIMARY KEY AUTOINCREMENT' : ''}${notNull && !pk ? ' NOT NULL' : ''}',
  ];
  return 'CREATE TABLE ${object.name} (\n${lines.join(',\n')}\n);';
}

// Cassandra / ScyllaDB: one keyspace of device telemetry ---------------------

const _keyspace = 'telemetry';

/// Keyspaces with their replication, then tables, a materialized view and a
/// user type, then columns marked PARTITION KEY / CLUSTERING / STATIC.
DbTreePage mockCassandraTree(DbObjectRef? parent) {
  DbObjectRef ref(DbObjectKind kind, String name, [String? parentName]) =>
      DbObjectRef(database: _keyspace, kind: kind, name: name, parentName: parentName);
  if (parent == null) {
    return DbTreePage(
      nodes: [
        _node(
          const DbObjectRef(database: _keyspace, kind: DbObjectKind.database, name: _keyspace),
          children: true,
          detail: 'NetworkTopologyStrategy dc1:3',
        ),
        _node(
          const DbObjectRef(database: 'audit', kind: DbObjectKind.database, name: 'audit'),
          children: true,
          detail: 'NetworkTopologyStrategy dc1:3',
        ),
      ],
    );
  }
  if (parent.kind == DbObjectKind.database) {
    return DbTreePage(
      nodes: [
        _node(ref(DbObjectKind.table, 'readings_by_device'), children: true),
        _node(ref(DbObjectKind.table, 'devices'), children: true),
        _node(ref(DbObjectKind.materializedView, 'readings_by_site'), children: true),
        _node(ref(DbObjectKind.userType, 'location'), children: true),
      ],
    );
  }
  const columns = {
    'readings_by_device': [
      ('device_id', 'uuid PARTITION KEY', true),
      ('day', 'date PARTITION KEY', true),
      ('ts', 'timestamp CLUSTERING DESC', true),
      ('site', 'text STATIC', false),
      ('temperature', 'double', false),
      ('humidity', 'double', false),
      ('tags', 'set<text>', false),
    ],
    'devices': [('device_id', 'uuid PARTITION KEY', true), ('model', 'text', false), ('installed', 'date', false)],
  };
  return DbTreePage(
    nodes: [
      for (final (name, detail, key) in columns[parent.name] ?? const <(String, String, bool)>[])
        _node(ref(DbObjectKind.column, name, parent.name), detail: detail, pk: key, nullable: !key),
    ],
  );
}

// Table designer: the PostgreSQL dialect and a small planner -----------------

const _pgTypes = [
  DbDesignType(name: 'smallint', group: 'numeric'),
  DbDesignType(name: 'integer', group: 'numeric'),
  DbDesignType(name: 'bigint', group: 'numeric'),
  DbDesignType(name: 'numeric', group: 'numeric', params: DbDesignTypeParams.precisionScale),
  DbDesignType(name: 'real', group: 'numeric'),
  DbDesignType(name: 'double precision', group: 'numeric'),
  DbDesignType(name: 'boolean'),
  DbDesignType(name: 'text', group: 'text'),
  DbDesignType(name: 'varchar', group: 'text', params: DbDesignTypeParams.length),
  DbDesignType(name: 'char', group: 'text', params: DbDesignTypeParams.length),
  DbDesignType(name: 'uuid'),
  DbDesignType(name: 'jsonb', group: 'json'),
  DbDesignType(name: 'json', group: 'json'),
  DbDesignType(name: 'date', group: 'time'),
  DbDesignType(name: 'time', group: 'time', params: DbDesignTypeParams.length),
  DbDesignType(name: 'timestamp', group: 'time', params: DbDesignTypeParams.length),
  DbDesignType(name: 'timestamptz', group: 'time', params: DbDesignTypeParams.length),
  DbDesignType(name: 'interval', group: 'time'),
  DbDesignType(name: 'bytea', group: 'binary'),
  DbDesignType(name: 'inet'),
];

/// What the designer offers; PostgreSQL mirrors the core's dialect, other
/// engines get a reduced demo list.
DbDesignDialect mockDesignDialect(DbEngine engine) => switch (engine) {
  DbEngine.postgresql => const DbDesignDialect(
    engine: DbEngine.postgresql,
    features: {
      'schemas',
      'primary_key',
      'unique',
      'foreign_keys',
      'checks',
      'indexes',
      'arrays',
      'table_comment',
      'column_comments',
      'alter',
      'transactional',
    },
    types: _pgTypes,
    defaultType: 'integer',
    identityKinds: ['by_default', 'always', 'serial'],
    defaultPresets: [
      'now()',
      'CURRENT_TIMESTAMP',
      'CURRENT_DATE',
      'gen_random_uuid()',
      '0',
      "''",
      'true',
      'false',
      "'{}'::jsonb",
    ],
    indexMethods: ['btree', 'hash', 'gin', 'gist', 'brin', 'spgist'],
    fkActions: ['NO ACTION', 'RESTRICT', 'CASCADE', 'SET NULL', 'SET DEFAULT'],
    options: [
      DbDesignOption(
        id: 'partition_by',
        kind: DbDesignOptionKind.choice,
        choices: ['', 'RANGE', 'LIST', 'HASH'],
        createOnly: true,
      ),
      DbDesignOption(id: 'partition_key', kind: DbDesignOptionKind.text, createOnly: true),
    ],
  ),
  _ => DbDesignDialect(
    engine: engine,
    features: const {'primary_key', 'indexes', 'alter'},
    types: const [
      DbDesignType(name: 'int', group: 'numeric'),
      DbDesignType(name: 'bigint', group: 'numeric'),
      DbDesignType(name: 'varchar', group: 'text', params: DbDesignTypeParams.length, lengthRequired: true),
      DbDesignType(name: 'text', group: 'text'),
      DbDesignType(name: 'datetime', group: 'time'),
    ],
    defaultType: 'int',
  ),
};

String _quote(String name) => '"${name.replaceAll('"', '""')}"';
String _literal(String text) => "'${text.replaceAll("'", "''")}'";

bool _defaultIsSql(String text) {
  final t = text.trim();
  final upper = t.toUpperCase();
  return RegExp(r'^-?\d+(\.\d+)?$').hasMatch(t) ||
      t.startsWith("'") ||
      t.contains('(') ||
      t.contains('::') ||
      const {'TRUE', 'FALSE', 'NULL', 'CURRENT_TIMESTAMP', 'CURRENT_DATE', 'CURRENT_TIME'}.contains(upper);
}

String _table(DbTableDesign d) => (d.schema ?? '').isEmpty ? _quote(d.name) : '${_quote(d.schema!)}.${_quote(d.name)}';

String _columnType(DbDesignColumn c) {
  if (c.identity == 'serial') return c.dataType == 'bigint' ? 'bigserial' : 'serial';
  return c.typeLabel;
}

String _column(DbDesignColumn c) {
  final line = StringBuffer('${_quote(c.name)} ${_columnType(c)}');
  if (c.identity == 'by_default') line.write(' GENERATED BY DEFAULT AS IDENTITY');
  if (c.identity == 'always') line.write(' GENERATED ALWAYS AS IDENTITY');
  if (!c.nullable) line.write(' NOT NULL');
  if (c.defaultValue.trim().isNotEmpty) {
    line.write(' DEFAULT ${_defaultIsSql(c.defaultValue) ? c.defaultValue.trim() : _literal(c.defaultValue.trim())}');
  }
  return '$line';
}

String _name(String name, String table, List<String> columns, String suffix) =>
    name.trim().isNotEmpty ? name : [table, ...columns, suffix].join('_');

String _foreignKey(DbTableDesign d, DbDesignForeignKey fk) {
  final schema = (fk.refSchema ?? '').isNotEmpty ? fk.refSchema : d.schema;
  final target = (schema ?? '').isEmpty ? _quote(fk.refTable) : '${_quote(schema!)}.${_quote(fk.refTable)}';
  return 'CONSTRAINT ${_quote(_name(fk.name, d.name, fk.columns, 'fkey'))} FOREIGN KEY (${fk.columns.map(_quote).join(', ')}) '
      'REFERENCES $target (${fk.refColumns.map(_quote).join(', ')})'
      '${fk.onDelete.isEmpty ? '' : ' ON DELETE ${fk.onDelete}'}${fk.onUpdate.isEmpty ? '' : ' ON UPDATE ${fk.onUpdate}'}';
}

String _index(DbTableDesign d, DbDesignIndex index) =>
    'CREATE ${index.unique ? 'UNIQUE ' : ''}INDEX ${_quote(_name(index.name, d.name, index.columns, 'idx'))} '
    'ON ${_table(d)}${index.method.isEmpty ? '' : ' USING ${index.method}'} (${index.columns.map(_quote).join(', ')})';

/// A stand-in for the core's planner (PostgreSQL syntax): CREATE TABLE with
/// keys, checks and foreign keys, then indexes and comments; for an existing
/// table, ALTER statements for added, dropped and changed columns and new
/// indexes. Never run anywhere.
DbDesignPlan mockPlanDesign(DbEngine engine, DbTableDesign? original, DbTableDesign design) {
  final issues = <DbDesignIssue>[
    if (design.name.trim().isEmpty) const DbDesignIssue(error: true, code: 'name_required', target: 'table.name'),
    if (design.columns.isEmpty) const DbDesignIssue(error: true, code: 'columns_required', target: 'columns'),
    for (final (i, c) in design.columns.indexed)
      if (c.name.trim().isEmpty) DbDesignIssue(error: true, code: 'name_required', target: 'columns.$i.name'),
  ];
  if (issues.any((i) => i.error)) return DbDesignPlan(issues: issues);
  final statements = <DbDesignStatement>[];
  if (original == null) {
    final lines = [
      for (final c in design.columns) _column(c),
      if (design.primaryKey.columns.isNotEmpty)
        'CONSTRAINT ${_quote(_name(design.primaryKey.name, design.name, const [], 'pkey'))} PRIMARY KEY '
                '(${design.primaryKey.columns.map(_quote).join(', ')})'
            .trim(),
      for (final key in design.uniques)
        'CONSTRAINT ${_quote(_name(key.name, design.name, key.columns, 'key'))} UNIQUE (${key.columns.map(_quote).join(', ')})',
      for (final check in design.checks)
        'CONSTRAINT ${_quote(_name(check.name, design.name, const [], 'check'))} CHECK (${check.expression.trim()})',
      for (final fk in design.foreignKeys) _foreignKey(design, fk),
    ];
    statements
      ..add(DbDesignStatement(sql: 'CREATE TABLE ${_table(design)} (\n    ${lines.join(',\n    ')}\n)'))
      ..addAll([for (final index in design.indexes) DbDesignStatement(sql: _index(design, index))]);
    if (design.comment.isNotEmpty) {
      statements.add(DbDesignStatement(sql: 'COMMENT ON TABLE ${_table(design)} IS ${_literal(design.comment)}'));
    }
    for (final c in design.columns.where((c) => c.comment.isNotEmpty)) {
      statements.add(
        DbDesignStatement(sql: 'COMMENT ON COLUMN ${_table(design)}.${_quote(c.name)} IS ${_literal(c.comment)}'),
      );
    }
  } else {
    final table = _table(original);
    String alter(String op) => 'ALTER TABLE $table $op';
    final before = {for (final c in original.columns) c.name: c};
    final kept = {for (final c in design.columns) ?c.original};
    for (final c in original.columns.where((c) => !kept.contains(c.name))) {
      statements.add(DbDesignStatement(sql: alter('DROP COLUMN ${_quote(c.name)}'), destructive: true));
    }
    for (final c in design.columns) {
      final old = c.original == null ? null : before[c.original];
      if (old == null) {
        statements.add(DbDesignStatement(sql: alter('ADD COLUMN ${_column(c)}')));
        continue;
      }
      if (old.name != c.name) {
        statements.add(DbDesignStatement(sql: alter('RENAME COLUMN ${_quote(old.name)} TO ${_quote(c.name)}')));
      }
      if (old.typeLabel != c.typeLabel) {
        statements.add(
          DbDesignStatement(
            sql: alter(
              'ALTER COLUMN ${_quote(c.name)} TYPE ${_columnType(c)} USING ${_quote(c.name)}::${_columnType(c)}',
            ),
            destructive: true,
          ),
        );
      }
      if (old.nullable != c.nullable) {
        statements.add(
          DbDesignStatement(sql: alter('ALTER COLUMN ${_quote(c.name)} ${c.nullable ? 'DROP' : 'SET'} NOT NULL')),
        );
      }
      if (old.defaultValue != c.defaultValue && c.defaultValue.trim().isNotEmpty) {
        final value = _defaultIsSql(c.defaultValue) ? c.defaultValue.trim() : _literal(c.defaultValue.trim());
        statements.add(DbDesignStatement(sql: alter('ALTER COLUMN ${_quote(c.name)} SET DEFAULT $value')));
      }
    }
    final indexes = {for (final i in original.indexes) i.name};
    for (final index in design.indexes.where((i) => i.original == null || !indexes.contains(i.original))) {
      statements.add(DbDesignStatement(sql: _index(design, index)));
    }
  }
  return DbDesignPlan(
    statements: statements,
    script: [for (final s in statements) '${s.sql};'].join('\n\n'),
    issues: issues,
    transactional: engine == DbEngine.postgresql && statements.isNotEmpty,
  );
}

/// The current structure of a demo table as the designer loads it.
DbTableDesign mockTableDesign(DbObjectRef table, List<(String, String, bool)> columns) {
  String type(String t) => t.startsWith('numeric') ? 'numeric' : t;
  (int?, int?) params(String t) {
    final m = RegExp(r'\((\d+),(\d+)\)').firstMatch(t);
    return m == null ? (null, null) : (int.parse(m[1]!), int.parse(m[2]!));
  }

  return DbTableDesign(
    database: table.database,
    schema: table.schema ?? 'public',
    name: table.name,
    original: table.name,
    comment: 'Demo shop: ${table.name}',
    columns: [
      for (final (name, t, pk) in columns)
        DbDesignColumn(
          name: name,
          original: name,
          dataType: type(t),
          length: params(t).$1,
          scale: params(t).$2,
          nullable: !pk && !name.endsWith('_id'),
          identity: pk && t == 'bigint' ? 'by_default' : '',
          defaultValue: name == 'created_at' ? 'now()' : '',
        ),
    ],
    primaryKey: DbDesignKey(
      name: '${table.name}_pkey',
      original: '${table.name}_pkey',
      columns: [
        for (final (name, _, pk) in columns)
          if (pk) name,
      ],
    ),
    foreignKeys: [
      for (final (name, _, _) in columns)
        if (name.endsWith('_id'))
          DbDesignForeignKey(
            name: '${table.name}_${name}_fkey',
            original: '${table.name}_${name}_fkey',
            columns: [name],
            refSchema: table.schema ?? 'public',
            refTable: '${name.replaceAll('_id', '')}s',
            refColumns: const ['id'],
          ),
    ],
    indexes: [
      if (columns.any((c) => c.$1 == 'created_at'))
        DbDesignIndex(
          name: '${table.name}_created_at_idx',
          original: '${table.name}_created_at_idx',
          columns: const ['created_at'],
        ),
    ],
  );
}

// Row changes ------------------------------------------------------------------

/// Columns of a demo table for new rows: keys and timestamps come from the
/// server, other NOT NULL columns are required.
List<DbEditColumn> mockEditColumns(List<(String, String, bool)> columns) => [
  for (final (name, type, pk) in columns)
    DbEditColumn(
      name: name,
      typeName: type,
      nullable: !pk && !name.endsWith('_id'),
      hasDefault: pk || name == 'created_at' || name == 'status',
      key: pk,
    ),
];

String _cellLiteral(DbCell cell) => switch (cell.kind) {
  DbCellKind.nullValue => 'NULL',
  DbCellKind.intValue || DbCellKind.uint || DbCellKind.decimal || DbCellKind.float => cell.text,
  DbCellKind.boolValue => cell.text.toUpperCase(),
  _ => _literal(cell.text),
};

/// DELETE, UPDATE and INSERT statements by the key, values written out the
/// way the review shows them.
String mockRowChangesSql(DbObjectRef table, List<DbRowChange> changes) {
  final name = '${_quote(table.schema ?? 'public')}.${_quote(table.name)}';
  String where(Map<String, DbCell> key) =>
      key.entries.map((e) => '${_quote(e.key)} = ${_cellLiteral(e.value)}').join(' AND ');
  final ordered = [
    ...changes.where((c) => c.kind == DbRowChangeKind.delete),
    ...changes.where((c) => c.kind == DbRowChangeKind.update),
    ...changes.where((c) => c.kind == DbRowChangeKind.insert),
  ];
  return [
    for (final c in ordered)
      switch (c.kind) {
        DbRowChangeKind.delete => 'DELETE FROM $name WHERE ${where(c.originalPrimaryKey)};',
        DbRowChangeKind.update =>
          'UPDATE $name SET ${c.values.entries.map((e) => '${_quote(e.key)} = ${_cellLiteral(e.value)}').join(', ')} '
              'WHERE ${where(c.originalPrimaryKey)};',
        DbRowChangeKind.insert =>
          'INSERT INTO $name (${c.values.keys.map(_quote).join(', ')}) '
              'VALUES (${c.values.values.map(_cellLiteral).join(', ')});',
      },
  ].join('\n');
}

// CSV import -------------------------------------------------------------------

/// A demo export of orders: one column the table does not have and two
/// totals written with a decimal comma.
DbCsvPreview mockCsvPreview(DbCsvOptions options) => DbCsvPreview(
  delimiter: options.delimiter ?? ';',
  encoding: options.encoding == DbCsvEncoding.auto ? DbCsvEncoding.utf8 : options.encoding,
  columns: options.header
      ? const ['id', 'customer_id', 'total', 'status', 'created_at', 'manager_note']
      : const ['column_1', 'column_2', 'column_3', 'column_4', 'column_5', 'column_6'],
  rows: const [
    ['20481', '501', '1290.50', 'paid', '2026-10-21 09:14:02', 'call back'],
    ['20482', '508', '317,25', 'new', '2026-10-21 09:40:11', null],
    ['20483', '515', '2045.00', 'shipped', '2026-10-21 10:02:45', null],
    ['20484', '522', '88,90', 'paid', '2026-10-21 10:31:20', 'gift wrap'],
    ['20485', '529', '640.00', 'refunded', '2026-10-21 11:05:57', null],
    ['20486', '536', '1999.99', 'paid', '2026-10-21 11:48:33', null],
  ],
  fileBytes: 48 * 1024,
);

List<DbCsvCheck> mockCsvChecks(List<DbCsvMapping> mapping) => [
  for (final m in mapping)
    DbCsvCheck(
      target: m.target,
      kind: m.target == 'total' ? 'decimal' : '',
      bad: m.target == 'total' ? 2 : 0,
      samples: m.target == 'total' ? const ['317,25', '88,90'] : const [],
    ),
];
