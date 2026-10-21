import 'package:consolecrypt/core/security/secret_text.dart';

typedef DbJson = Map<String, dynamic>;

enum DbEngine {
  postgresql('postgresql', 'PostgreSQL', 5432),
  mysql('mysql', 'MySQL', 3306),
  mariadb('mariadb', 'MariaDB', 3306),
  clickhouse('clickhouse', 'ClickHouse', 8443),
  redis('redis', 'Redis', 6379),
  mongodb('mongodb', 'MongoDB', 27017);

  const DbEngine(this.wireName, this.label, this.port);
  final String wireName, label;
  final int port;
  bool get sql => this != redis && this != mongodb;
  static DbEngine parse(String value) => values.firstWhere((e) => e.wireName == value);
}

enum DbTlsMode { disable, prefer, require, verify }

enum DbRouteKind { direct, ssh, jump, proxy, kubernetes, tunnel, docker }

/// What a Kubernetes route forwards to (the kind names are API identifiers).
enum DbKubeTargetKind {
  pod('pod', 'Pod'), // l10n-ignore: Kubernetes kind
  service('service', 'Service'), // l10n-ignore: Kubernetes kind
  deployment('deployment', 'Deployment'), // l10n-ignore: Kubernetes kind
  statefulSet('stateful_set', 'StatefulSet'); // l10n-ignore: Kubernetes kind

  const DbKubeTargetKind(this.wireName, this.label);
  final String wireName, label;
  static DbKubeTargetKind parse(String? value) => values.firstWhere((e) => e.wireName == value, orElse: () => pod);
}

enum DbAuthMechanism {
  defaultAuth('default'),
  scramSha256('scram_sha256'),
  scramSha1('scram_sha1'),
  mongoX509('mongo_x509');

  const DbAuthMechanism(this.wireName);
  final String wireName;
}

enum DbMongoReadPreference {
  primary('primary'),
  primaryPreferred('primary_preferred'),
  secondary('secondary'),
  secondaryPreferred('secondary_preferred'),
  nearest('nearest');

  const DbMongoReadPreference(this.wireName);
  final String wireName;
}

final class DbTls {
  const DbTls({
    this.mode = DbTlsMode.verify,
    this.certificateAuthorityPem,
    this.serverName,
    this.allowUnboundAuth = false,
  });
  final DbTlsMode mode;
  final String? certificateAuthorityPem, serverName;

  /// PostgreSQL "require": allow a password without SCRAM channel binding.
  final bool allowUnboundAuth;
  factory DbTls.fromJson(DbJson json) => DbTls(
    mode: DbTlsMode.values.byName(json['mode'] as String? ?? 'verify'),
    certificateAuthorityPem: json['certificate_authority_pem'] as String?,
    serverName: json['server_name'] as String?,
    allowUnboundAuth: json['allow_unbound_auth'] as bool? ?? false,
  );
  DbJson toJson() => {
    'mode': mode.name,
    'certificate_authority_pem': certificateAuthorityPem,
    'server_name': serverName,
    // Omitted when off: older clients reject unknown fields.
    if (allowUnboundAuth) 'allow_unbound_auth': true,
  };
}

/// How the database socket is reached. [reference] is the SSH host, jump
/// profile, proxy, Kubernetes context or saved tunnel id, or the SSH host of a
/// Docker engine (null: this computer). Kubernetes and Docker routes also name
/// the forwarded workload or container; the connection port is its container
/// port. These forwards are restored by the core on connect.
final class DbRoute {
  const DbRoute({
    this.kind = DbRouteKind.direct,
    this.reference,
    this.namespace = '',
    this.targetKind = DbKubeTargetKind.pod,
    this.targetName = '',
    this.container = '',
  });
  final DbRouteKind kind;
  final String? reference;
  final String namespace, targetName, container;
  final DbKubeTargetKind targetKind;

  static String _referenceKey(DbRouteKind kind) => switch (kind) {
    DbRouteKind.direct => '',
    DbRouteKind.ssh => 'host_id',
    DbRouteKind.jump => 'jump_profile_id',
    DbRouteKind.proxy => 'proxy_id',
    DbRouteKind.kubernetes => 'context_id',
    DbRouteKind.tunnel => 'tunnel_id',
    DbRouteKind.docker => 'host_id',
  };

  factory DbRoute.fromJson(DbJson json) {
    final kind = DbRouteKind.values.byName(json['kind'] as String? ?? 'direct');
    return DbRoute(
      kind: kind,
      reference: json[_referenceKey(kind)] as String?,
      namespace: json['namespace'] as String? ?? '',
      targetKind: DbKubeTargetKind.parse(json['target_kind'] as String?),
      targetName: json['target_name'] as String? ?? '',
      container: json['container'] as String? ?? '',
    );
  }
  DbJson toJson() => {
    'kind': kind.name,
    if (kind != DbRouteKind.direct) _referenceKey(kind): reference,
    if (kind == DbRouteKind.kubernetes) ...{
      'namespace': namespace,
      'target_kind': targetKind.wireName,
      'target_name': targetName,
    },
    if (kind == DbRouteKind.docker) 'container': container,
  };

  /// A forward the core restores on connect (no fixed local port is saved).
  bool get restorable => const {DbRouteKind.kubernetes, DbRouteKind.tunnel, DbRouteKind.docker}.contains(kind);

  /// Same route and, for Kubernetes, the same forwarded workload.
  bool sameAs(DbRoute other) =>
      kind == other.kind &&
      (kind == DbRouteKind.direct || reference == other.reference) &&
      (kind != DbRouteKind.kubernetes ||
          (namespace == other.namespace && targetKind == other.targetKind && targetName == other.targetName)) &&
      (kind != DbRouteKind.docker || container == other.container);
}

final class DbAuth {
  const DbAuth({
    this.passwordSecretId,
    this.certificatePem,
    this.keySecretId,
    this.mechanism = DbAuthMechanism.defaultAuth,
  });
  final String? passwordSecretId, certificatePem, keySecretId;
  final DbAuthMechanism mechanism;
  factory DbAuth.fromJson(DbJson json) {
    final certificate = json['client_certificate'] as DbJson?;
    return DbAuth(
      passwordSecretId: json['password_secret_id'] as String?,
      certificatePem: certificate?['certificate_pem'] as String?,
      keySecretId: certificate?['key_secret_id'] as String?,
      mechanism: DbAuthMechanism.values.firstWhere((e) => e.wireName == (json['mechanism'] ?? 'default')),
    );
  }
  DbJson toJson() => {
    'password_secret_id': passwordSecretId,
    'mechanism': mechanism.wireName,
    'client_certificate': certificatePem == null
        ? null
        : {'certificate_pem': certificatePem, 'key_secret_id': keySecretId},
  };
}

final class DbLimits {
  const DbLimits({
    this.maxRows = 1000,
    this.statementTimeoutSeconds = 60,
    this.connectTimeoutSeconds = 10,
    this.resultMemoryBytes = 64 * 1024 * 1024,
  });
  final int maxRows, statementTimeoutSeconds, connectTimeoutSeconds, resultMemoryBytes;
  factory DbLimits.fromJson(DbJson json) => DbLimits(
    maxRows: json['max_rows'] as int,
    statementTimeoutSeconds: json['statement_timeout_seconds'] as int,
    connectTimeoutSeconds: json['connect_timeout_seconds'] as int,
    resultMemoryBytes: json['result_memory_bytes'] as int,
  );
  DbJson toJson() => {
    'max_rows': maxRows,
    'statement_timeout_seconds': statementTimeoutSeconds,
    'connect_timeout_seconds': connectTimeoutSeconds,
    'result_memory_bytes': resultMemoryBytes,
  };
}

/// Only documented driver settings can be emitted; arbitrary driver URL options
/// (including switches that weaken TLS or enable file access) are not accepted.
final class DbDriverOptions {
  const DbDriverOptions({
    required this.engine,
    this.applicationName = 'ConsoleCrypt',
    this.searchPath = const [],
    this.charset = 'utf8mb4',
    this.settings = const {},
    this.resp3 = false,
    this.authSource,
    this.replicaSet,
    this.srv = false,
    this.readPreference = DbMongoReadPreference.secondaryPreferred,
  });
  final DbEngine engine;
  final String applicationName, charset;
  final List<String> searchPath;
  final Map<String, int> settings;
  final bool resp3, srv;
  final String? authSource, replicaSet;
  final DbMongoReadPreference readPreference;
  factory DbDriverOptions.fromJson(DbJson json) => DbDriverOptions(
    engine: DbEngine.parse(json['kind'] as String),
    applicationName: json['application_name'] as String? ?? 'ConsoleCrypt',
    searchPath: (json['search_path'] as List? ?? const []).cast<String>(),
    charset: json['charset'] as String? ?? 'utf8mb4',
    settings: (json['settings'] as Map? ?? const {}).cast<String, int>(),
    resp3: json['resp3'] as bool? ?? false,
    authSource: json['auth_source'] as String?,
    replicaSet: json['replica_set'] as String?,
    srv: json['srv'] as bool? ?? false,
    readPreference: DbMongoReadPreference.values.firstWhere(
      (e) => e.wireName == (json['read_preference'] ?? 'secondary_preferred'),
    ),
  );
  DbJson toJson() => {
    'kind': engine.wireName,
    ...switch (engine) {
      DbEngine.postgresql => {'application_name': applicationName, 'search_path': searchPath},
      DbEngine.mysql || DbEngine.mariadb => {'charset': charset},
      DbEngine.clickhouse => {'settings': settings},
      DbEngine.redis => {'resp3': resp3},
      DbEngine.mongodb => {
        'auth_source': authSource,
        'replica_set': replicaSet,
        'srv': srv,
        'read_preference': readPreference.wireName,
      },
    },
  };
}

/// Metadata only. Passwords, private keys and connection URLs never belong here.
final class DbConnection {
  const DbConnection({
    required this.id,
    required this.name,
    required this.engine,
    required this.host,
    required this.port,
    required this.options,
    this.database = '',
    this.username = '',
    this.auth = const DbAuth(),
    this.tls = const DbTls(),
    this.route = const DbRoute(),
    this.production = true,
    this.limits = const DbLimits(),
    this.groupId,
    this.tags = const [],
    this.color,
    this.createdAt,
    this.updatedAt,
  });
  final String id, name, host, database, username;
  final DbEngine engine;
  final int port;
  final DbAuth auth;
  final DbTls tls;
  final DbRoute route;
  final bool production;
  final DbLimits limits;
  final DbDriverOptions options;
  final String? groupId, color;
  final List<String> tags;
  final DateTime? createdAt, updatedAt;
  factory DbConnection.fromJson(DbJson json) => DbConnection(
    id: json['id'] as String,
    name: json['name'] as String,
    engine: DbEngine.parse(json['engine'] as String),
    host: json['host'] as String,
    port: json['port'] as int,
    database: json['database'] as String? ?? '',
    username: json['username'] as String? ?? '',
    auth: DbAuth.fromJson(json['auth'] as DbJson? ?? {}),
    tls: DbTls.fromJson(json['tls'] as DbJson? ?? {}),
    route: DbRoute.fromJson(json['route'] as DbJson? ?? {}),
    production: json['production'] as bool? ?? true,
    limits: json['limits'] == null ? const DbLimits() : DbLimits.fromJson(json['limits'] as DbJson),
    options: DbDriverOptions.fromJson(json['options'] as DbJson),
    groupId: json['group_id'] as String?,
    tags: (json['tags'] as List? ?? const []).cast<String>(),
    color: json['color'] as String?,
    createdAt: DateTime.tryParse(json['created_at'] as String? ?? ''),
    updatedAt: DateTime.tryParse(json['updated_at'] as String? ?? ''),
  );
  DbJson toJson() => {
    'id': id,
    'name': name,
    'engine': engine.wireName,
    'host': host,
    'port': port,
    'database': database,
    'username': username,
    'auth': auth.toJson(),
    'tls': tls.toJson(),
    'route': route.toJson(),
    'production': production,
    'limits': limits.toJson(),
    'options': options.toJson(),
    'group_id': groupId,
    'tags': tags,
    'color': color,
    if (createdAt != null) 'created_at': createdAt!.toUtc().toIso8601String(),
    if (updatedAt != null) 'updated_at': updatedAt!.toUtc().toIso8601String(),
  };
}

/// An empty metadata ID means create; native code assigns IDs and timestamps.
/// Null secret fields preserve existing references; explicit clearing is separate.
final class DbConnectionDraft {
  const DbConnectionDraft({
    required this.metadata,
    this.password,
    this.privateKey,
    this.savePassword = true,
    this.clearPassword = false,
    this.clearCertificate = false,
  });
  final DbConnection metadata;
  final SecretText? password, privateKey;
  final bool savePassword, clearPassword, clearCertificate;
  void wipe() {
    password?.wipe();
    privateKey?.wipe();
  }

  @override
  String toString() => 'DbConnectionDraft(<redacted>)';
}

enum DbCellKind { nullValue, boolValue, intValue, uint, decimal, float, text, date, time, timestamp, binary, json }

extension DbCellKindWire on DbCellKind {
  String get wireName => DbCell._wire[index];
}

final class DbCell {
  const DbCell(this.kind, this.value);
  const DbCell.nullValue() : kind = DbCellKind.nullValue, value = null;
  final DbCellKind kind;

  /// Bool or String; all numeric values retain exact wire precision.
  final Object? value;
  static const _wire = [
    'null',
    'bool',
    'int',
    'uint',
    'decimal',
    'float',
    'text',
    'date',
    'time',
    'timestamp',
    'binary',
    'json',
  ];
  bool get numeric => const [DbCellKind.intValue, DbCellKind.uint, DbCellKind.decimal, DbCellKind.float].contains(kind);
  String get text => value?.toString() ?? '';
  factory DbCell.fromJson(DbJson json) {
    final index = _wire.indexOf(json['kind'] as String);
    if (index < 0) {
      throw const FormatException('Unsupported database cell kind');
    }
    final kind = DbCellKind.values[index], value = json['value'];
    if (kind == DbCellKind.nullValue
        ? value != null
        : kind == DbCellKind.boolValue
        ? value is! bool
        : value is! String) {
      throw const FormatException('Invalid typed database cell');
    }
    return DbCell(kind, value);
  }
  DbJson toJson() => {'kind': _wire[kind.index], if (kind != DbCellKind.nullValue) 'value': value};
  @override
  String toString() => 'DbCell(<redacted>)';
}

final class DbColumn {
  const DbColumn({
    required this.name,
    required this.typeName,
    this.nullable = true,
    this.sensitive = false,
    this.primaryKey = false,
  });
  final String name, typeName;
  final bool nullable, sensitive, primaryKey;
  factory DbColumn.fromJson(DbJson json) => DbColumn(
    name: json['name'] as String,
    typeName: json['type_name'] as String,
    nullable: json['nullable'] as bool,
    sensitive: json['sensitive'] as bool,
    primaryKey: json['primary_key'] as bool? ?? false,
  );
}

enum DbServerReadOnly { enforced, roleVerified, unverified }

/// What protects a live session: verified TLS, TLS without certificate
/// verification, no TLS inside a tunnel (or on this computer), or nothing.
enum DbTransportSecurity {
  verified,
  unverified,
  tunnel,
  plaintext;

  static DbTransportSecurity? parse(Object? value) => values.where((e) => e.name == value).firstOrNull;

  /// What a saved connection gets before it is opened, or null when only the
  /// server can tell ("prefer" inside a tunnel or on this computer).
  static DbTransportSecurity? expected(DbConnection connection, {required bool loopback}) {
    final protected = connection.route.kind != DbRouteKind.direct && connection.route.kind != DbRouteKind.proxy;
    final local = protected || (connection.route.kind == DbRouteKind.direct && loopback);
    return switch (connection.tls.mode) {
      DbTlsMode.verify => verified,
      DbTlsMode.require => unverified,
      DbTlsMode.prefer => local ? null : verified,
      DbTlsMode.disable => local ? tunnel : plaintext,
    };
  }
}

final class DbSessionInfo {
  const DbSessionInfo({
    required this.id,
    required this.connectionId,
    required this.engine,
    required this.serverVersion,
    required this.production,
    required this.readOnly,
    required this.serverReadOnly,
    this.transportSecurity,
  });
  final String id, connectionId, serverVersion;
  final DbEngine engine;
  final bool production, readOnly;
  final DbServerReadOnly serverReadOnly;
  final DbTransportSecurity? transportSecurity;
  factory DbSessionInfo.fromJson(DbJson json) => DbSessionInfo(
    id: json['id'] as String,
    connectionId: json['connection_id'] as String,
    engine: DbEngine.parse(json['engine'] as String),
    serverVersion: json['server_version'] as String,
    production: json['production'] as bool,
    readOnly: json['read_only'] as bool,
    serverReadOnly: switch (json['server_read_only']) {
      'enforced' => DbServerReadOnly.enforced,
      'role_verified' => DbServerReadOnly.roleVerified,
      _ => DbServerReadOnly.unverified,
    },
    transportSecurity: DbTransportSecurity.parse(json['transport_security']),
  );
}

enum DbObjectKind {
  database('database'),
  schema('schema'),
  table('table'),
  view('view'),
  materializedView('materialized_view'),
  sequence('sequence'),
  function('function'),
  procedure('procedure'),
  column('column'),
  indexObject('index'),
  constraint('constraint'),
  trigger('trigger'),
  dictionary('dictionary'),
  projection('projection'),
  redisDatabase('redis_database'),
  redisKey('redis_key'),
  collection('collection');

  const DbObjectKind(this.wireName);
  final String wireName;
}

final class DbObjectRef {
  const DbObjectRef({required this.database, required this.kind, required this.name, this.schema, this.parentName});
  final String database, name;
  final String? schema, parentName;
  final DbObjectKind kind;
  // Length prefixes prevent collisions even when identifiers contain separators.
  String get id => [database, schema ?? '', kind.wireName, name, parentName ?? ''].map((s) => '${s.length}:$s').join();
  factory DbObjectRef.fromJson(DbJson json) => DbObjectRef(
    database: json['database'] as String,
    schema: json['schema'] as String?,
    kind: DbObjectKind.values.firstWhere((e) => e.wireName == json['kind']),
    name: json['name'] as String,
    parentName: json['parent_name'] as String?,
  );
  DbJson toJson() => {
    'database': database,
    'schema': schema,
    'kind': kind.wireName,
    'name': name,
    'parent_name': parentName,
  };
}

final class DbTreeNode {
  const DbTreeNode({
    required this.reference,
    required this.hasChildren,
    this.detail,
    this.nullable,
    this.defaultValue,
    this.primaryKey = false,
    this.foreignKey,
  });
  final DbObjectRef reference;
  final bool hasChildren, primaryKey;
  final bool? nullable;
  final String? detail, defaultValue;
  final DbObjectRef? foreignKey;
  factory DbTreeNode.fromJson(DbJson json) => DbTreeNode(
    reference: DbObjectRef.fromJson(json['reference'] as DbJson),
    hasChildren: json['has_children'] as bool,
    detail: json['detail'] as String?,
    nullable: json['nullable'] as bool?,
    defaultValue: json['default_value'] as String?,
    primaryKey: json['primary_key'] as bool? ?? false,
    foreignKey: json['foreign_key'] == null ? null : DbObjectRef.fromJson(json['foreign_key'] as DbJson),
  );
}

final class DbTreePage {
  const DbTreePage({required this.nodes, this.nextCursor});
  final List<DbTreeNode> nodes;
  final String? nextCursor;
  factory DbTreePage.fromJson(DbJson json) => DbTreePage(
    nodes: (json['nodes'] as List).map((e) => DbTreeNode.fromJson(e as DbJson)).toList(growable: false),
    nextCursor: json['next_cursor'] as String?,
  );
}

final class DbParameter {
  const DbParameter({required this.name, required this.value, this.typeHint});
  final String name;
  final DbCell value;
  final String? typeHint;
  DbJson toJson() => {'name': name, 'value': value.toJson(), 'type_hint': typeHint};
  @override
  String toString() => 'DbParameter(<redacted>)';
}

final class DbQueryRequest {
  const DbQueryRequest({required this.text, this.parameters = const [], this.approvalTicket});
  final String text;
  final List<DbParameter> parameters;
  final String? approvalTicket;
  DbJson toJson() => {
    'text': text,
    'parameters': parameters.map((p) => p.toJson()).toList(),
    'approval_ticket': approvalTicket,
  };
  @override
  String toString() => 'DbQueryRequest(<redacted>)';
}

enum DbQueryEventKind { started, rows, notice, paused, statementDone, done, cancelled, error }

final class DbQueryEvent {
  const DbQueryEvent({
    required this.kind,
    this.resultIndex = 0,
    this.columns = const [],
    this.editableTable,
    this.rows = const [],
    this.message,
    this.loadedRows = 0,
    this.hasMore = false,
    this.elapsedMs = 0,
    this.affectedRows,
    this.truncated = false,
    this.code,
  });
  final DbQueryEventKind kind;
  final int resultIndex, loadedRows, elapsedMs;
  final List<DbColumn> columns;
  final DbObjectRef? editableTable;
  final List<List<DbCell>> rows;
  final String? message, code;
  final int? affectedRows;
  final bool hasMore, truncated;
  factory DbQueryEvent.fromJson(DbJson json) => DbQueryEvent(
    kind: switch (json['kind']) {
      'statement_done' => DbQueryEventKind.statementDone,
      final String value => DbQueryEventKind.values.byName(value),
      _ => throw const FormatException('Invalid database query event'),
    },
    resultIndex: json['result_index'] as int? ?? 0,
    columns: (json['columns'] as List? ?? const []).map((e) => DbColumn.fromJson(e as DbJson)).toList(growable: false),
    editableTable: json['editable_table'] == null ? null : DbObjectRef.fromJson(json['editable_table'] as DbJson),
    rows: (json['rows'] as List? ?? const [])
        .map((row) => (row as List).map((cell) => DbCell.fromJson(cell as DbJson)).toList(growable: false))
        .toList(growable: false),
    message: json['message'] as String?,
    loadedRows: json['loaded_rows'] as int? ?? 0,
    hasMore: json['has_more'] as bool? ?? false,
    elapsedMs: json['elapsed_ms'] as int? ?? 0,
    affectedRows: json['affected_rows'] as int?,
    truncated: json['truncated'] as bool? ?? false,
    code: json['code'] as String?,
  );
  @override
  String toString() => 'DbQueryEvent(${kind.name}, <redacted>)';
}

enum DbExportFormat { csv, tsv, json, ndjson, markdown, insert }

final class DbExportOptions {
  const DbExportOptions({
    required this.format,
    this.header = true,
    this.bom = false,
    this.delimiter = ',',
    this.quote = '"',
  });
  final DbExportFormat format;
  final bool header, bom;
  final String delimiter, quote;
  DbJson toJson() => {'format': format.name, 'header': header, 'bom': bom, 'delimiter': delimiter, 'quote': quote};
}

final class DbExportProgress {
  const DbExportProgress({
    required this.id,
    this.rows = 0,
    this.bytes = 0,
    this.done = false,
    this.cancelled = false,
    this.error,
  });
  final String id;
  final int rows, bytes;
  final bool done, cancelled;
  final String? error;
  factory DbExportProgress.fromJson(DbJson json) => DbExportProgress(
    id: json['id'] as String,
    rows: json['rows'] as int,
    bytes: json['bytes'] as int,
    done: json['done'] as bool,
    cancelled: json['cancelled'] as bool,
    error: json['error'] as String?,
  );
}
