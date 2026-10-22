import 'dart:convert';

import 'package:consolecrypt/db/db_models.dart';

/// Table designer wire models (JSON of the core's `designer` module). The
/// design is edited in place by the designer controller; [DbTableDesign.copy]
/// takes snapshots for undo and the pending-changes comparison.

List<String> _strings(Object? value) => (value as List? ?? const []).cast<String>().toList();

final class DbDesignColumn {
  DbDesignColumn({
    required this.name,
    required this.dataType,
    this.original,
    this.length,
    this.scale,
    this.array = false,
    this.unsigned = false,
    this.nullable = true,
    this.defaultValue = '',
    this.identity = '',
    this.lowCardinality = false,
    this.comment = '',
  });
  String? original;
  String name, dataType, defaultValue, identity, comment;
  int? length, scale;
  bool array, unsigned, nullable, lowCardinality;

  factory DbDesignColumn.fromJson(DbJson json) => DbDesignColumn(
    original: json['original'] as String?,
    name: json['name'] as String? ?? '',
    dataType: json['data_type'] as String? ?? '',
    length: json['length'] as int?,
    scale: json['scale'] as int?,
    array: json['array'] as bool? ?? false,
    unsigned: json['unsigned'] as bool? ?? false,
    nullable: json['nullable'] as bool? ?? true,
    defaultValue: json['default'] as String? ?? '',
    identity: json['identity'] as String? ?? '',
    lowCardinality: json['low_cardinality'] as bool? ?? false,
    comment: json['comment'] as String? ?? '',
  );
  DbJson toJson() => {
    'original': original,
    'name': name,
    'data_type': dataType,
    'length': length,
    'scale': scale,
    'array': array,
    'unsigned': unsigned,
    'nullable': nullable,
    'default': defaultValue,
    'identity': identity,
    'low_cardinality': lowCardinality,
    'comment': comment,
  };
  DbDesignColumn copy() => DbDesignColumn.fromJson(toJson());

  /// The type as shown in the tree (`varchar(50)`, `numeric(10,2)`, `text[]`).
  String get typeLabel {
    final params = length == null ? '' : (scale == null ? '($length)' : '($length,$scale)');
    return '$dataType${dataType.contains('(') ? '' : params}${array ? '[]' : ''}';
  }
}

/// Primary key or unique constraint.
final class DbDesignKey {
  DbDesignKey({this.name = '', List<String>? columns, this.original}) : columns = columns ?? [];
  String? original;
  String name;
  List<String> columns;
  factory DbDesignKey.fromJson(DbJson json) => DbDesignKey(
    original: json['original'] as String?,
    name: json['name'] as String? ?? '',
    columns: _strings(json['columns']),
  );
  DbJson toJson() => {'original': original, 'name': name, 'columns': columns};
}

final class DbDesignForeignKey {
  DbDesignForeignKey({
    this.name = '',
    List<String>? columns,
    this.refSchema,
    this.refTable = '',
    List<String>? refColumns,
    this.onDelete = '',
    this.onUpdate = '',
    this.original,
  }) : columns = columns ?? [],
       refColumns = refColumns ?? [];
  String? original, refSchema;
  String name, refTable, onDelete, onUpdate;
  List<String> columns, refColumns;
  factory DbDesignForeignKey.fromJson(DbJson json) => DbDesignForeignKey(
    original: json['original'] as String?,
    name: json['name'] as String? ?? '',
    columns: _strings(json['columns']),
    refSchema: json['ref_schema'] as String?,
    refTable: json['ref_table'] as String? ?? '',
    refColumns: _strings(json['ref_columns']),
    onDelete: json['on_delete'] as String? ?? '',
    onUpdate: json['on_update'] as String? ?? '',
  );
  DbJson toJson() => {
    'original': original,
    'name': name,
    'columns': columns,
    'ref_schema': refSchema,
    'ref_table': refTable,
    'ref_columns': refColumns,
    'on_delete': onDelete,
    'on_update': onUpdate,
  };
}

final class DbDesignIndex {
  DbDesignIndex({this.name = '', List<String>? columns, this.unique = false, this.method = '', this.original})
    : columns = columns ?? [];
  String? original;
  String name, method;
  List<String> columns;
  bool unique;
  factory DbDesignIndex.fromJson(DbJson json) => DbDesignIndex(
    original: json['original'] as String?,
    name: json['name'] as String? ?? '',
    columns: _strings(json['columns']),
    unique: json['unique'] as bool? ?? false,
    method: json['method'] as String? ?? '',
  );
  DbJson toJson() => {'original': original, 'name': name, 'columns': columns, 'unique': unique, 'method': method};
}

final class DbDesignCheck {
  DbDesignCheck({this.name = '', this.expression = '', this.original});
  String? original;
  String name, expression;
  factory DbDesignCheck.fromJson(DbJson json) => DbDesignCheck(
    original: json['original'] as String?,
    name: json['name'] as String? ?? '',
    expression: json['expression'] as String? ?? '',
  );
  DbJson toJson() => {'original': original, 'name': name, 'expression': expression};
}

final class DbTableDesign {
  DbTableDesign({
    this.database = '',
    this.schema,
    this.name = '',
    this.original,
    this.comment = '',
    List<DbDesignColumn>? columns,
    DbDesignKey? primaryKey,
    List<DbDesignKey>? uniques,
    List<DbDesignForeignKey>? foreignKeys,
    List<DbDesignIndex>? indexes,
    List<DbDesignCheck>? checks,
    Map<String, String>? options,
  }) : columns = columns ?? [],
       primaryKey = primaryKey ?? DbDesignKey(),
       uniques = uniques ?? [],
       foreignKeys = foreignKeys ?? [],
       indexes = indexes ?? [],
       checks = checks ?? [],
       options = options ?? {};
  String database, name, comment;
  String? schema, original;
  List<DbDesignColumn> columns;
  DbDesignKey primaryKey;
  List<DbDesignKey> uniques;
  List<DbDesignForeignKey> foreignKeys;
  List<DbDesignIndex> indexes;
  List<DbDesignCheck> checks;
  Map<String, String> options;

  factory DbTableDesign.fromJson(DbJson json) => DbTableDesign(
    database: json['database'] as String? ?? '',
    schema: json['schema'] as String?,
    name: json['name'] as String? ?? '',
    original: json['original'] as String?,
    comment: json['comment'] as String? ?? '',
    columns: [for (final c in (json['columns'] as List? ?? const [])) DbDesignColumn.fromJson(c as DbJson)],
    primaryKey: json['primary_key'] == null ? null : DbDesignKey.fromJson(json['primary_key'] as DbJson),
    uniques: [for (final k in (json['uniques'] as List? ?? const [])) DbDesignKey.fromJson(k as DbJson)],
    foreignKeys: [
      for (final k in (json['foreign_keys'] as List? ?? const [])) DbDesignForeignKey.fromJson(k as DbJson),
    ],
    indexes: [for (final i in (json['indexes'] as List? ?? const [])) DbDesignIndex.fromJson(i as DbJson)],
    checks: [for (final c in (json['checks'] as List? ?? const [])) DbDesignCheck.fromJson(c as DbJson)],
    options: (json['options'] as Map? ?? const {}).map((k, v) => MapEntry(k as String, v as String)),
  );
  DbJson toJson() => {
    'database': database,
    'schema': schema,
    'name': name,
    'original': original,
    'comment': comment,
    'columns': [for (final c in columns) c.toJson()],
    'primary_key': primaryKey.toJson(),
    'uniques': [for (final k in uniques) k.toJson()],
    'foreign_keys': [for (final k in foreignKeys) k.toJson()],
    'indexes': [for (final i in indexes) i.toJson()],
    'checks': [for (final c in checks) c.toJson()],
    'options': options,
  };
  String encode() => jsonEncode(toJson());
  DbTableDesign copy() => DbTableDesign.fromJson(jsonDecode(encode()) as DbJson);
}

enum DbDesignTypeParams { none, length, precisionScale }

final class DbDesignType {
  const DbDesignType({
    required this.name,
    this.group = 'other',
    this.params = DbDesignTypeParams.none,
    this.lengthRequired = false,
  });
  final String name, group;
  final DbDesignTypeParams params;
  final bool lengthRequired;
  factory DbDesignType.fromJson(DbJson json) => DbDesignType(
    name: json['name'] as String,
    group: json['group'] as String? ?? 'other',
    params: switch (json['params']) {
      'length' => DbDesignTypeParams.length,
      'precision_scale' => DbDesignTypeParams.precisionScale,
      _ => DbDesignTypeParams.none,
    },
    lengthRequired: json['length_required'] as bool? ?? false,
  );
}

enum DbDesignOptionKind { boolean, choice, text, number }

final class DbDesignOption {
  const DbDesignOption({
    required this.id,
    required this.kind,
    this.choices = const [],
    this.defaultValue = '',
    this.createOnly = false,
  });
  final String id, defaultValue;
  final DbDesignOptionKind kind;
  final List<String> choices;
  final bool createOnly;
  factory DbDesignOption.fromJson(DbJson json) => DbDesignOption(
    id: json['id'] as String,
    kind: switch (json['kind']) {
      'bool' => DbDesignOptionKind.boolean,
      'choice' => DbDesignOptionKind.choice,
      'number' => DbDesignOptionKind.number,
      _ => DbDesignOptionKind.text,
    },
    choices: _strings(json['choices']),
    defaultValue: json['default'] as String? ?? '',
    createOnly: json['create_only'] as bool? ?? false,
  );
}

/// What the designer offers for an engine (from the core's dialect).
final class DbDesignDialect {
  const DbDesignDialect({
    required this.engine,
    this.features = const {},
    this.types = const [],
    this.defaultType = '',
    this.identityKinds = const [],
    this.defaultPresets = const [],
    this.indexMethods = const [],
    this.fkActions = const [],
    this.options = const [],
    this.maxIdentifier = 63,
  });
  final DbEngine engine;
  final Set<String> features;
  final List<DbDesignType> types;
  final String defaultType;
  final List<String> identityKinds, defaultPresets, indexMethods, fkActions;
  final List<DbDesignOption> options;
  final int maxIdentifier;

  bool has(String feature) => features.contains(feature);
  bool get collection => has('collection');
  bool get schemas => has('schemas');

  factory DbDesignDialect.fromJson(DbJson json) => DbDesignDialect(
    engine: DbEngine.parse(json['engine'] as String),
    features: {
      for (final MapEntry(:key, :value) in (json['features'] as Map? ?? const {}).entries)
        if (value == true) key as String,
    },
    types: [for (final t in (json['types'] as List? ?? const [])) DbDesignType.fromJson(t as DbJson)],
    defaultType: json['default_type'] as String? ?? '',
    identityKinds: _strings(json['identity_kinds']),
    defaultPresets: _strings(json['default_presets']),
    indexMethods: _strings(json['index_methods']),
    fkActions: _strings(json['fk_actions']),
    options: [for (final o in (json['options'] as List? ?? const [])) DbDesignOption.fromJson(o as DbJson)],
    maxIdentifier: json['max_identifier'] as int? ?? 63,
  );

  DbDesignType? type(String name) {
    final lower = name.trim().toLowerCase();
    return types.where((t) => t.name.toLowerCase() == lower).firstOrNull;
  }
}

final class DbDesignIssue {
  const DbDesignIssue({required this.error, required this.code, required this.target, this.value = ''});
  final bool error;
  final String code, target, value;
  factory DbDesignIssue.fromJson(DbJson json) => DbDesignIssue(
    error: json['severity'] == 'error',
    code: json['code'] as String,
    target: json['target'] as String? ?? '',
    value: json['value'] as String? ?? '',
  );
}

final class DbDesignStatement {
  const DbDesignStatement({required this.sql, this.destructive = false});
  final String sql;
  final bool destructive;
}

final class DbDesignPlan {
  const DbDesignPlan({
    this.statements = const [],
    this.script = '',
    this.issues = const [],
    this.transactional = false,
  });
  final List<DbDesignStatement> statements;
  final String script;
  final List<DbDesignIssue> issues;
  final bool transactional;
  bool get hasErrors => issues.any((i) => i.error);
  bool get destructive => statements.any((s) => s.destructive);
  factory DbDesignPlan.fromJson(DbJson json) => DbDesignPlan(
    statements: [
      for (final s in (json['statements'] as List? ?? const []))
        DbDesignStatement(sql: (s as DbJson)['sql'] as String, destructive: s['destructive'] as bool? ?? false),
    ],
    script: json['script'] as String? ?? '',
    issues: [for (final i in (json['issues'] as List? ?? const [])) DbDesignIssue.fromJson(i as DbJson)],
    transactional: json['transactional'] as bool? ?? false,
  );
}

enum DbDesignStatementStatus { done, failed, rolledBack, skipped }

final class DbDesignStatementResult {
  const DbDesignStatementResult({required this.status, this.errorCode});
  final DbDesignStatementStatus status;
  final String? errorCode;
}

final class DbDesignApplyResult {
  const DbDesignApplyResult({required this.statements, required this.committed});
  final List<DbDesignStatementResult> statements;
  final bool committed;
  factory DbDesignApplyResult.fromJson(DbJson json) => DbDesignApplyResult(
    committed: json['committed'] as bool? ?? false,
    statements: [
      for (final s in (json['statements'] as List? ?? const []))
        DbDesignStatementResult(
          status: switch ((s as DbJson)['status']) {
            'done' => DbDesignStatementStatus.done,
            'failed' => DbDesignStatementStatus.failed,
            'rolled_back' => DbDesignStatementStatus.rolledBack,
            _ => DbDesignStatementStatus.skipped,
          },
          errorCode: s['error_code'] as String?,
        ),
    ],
  );
}

/// Optional backend capability: the table designer. Planning is pure (no
/// session); applying takes a request that went through the review (its
/// ticket) and the vault passphrase confirmation.
abstract interface class DbTableDesignService {
  Future<DbDesignDialect> designDialect(DbEngine engine);
  Future<DbDesignPlan> planDesign(DbEngine engine, DbTableDesign? original, DbTableDesign design);
  Future<DbTableDesign> loadTableDesign(String sessionId, DbObjectRef table);
  Future<DbDesignApplyResult> applyDesign(String sessionId, DbQueryRequest request);
}

/// Engines with a designer dialect in the core (a new dialect is added here
/// too).
const dbDesignerEngines = {
  DbEngine.postgresql,
  DbEngine.mysql,
  DbEngine.mariadb,
  DbEngine.clickhouse,
  DbEngine.mongodb,
};

/// Engines whose tables (or collections) can be designed.
bool dbDesignerSupported(DbEngine engine) => dbDesignerEngines.contains(engine);
