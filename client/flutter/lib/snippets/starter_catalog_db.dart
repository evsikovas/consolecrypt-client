// Query text was checked against PostgreSQL 17, MySQL 8.4, ClickHouse 25.8,
// Redis 7.4 and MongoDB 7.0 (mongosh); keep the per-package counts in
// test/unit/snippet_catalog_test.dart in sync when adding entries.
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/snippets/starter_catalog.dart';

/// Tag on every database starter snippet (the Databases history uses it too).
const databaseSnippetTag = 'database';

/// Tag on snippets the Databases workspace guard refuses (server admin
/// commands); they are meant for the client CLI in a terminal session.
const terminalOnlySnippetTag = 'terminal-only';

typedef _Pack = ({String name, SnippetType type, String? shell, String client});

/// Database starter packages. Type and dialect (`shell`) follow
/// `db_save_history_snippet`: PostgreSQL → postgresql; MySQL / MariaDB /
/// ClickHouse → sql + "mysql" / "clickhouse"; Redis → redis_cli; MongoDB →
/// shell + "mongosh". Query text is never localized.
List<StarterSnippetPackage> databaseStarterPackages(AppLocalizations l, StarterEntry entry) {
  Snippet db(
    _Pack pack,
    String id,
    String name,
    String note,
    List<String> lines, {
    List<SnippetVariable> variables = const [],
    RiskLevel risk = RiskLevel.readOnly,
    bool terminalOnly = false,
  }) => entry(
    id,
    name,
    pack.name,
    pack.type,
    lines.join('\n'),
    variables: variables,
    risk: risk,
    shell: pack.shell,
    description: terminalOnly
        ? l.snippetStarterDbTerminalOrigin(note, pack.client, pack.name)
        : l.snippetStarterDbOrigin(note, pack.client, pack.name),
    tags: terminalOnly ? const [databaseSnippetTag, terminalOnlySnippetTag] : const [databaseSnippetTag],
  );

  final _Pack postgresql = (name: l.snippetPackPostgresql, type: SnippetType.postgresql, shell: null, client: 'psql');
  final _Pack mysql = (name: l.snippetPackMysql, type: SnippetType.sql, shell: 'mysql', client: 'mysql');
  final _Pack clickhouse = (
    name: l.snippetPackClickhouse,
    type: SnippetType.sql,
    shell: 'clickhouse',
    client: 'clickhouse-client',
  );
  final _Pack redis = (name: l.snippetPackRedis, type: SnippetType.redisCli, shell: null, client: 'redis-cli');
  final _Pack mongodb = (name: l.snippetPackMongodb, type: SnippetType.shell, shell: 'mongosh', client: 'mongosh');
  return [
    StarterSnippetPackage('postgresql', postgresql.name, l.snippetPackPostgresqlDescription, [
      db(postgresql, 'db.pg.activity', l.snippetDbPgActivity, l.snippetDbPgActivityNote, [
        r'SELECT pid, usename, datname, application_name, client_addr, state,', // l10n-ignore: query text
        r'       now() - query_start AS duration, wait_event_type, wait_event,', // l10n-ignore: query text
        r'       left(query, 200) AS query', // l10n-ignore: query text
        r'FROM pg_catalog.pg_stat_activity', // l10n-ignore: query text
        r"WHERE state <> 'idle' AND pid <> pg_backend_pid()", // l10n-ignore: query text
        r'ORDER BY duration DESC NULLS LAST;', // l10n-ignore: query text
      ]),
      db(
        postgresql,
        'db.pg.long_running',
        l.snippetDbPgLongRunning,
        l.snippetDbPgLongRunningNote,
        [
          r'SELECT pid, usename, datname, state, now() - query_start AS duration,', // l10n-ignore: query text
          r'       wait_event_type, wait_event, left(query, 200) AS query', // l10n-ignore: query text
          r'FROM pg_catalog.pg_stat_activity', // l10n-ignore: query text
          r"WHERE state <> 'idle' AND pid <> pg_backend_pid()", // l10n-ignore: query text
          r"  AND query_start < now() - interval '1 second' * {{seconds}}", // l10n-ignore: query text
          r'ORDER BY duration DESC;', // l10n-ignore: query text
        ],
        variables: const [SnippetVariable(name: 'seconds', defaultValue: '60')],
      ),
      db(postgresql, 'db.pg.blocking', l.snippetDbPgBlocking, l.snippetDbPgBlockingNote, [
        r'WITH RECURSIVE tree AS (', // l10n-ignore: query text
        r'  SELECT a.pid, ARRAY[a.pid] AS path', // l10n-ignore: query text
        r'  FROM pg_catalog.pg_stat_activity a', // l10n-ignore: query text
        r'  WHERE cardinality(pg_blocking_pids(a.pid)) = 0', // l10n-ignore: query text
        r'    AND EXISTS (SELECT 1 FROM pg_catalog.pg_stat_activity w WHERE a.pid = ANY (pg_blocking_pids(w.pid)))', // l10n-ignore: query text
        r'  UNION ALL', // l10n-ignore: query text
        r'  SELECT w.pid, t.path || w.pid', // l10n-ignore: query text
        r'  FROM tree t', // l10n-ignore: query text
        r'  JOIN pg_catalog.pg_stat_activity w ON t.pid = ANY (pg_blocking_pids(w.pid))', // l10n-ignore: query text
        r'  WHERE NOT w.pid = ANY (t.path)', // l10n-ignore: query text
        r')', // l10n-ignore: query text
        r"SELECT repeat('  ', cardinality(t.path) - 1) || t.pid AS pid_tree,", // l10n-ignore: query text
        r'       a.usename, a.state, a.wait_event_type, a.wait_event,', // l10n-ignore: query text
        r'       now() - a.query_start AS duration, left(a.query, 120) AS query', // l10n-ignore: query text
        r'FROM tree t', // l10n-ignore: query text
        r'JOIN pg_catalog.pg_stat_activity a ON a.pid = t.pid', // l10n-ignore: query text
        r'ORDER BY t.path;', // l10n-ignore: query text
      ]),
      db(
        postgresql,
        'db.pg.table_sizes',
        l.snippetDbPgTableSizes,
        l.snippetDbPgTableSizesNote,
        [
          r'SELECT n.nspname AS schema_name, c.relname AS table_name,', // l10n-ignore: query text
          r'       pg_size_pretty(pg_total_relation_size(c.oid)) AS total,', // l10n-ignore: query text
          r'       pg_size_pretty(pg_relation_size(c.oid)) AS heap,', // l10n-ignore: query text
          r'       pg_size_pretty(pg_indexes_size(c.oid)) AS indexes,', // l10n-ignore: query text
          r'       pg_size_pretty(pg_total_relation_size(c.oid) - pg_relation_size(c.oid) - pg_indexes_size(c.oid)) AS toast_other,', // l10n-ignore: query text
          r'       c.reltuples::bigint AS approx_rows', // l10n-ignore: query text
          r'FROM pg_catalog.pg_class c', // l10n-ignore: query text
          r'JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace', // l10n-ignore: query text
          r"WHERE c.relkind IN ('r', 'm')", // l10n-ignore: query text
          r"  AND n.nspname NOT IN ('pg_catalog', 'information_schema')", // l10n-ignore: query text
          r"  AND n.nspname NOT LIKE 'pg_toast%'", // l10n-ignore: query text
          r'ORDER BY pg_total_relation_size(c.oid) DESC', // l10n-ignore: query text
          r'LIMIT {{limit}};', // l10n-ignore: query text
        ],
        variables: const [SnippetVariable(name: 'limit', defaultValue: '20')],
      ),
      db(postgresql, 'db.pg.database_sizes', l.snippetDbPgDatabaseSizes, l.snippetDbPgDatabaseSizesNote, [
        r'SELECT datname, pg_size_pretty(pg_database_size(datname)) AS size', // l10n-ignore: query text
        r'FROM pg_catalog.pg_database', // l10n-ignore: query text
        r'WHERE datallowconn', // l10n-ignore: query text
        r'ORDER BY pg_database_size(datname) DESC;', // l10n-ignore: query text
      ]),
      db(
        postgresql,
        'db.pg.index_usage',
        l.snippetDbPgIndexUsage,
        l.snippetDbPgIndexUsageNote,
        [
          r'SELECT s.schemaname, s.relname AS table_name, s.indexrelname AS index_name, s.idx_scan,', // l10n-ignore: query text
          r'       pg_size_pretty(pg_relation_size(s.indexrelid)) AS size, i.indisunique AS is_unique', // l10n-ignore: query text
          r'FROM pg_catalog.pg_stat_user_indexes s', // l10n-ignore: query text
          r'JOIN pg_catalog.pg_index i ON i.indexrelid = s.indexrelid', // l10n-ignore: query text
          r'ORDER BY s.idx_scan, pg_relation_size(s.indexrelid) DESC', // l10n-ignore: query text
          r'LIMIT {{limit}};', // l10n-ignore: query text
        ],
        variables: const [SnippetVariable(name: 'limit', defaultValue: '50')],
      ),
      db(
        postgresql,
        'db.pg.dead_tuples',
        l.snippetDbPgDeadTuples,
        l.snippetDbPgDeadTuplesNote,
        [
          r'SELECT schemaname, relname AS table_name, n_live_tup, n_dead_tup,', // l10n-ignore: query text
          r'       round(100.0 * n_dead_tup / nullif(n_live_tup + n_dead_tup, 0), 1) AS dead_pct,', // l10n-ignore: query text
          r'       last_autovacuum, last_vacuum, last_autoanalyze, last_analyze,', // l10n-ignore: query text
          r'       autovacuum_count, n_mod_since_analyze', // l10n-ignore: query text
          r'FROM pg_catalog.pg_stat_user_tables', // l10n-ignore: query text
          r'ORDER BY n_dead_tup DESC', // l10n-ignore: query text
          r'LIMIT {{limit}};', // l10n-ignore: query text
        ],
        variables: const [SnippetVariable(name: 'limit', defaultValue: '20')],
      ),
      db(postgresql, 'db.pg.cache_hit', l.snippetDbPgCacheHit, l.snippetDbPgCacheHitNote, [
        r'SELECT datname, blks_hit, blks_read,', // l10n-ignore: query text
        r'       round(100.0 * blks_hit / nullif(blks_hit + blks_read, 0), 2) AS hit_pct', // l10n-ignore: query text
        r'FROM pg_catalog.pg_stat_database', // l10n-ignore: query text
        r'WHERE datname IS NOT NULL', // l10n-ignore: query text
        r'ORDER BY blks_hit + blks_read DESC;', // l10n-ignore: query text
      ]),
      db(postgresql, 'db.pg.replication', l.snippetDbPgReplication, l.snippetDbPgReplicationNote, [
        r'SELECT pid, usename, application_name, client_addr, state, sync_state,', // l10n-ignore: query text
        r'       pg_size_pretty(pg_wal_lsn_diff(', // l10n-ignore: query text
        r'         CASE WHEN pg_is_in_recovery() THEN pg_last_wal_replay_lsn() ELSE pg_current_wal_lsn() END,', // l10n-ignore: query text
        r'         replay_lsn)) AS replay_lag_size,', // l10n-ignore: query text
        r'       write_lag, flush_lag, replay_lag', // l10n-ignore: query text
        r'FROM pg_catalog.pg_stat_replication', // l10n-ignore: query text
        r'ORDER BY application_name;', // l10n-ignore: query text
      ]),
      db(postgresql, 'db.pg.slots', l.snippetDbPgSlots, l.snippetDbPgSlotsNote, [
        r'SELECT slot_name, slot_type, database, active, active_pid, wal_status,', // l10n-ignore: query text
        r'       pg_size_pretty(pg_wal_lsn_diff(', // l10n-ignore: query text
        r'         CASE WHEN pg_is_in_recovery() THEN pg_last_wal_replay_lsn() ELSE pg_current_wal_lsn() END,', // l10n-ignore: query text
        r'         restart_lsn)) AS retained_wal', // l10n-ignore: query text
        r'FROM pg_catalog.pg_replication_slots', // l10n-ignore: query text
        r'ORDER BY restart_lsn NULLS LAST;', // l10n-ignore: query text
      ]),
      db(postgresql, 'db.pg.connections', l.snippetDbPgConnections, l.snippetDbPgConnectionsNote, [
        r'SELECT datname, usename, application_name, state, count(*) AS connections', // l10n-ignore: query text
        r'FROM pg_catalog.pg_stat_activity', // l10n-ignore: query text
        r"WHERE backend_type = 'client backend'", // l10n-ignore: query text
        r'GROUP BY datname, usename, application_name, state', // l10n-ignore: query text
        r'ORDER BY connections DESC;', // l10n-ignore: query text
      ]),
      db(postgresql, 'db.pg.settings_changed', l.snippetDbPgSettingsChanged, l.snippetDbPgSettingsChangedNote, [
        r'SELECT name, setting, unit, source, sourcefile, pending_restart', // l10n-ignore: query text
        r'FROM pg_catalog.pg_settings', // l10n-ignore: query text
        r"WHERE source <> 'default'", // l10n-ignore: query text
        r'ORDER BY name;', // l10n-ignore: query text
      ]),
      db(
        postgresql,
        'db.pg.columns',
        l.snippetDbPgColumns,
        l.snippetDbPgColumnsNote,
        [
          r'SELECT c.ordinal_position AS pos, c.column_name, c.data_type,', // l10n-ignore: query text
          r'       c.character_maximum_length AS max_length, c.is_nullable, c.column_default,', // l10n-ignore: query text
          r"       col_description(format('%I.%I', c.table_schema, c.table_name)::regclass, c.ordinal_position) AS comment", // l10n-ignore: query text
          r'FROM information_schema.columns c', // l10n-ignore: query text
          r"WHERE c.table_schema = '{{schema}}' AND c.table_name = '{{table}}'", // l10n-ignore: query text
          r'ORDER BY c.ordinal_position;', // l10n-ignore: query text
        ],
        variables: const [
          SnippetVariable(name: 'schema', defaultValue: 'public'),
          SnippetVariable(name: 'table'),
        ],
      ),
      db(
        postgresql,
        'db.pg.foreign_keys',
        l.snippetDbPgForeignKeys,
        l.snippetDbPgForeignKeysNote,
        [
          r'SELECT conname AS constraint_name, conrelid::regclass AS table_name,', // l10n-ignore: query text
          r'       confrelid::regclass AS referenced_table, pg_get_constraintdef(oid) AS definition', // l10n-ignore: query text
          r'FROM pg_catalog.pg_constraint', // l10n-ignore: query text
          r"WHERE contype = 'f'", // l10n-ignore: query text
          r"  AND (conrelid = '{{schema}}.{{table}}'::regclass OR confrelid = '{{schema}}.{{table}}'::regclass)", // l10n-ignore: query text
          r'ORDER BY conrelid::regclass::text, conname;', // l10n-ignore: query text
        ],
        variables: const [
          SnippetVariable(name: 'schema', defaultValue: 'public'),
          SnippetVariable(name: 'table'),
        ],
      ),
      db(
        postgresql,
        'db.pg.sequences',
        l.snippetDbPgSequences,
        l.snippetDbPgSequencesNote,
        [
          r'SELECT schemaname, sequencename, data_type, last_value, max_value,', // l10n-ignore: query text
          r'       round(100.0 * last_value / max_value, 2) AS used_pct', // l10n-ignore: query text
          r'FROM pg_catalog.pg_sequences', // l10n-ignore: query text
          r'WHERE last_value IS NOT NULL AND increment_by > 0', // l10n-ignore: query text
          r'  AND 100.0 * last_value / max_value >= {{percent}}', // l10n-ignore: query text
          r'ORDER BY used_pct DESC;', // l10n-ignore: query text
        ],
        variables: const [SnippetVariable(name: 'percent', defaultValue: '50')],
      ),
      db(postgresql, 'db.pg.fk_without_index', l.snippetDbPgFkWithoutIndex, l.snippetDbPgFkWithoutIndexNote, [
        r'SELECT c.conrelid::regclass AS table_name, c.conname AS constraint_name,', // l10n-ignore: query text
        r'       pg_get_constraintdef(c.oid) AS definition', // l10n-ignore: query text
        r'FROM pg_catalog.pg_constraint c', // l10n-ignore: query text
        r"WHERE c.contype = 'f'", // l10n-ignore: query text
        r'  AND NOT EXISTS (', // l10n-ignore: query text
        r'    SELECT 1 FROM pg_catalog.pg_index i', // l10n-ignore: query text
        r'    WHERE i.indrelid = c.conrelid', // l10n-ignore: query text
        r'      AND (i.indkey::smallint[])[0:cardinality(c.conkey) - 1] @> c.conkey', // l10n-ignore: query text
        r'  )', // l10n-ignore: query text
        r'ORDER BY c.conrelid::regclass::text, c.conname;', // l10n-ignore: query text
      ]),
      db(
        postgresql,
        'db.pg.statements',
        l.snippetDbPgStatements,
        l.snippetDbPgStatementsNote,
        [
          r'-- requires pg_stat_statements: shared_preload_libraries + CREATE EXTENSION pg_stat_statements;', // l10n-ignore: query text
          r'SELECT round(total_exec_time::numeric, 1) AS total_ms, calls,', // l10n-ignore: query text
          r'       round(mean_exec_time::numeric, 2) AS mean_ms, rows,', // l10n-ignore: query text
          r'       round(100.0 * shared_blks_hit / nullif(shared_blks_hit + shared_blks_read, 0), 1) AS hit_pct,', // l10n-ignore: query text
          r'       left(query, 200) AS query', // l10n-ignore: query text
          r'FROM pg_stat_statements', // l10n-ignore: query text
          r'ORDER BY total_exec_time DESC', // l10n-ignore: query text
          r'LIMIT {{limit}};', // l10n-ignore: query text
        ],
        variables: const [SnippetVariable(name: 'limit', defaultValue: '20')],
      ),
      db(
        postgresql,
        'db.pg.explain_analyze',
        l.snippetDbPgExplainAnalyze,
        l.snippetDbPgExplainAnalyzeNote,
        [
          r'EXPLAIN (ANALYZE, BUFFERS)', // l10n-ignore: query text
          r"SELECT * FROM {{table}} WHERE {{column}} = '{{value}}';", // l10n-ignore: query text
        ],
        variables: const [
          SnippetVariable(name: 'table'),
          SnippetVariable(name: 'column'),
          SnippetVariable(name: 'value'),
        ],
        risk: RiskLevel.modifying,
      ),
      db(
        postgresql,
        'db.pg.cancel',
        l.snippetDbPgCancel,
        l.snippetDbPgCancelNote,
        [
          r'SELECT pg_cancel_backend({{pid}});', // l10n-ignore: query text
        ],
        variables: const [SnippetVariable(name: 'pid')],
        risk: RiskLevel.modifying,
      ),
      db(
        postgresql,
        'db.pg.terminate',
        l.snippetDbPgTerminate,
        l.snippetDbPgTerminateNote,
        [
          r'SELECT pg_terminate_backend({{pid}});', // l10n-ignore: query text
        ],
        variables: const [SnippetVariable(name: 'pid')],
        risk: RiskLevel.destructive,
      ),
    ]),
    StarterSnippetPackage('mysql', mysql.name, l.snippetPackMysqlDescription, [
      db(mysql, 'db.mysql.processlist', l.snippetDbMysqlProcesslist, l.snippetDbMysqlProcesslistNote, [
        r'SELECT id, user, host, db, command, time, state, LEFT(info, 200) AS query', // l10n-ignore: query text
        r'FROM information_schema.processlist', // l10n-ignore: query text
        r"WHERE command <> 'Sleep' AND id <> CONNECTION_ID()", // l10n-ignore: query text
        r'ORDER BY time DESC;', // l10n-ignore: query text
      ]),
      db(
        mysql,
        'db.mysql.long_running',
        l.snippetDbMysqlLongRunning,
        l.snippetDbMysqlLongRunningNote,
        [
          r'SELECT id, user, host, db, command, time, state, LEFT(info, 200) AS query', // l10n-ignore: query text
          r'FROM information_schema.processlist', // l10n-ignore: query text
          r"WHERE command <> 'Sleep' AND id <> CONNECTION_ID() AND time >= {{seconds}}", // l10n-ignore: query text
          r'ORDER BY time DESC;', // l10n-ignore: query text
        ],
        variables: const [SnippetVariable(name: 'seconds', defaultValue: '60')],
      ),
      db(mysql, 'db.mysql.lock_waits', l.snippetDbMysqlLockWaits, l.snippetDbMysqlLockWaitsNote, [
        r'SELECT wait_age, locked_table, locked_index, locked_type,', // l10n-ignore: query text
        r'       waiting_pid, LEFT(waiting_query, 120) AS waiting_query,', // l10n-ignore: query text
        r'       blocking_pid, LEFT(blocking_query, 120) AS blocking_query', // l10n-ignore: query text
        r'FROM sys.innodb_lock_waits', // l10n-ignore: query text
        r'ORDER BY wait_age DESC;', // l10n-ignore: query text
      ]),
      db(mysql, 'db.mysql.transactions', l.snippetDbMysqlTransactions, l.snippetDbMysqlTransactionsNote, [
        r'SELECT trx_id, trx_mysql_thread_id AS thread_id, trx_state, trx_started,', // l10n-ignore: query text
        r'       TIMESTAMPDIFF(SECOND, trx_started, NOW()) AS age_s,', // l10n-ignore: query text
        r'       trx_rows_locked, trx_rows_modified, LEFT(trx_query, 200) AS query', // l10n-ignore: query text
        r'FROM information_schema.innodb_trx', // l10n-ignore: query text
        r'ORDER BY trx_started;', // l10n-ignore: query text
      ]),
      db(
        mysql,
        'db.mysql.table_sizes',
        l.snippetDbMysqlTableSizes,
        l.snippetDbMysqlTableSizesNote,
        [
          r'SELECT table_schema, table_name, engine, table_rows,', // l10n-ignore: query text
          r'       ROUND(data_length / 1048576, 1) AS data_mb,', // l10n-ignore: query text
          r'       ROUND(index_length / 1048576, 1) AS index_mb,', // l10n-ignore: query text
          r'       ROUND((data_length + index_length) / 1048576, 1) AS total_mb,', // l10n-ignore: query text
          r'       ROUND(data_free / 1048576, 1) AS free_mb', // l10n-ignore: query text
          r'FROM information_schema.tables', // l10n-ignore: query text
          r"WHERE table_type = 'BASE TABLE'", // l10n-ignore: query text
          r"  AND table_schema NOT IN ('mysql', 'information_schema', 'performance_schema', 'sys')", // l10n-ignore: query text
          r'ORDER BY data_length + index_length DESC', // l10n-ignore: query text
          r'LIMIT {{limit}};', // l10n-ignore: query text
        ],
        variables: const [SnippetVariable(name: 'limit', defaultValue: '20')],
      ),
      db(mysql, 'db.mysql.database_sizes', l.snippetDbMysqlDatabaseSizes, l.snippetDbMysqlDatabaseSizesNote, [
        r'SELECT table_schema AS database_name, COUNT(*) AS table_count,', // l10n-ignore: query text
        r'       ROUND(SUM(data_length + index_length) / 1048576, 1) AS total_mb', // l10n-ignore: query text
        r'FROM information_schema.tables', // l10n-ignore: query text
        r'GROUP BY table_schema', // l10n-ignore: query text
        r'ORDER BY SUM(data_length + index_length) DESC;', // l10n-ignore: query text
      ]),
      db(
        mysql,
        'db.mysql.index_usage',
        l.snippetDbMysqlIndexUsage,
        l.snippetDbMysqlIndexUsageNote,
        [
          r'SELECT object_schema, object_name AS table_name, index_name,', // l10n-ignore: query text
          r'       count_read, count_write, count_fetch', // l10n-ignore: query text
          r'FROM performance_schema.table_io_waits_summary_by_index_usage', // l10n-ignore: query text
          r'WHERE index_name IS NOT NULL', // l10n-ignore: query text
          r"  AND object_schema NOT IN ('mysql', 'performance_schema', 'sys')", // l10n-ignore: query text
          r'ORDER BY count_read DESC', // l10n-ignore: query text
          r'LIMIT {{limit}};', // l10n-ignore: query text
        ],
        variables: const [SnippetVariable(name: 'limit', defaultValue: '50')],
      ),
      db(mysql, 'db.mysql.unused_indexes', l.snippetDbMysqlUnusedIndexes, l.snippetDbMysqlUnusedIndexesNote, [
        r'SELECT object_schema, object_name AS table_name, index_name', // l10n-ignore: query text
        r'FROM sys.schema_unused_indexes', // l10n-ignore: query text
        r'ORDER BY object_schema, object_name, index_name;', // l10n-ignore: query text
      ]),
      db(
        mysql,
        'db.mysql.auto_increment',
        l.snippetDbMysqlAutoIncrement,
        l.snippetDbMysqlAutoIncrementNote,
        [
          r'SELECT table_schema, table_name, column_name, column_type, auto_increment, max_value,', // l10n-ignore: query text
          r'       ROUND(auto_increment_ratio * 100, 2) AS used_pct', // l10n-ignore: query text
          r'FROM sys.schema_auto_increment_columns', // l10n-ignore: query text
          r'ORDER BY auto_increment_ratio DESC', // l10n-ignore: query text
          r'LIMIT {{limit}};', // l10n-ignore: query text
        ],
        variables: const [SnippetVariable(name: 'limit', defaultValue: '20')],
      ),
      db(
        mysql,
        'db.mysql.indexes',
        l.snippetDbMysqlIndexes,
        l.snippetDbMysqlIndexesNote,
        [
          r'SELECT index_name, seq_in_index, column_name, non_unique, cardinality, index_type', // l10n-ignore: query text
          r'FROM information_schema.statistics', // l10n-ignore: query text
          r"WHERE table_schema = '{{schema}}' AND table_name = '{{table}}'", // l10n-ignore: query text
          r'ORDER BY index_name, seq_in_index;', // l10n-ignore: query text
        ],
        variables: const [
          SnippetVariable(name: 'schema'),
          SnippetVariable(name: 'table'),
        ],
      ),
      db(
        mysql,
        'db.mysql.columns',
        l.snippetDbMysqlColumns,
        l.snippetDbMysqlColumnsNote,
        [
          r'SELECT ordinal_position, column_name, column_type, is_nullable, column_default,', // l10n-ignore: query text
          r'       column_key, extra, column_comment', // l10n-ignore: query text
          r'FROM information_schema.columns', // l10n-ignore: query text
          r"WHERE table_schema = '{{schema}}' AND table_name = '{{table}}'", // l10n-ignore: query text
          r'ORDER BY ordinal_position;', // l10n-ignore: query text
        ],
        variables: const [
          SnippetVariable(name: 'schema'),
          SnippetVariable(name: 'table'),
        ],
      ),
      db(
        mysql,
        'db.mysql.foreign_keys',
        l.snippetDbMysqlForeignKeys,
        l.snippetDbMysqlForeignKeysNote,
        [
          r'SELECT constraint_name, table_name, column_name,', // l10n-ignore: query text
          r'       referenced_table_name, referenced_column_name', // l10n-ignore: query text
          r'FROM information_schema.key_column_usage', // l10n-ignore: query text
          r'WHERE referenced_table_name IS NOT NULL', // l10n-ignore: query text
          r"  AND table_schema = '{{schema}}'", // l10n-ignore: query text
          r"  AND (table_name = '{{table}}' OR referenced_table_name = '{{table}}')", // l10n-ignore: query text
          r'ORDER BY constraint_name, ordinal_position;', // l10n-ignore: query text
        ],
        variables: const [
          SnippetVariable(name: 'schema'),
          SnippetVariable(name: 'table'),
        ],
      ),
      db(mysql, 'db.mysql.replica_status', l.snippetDbMysqlReplicaStatus, l.snippetDbMysqlReplicaStatusNote, [
        r'SHOW REPLICA STATUS;', // l10n-ignore: query text
      ]),
      db(
        mysql,
        'db.mysql.variables',
        l.snippetDbMysqlVariables,
        l.snippetDbMysqlVariablesNote,
        [
          r"SHOW GLOBAL VARIABLES LIKE '{{pattern}}';", // l10n-ignore: query text
        ],
        variables: const [SnippetVariable(name: 'pattern', defaultValue: '%timeout%')],
      ),
      db(mysql, 'db.mysql.status', l.snippetDbMysqlStatus, l.snippetDbMysqlStatusNote, [
        r'SHOW GLOBAL STATUS WHERE Variable_name IN (', // l10n-ignore: query text
        r"  'Uptime', 'Threads_connected', 'Threads_running', 'Max_used_connections',", // l10n-ignore: query text
        r"  'Aborted_connects', 'Slow_queries', 'Questions', 'Innodb_row_lock_waits',", // l10n-ignore: query text
        r"  'Innodb_buffer_pool_reads', 'Innodb_buffer_pool_read_requests');", // l10n-ignore: query text
      ]),
      db(mysql, 'db.mysql.connections', l.snippetDbMysqlConnections, l.snippetDbMysqlConnectionsNote, [
        r"SELECT user, SUBSTRING_INDEX(host, ':', 1) AS client_host, db, command, COUNT(*) AS connections", // l10n-ignore: query text
        r'FROM information_schema.processlist', // l10n-ignore: query text
        r'GROUP BY user, client_host, db, command', // l10n-ignore: query text
        r'ORDER BY connections DESC;', // l10n-ignore: query text
      ]),
      db(
        mysql,
        'db.mysql.digest',
        l.snippetDbMysqlDigest,
        l.snippetDbMysqlDigestNote,
        [
          r'SELECT schema_name, LEFT(digest_text, 200) AS query, count_star AS calls,', // l10n-ignore: query text
          r'       ROUND(sum_timer_wait / 1000000000000, 3) AS total_s,', // l10n-ignore: query text
          r'       ROUND(avg_timer_wait / 1000000000, 3) AS avg_ms,', // l10n-ignore: query text
          r'       sum_rows_examined AS rows_examined, sum_rows_sent AS rows_sent', // l10n-ignore: query text
          r'FROM performance_schema.events_statements_summary_by_digest', // l10n-ignore: query text
          r'ORDER BY sum_timer_wait DESC', // l10n-ignore: query text
          r'LIMIT {{limit}};', // l10n-ignore: query text
        ],
        variables: const [SnippetVariable(name: 'limit', defaultValue: '20')],
      ),
      db(mysql, 'db.mysql.innodb_status', l.snippetDbMysqlInnodbStatus, l.snippetDbMysqlInnodbStatusNote, [
        r'SHOW ENGINE INNODB STATUS;', // l10n-ignore: query text
      ]),
      db(
        mysql,
        'db.mysql.explain_analyze',
        l.snippetDbMysqlExplainAnalyze,
        l.snippetDbMysqlExplainAnalyzeNote,
        [
          r'EXPLAIN ANALYZE', // l10n-ignore: query text
          r"SELECT * FROM {{table}} WHERE {{column}} = '{{value}}';", // l10n-ignore: query text
        ],
        variables: const [
          SnippetVariable(name: 'table'),
          SnippetVariable(name: 'column'),
          SnippetVariable(name: 'value'),
        ],
        risk: RiskLevel.modifying,
      ),
      db(
        mysql,
        'db.mysql.kill_query',
        l.snippetDbMysqlKillQuery,
        l.snippetDbMysqlKillQueryNote,
        [
          r'KILL QUERY {{id}};', // l10n-ignore: query text
        ],
        variables: const [SnippetVariable(name: 'id')],
        risk: RiskLevel.modifying,
        terminalOnly: true,
      ),
    ]),
    StarterSnippetPackage('clickhouse', clickhouse.name, l.snippetPackClickhouseDescription, [
      db(clickhouse, 'db.clickhouse.processes', l.snippetDbClickhouseProcesses, l.snippetDbClickhouseProcessesNote, [
        r'SELECT query_id, user, round(elapsed, 1) AS elapsed_s,', // l10n-ignore: query text
        r'       formatReadableSize(memory_usage) AS memory, read_rows,', // l10n-ignore: query text
        r'       formatReadableSize(read_bytes) AS read_size, substring(query, 1, 200) AS query', // l10n-ignore: query text
        r'FROM system.processes', // l10n-ignore: query text
        r'WHERE is_initial_query', // l10n-ignore: query text
        r'ORDER BY elapsed DESC;', // l10n-ignore: query text
      ]),
      db(
        clickhouse,
        'db.clickhouse.long_running',
        l.snippetDbClickhouseLongRunning,
        l.snippetDbClickhouseLongRunningNote,
        [
          r'SELECT query_id, user, round(elapsed, 1) AS elapsed_s,', // l10n-ignore: query text
          r'       formatReadableSize(memory_usage) AS memory, substring(query, 1, 200) AS query', // l10n-ignore: query text
          r'FROM system.processes', // l10n-ignore: query text
          r'WHERE is_initial_query AND elapsed >= {{seconds}}', // l10n-ignore: query text
          r'ORDER BY elapsed DESC;', // l10n-ignore: query text
        ],
        variables: const [SnippetVariable(name: 'seconds', defaultValue: '60')],
      ),
      db(
        clickhouse,
        'db.clickhouse.slow_queries',
        l.snippetDbClickhouseSlowQueries,
        l.snippetDbClickhouseSlowQueriesNote,
        [
          r'SELECT event_time, query_duration_ms, user, read_rows,', // l10n-ignore: query text
          r'       formatReadableSize(read_bytes) AS read_size,', // l10n-ignore: query text
          r'       formatReadableSize(memory_usage) AS memory, substring(query, 1, 200) AS query', // l10n-ignore: query text
          r'FROM system.query_log', // l10n-ignore: query text
          r"WHERE type = 'QueryFinish'", // l10n-ignore: query text
          r'  AND event_date >= toDate(now() - INTERVAL {{hours}} HOUR)', // l10n-ignore: query text
          r'  AND event_time >= now() - INTERVAL {{hours}} HOUR', // l10n-ignore: query text
          r'ORDER BY query_duration_ms DESC', // l10n-ignore: query text
          r'LIMIT {{limit}};', // l10n-ignore: query text
        ],
        variables: const [
          SnippetVariable(name: 'hours', defaultValue: '24'),
          SnippetVariable(name: 'limit', defaultValue: '20'),
        ],
      ),
      db(
        clickhouse,
        'db.clickhouse.failed_queries',
        l.snippetDbClickhouseFailedQueries,
        l.snippetDbClickhouseFailedQueriesNote,
        [
          r'SELECT event_time, user, exception_code,', // l10n-ignore: query text
          r'       substring(exception, 1, 200) AS error, substring(query, 1, 200) AS query', // l10n-ignore: query text
          r'FROM system.query_log', // l10n-ignore: query text
          r"WHERE type IN ('ExceptionBeforeStart', 'ExceptionWhileProcessing')", // l10n-ignore: query text
          r'  AND event_date >= toDate(now() - INTERVAL {{hours}} HOUR)', // l10n-ignore: query text
          r'  AND event_time >= now() - INTERVAL {{hours}} HOUR', // l10n-ignore: query text
          r'ORDER BY event_time DESC', // l10n-ignore: query text
          r'LIMIT {{limit}};', // l10n-ignore: query text
        ],
        variables: const [
          SnippetVariable(name: 'hours', defaultValue: '24'),
          SnippetVariable(name: 'limit', defaultValue: '20'),
        ],
      ),
      db(
        clickhouse,
        'db.clickhouse.query_patterns',
        l.snippetDbClickhouseQueryPatterns,
        l.snippetDbClickhouseQueryPatternsNote,
        [
          r'SELECT normalized_query_hash, any(substring(query, 1, 150)) AS example, count() AS calls,', // l10n-ignore: query text
          r'       round(sum(query_duration_ms) / 1000, 1) AS total_s,', // l10n-ignore: query text
          r'       round(avg(query_duration_ms)) AS avg_ms,', // l10n-ignore: query text
          r'       formatReadableSize(sum(read_bytes)) AS read_size', // l10n-ignore: query text
          r'FROM system.query_log', // l10n-ignore: query text
          r"WHERE type = 'QueryFinish'", // l10n-ignore: query text
          r'  AND event_date >= toDate(now() - INTERVAL {{hours}} HOUR)', // l10n-ignore: query text
          r'  AND event_time >= now() - INTERVAL {{hours}} HOUR', // l10n-ignore: query text
          r'GROUP BY normalized_query_hash', // l10n-ignore: query text
          r'ORDER BY sum(query_duration_ms) DESC', // l10n-ignore: query text
          r'LIMIT {{limit}};', // l10n-ignore: query text
        ],
        variables: const [
          SnippetVariable(name: 'hours', defaultValue: '24'),
          SnippetVariable(name: 'limit', defaultValue: '20'),
        ],
      ),
      db(
        clickhouse,
        'db.clickhouse.table_sizes',
        l.snippetDbClickhouseTableSizes,
        l.snippetDbClickhouseTableSizesNote,
        [
          r'SELECT database, table, sum(rows) AS total_rows,', // l10n-ignore: query text
          r'       formatReadableSize(sum(data_compressed_bytes)) AS compressed,', // l10n-ignore: query text
          r'       formatReadableSize(sum(data_uncompressed_bytes)) AS uncompressed,', // l10n-ignore: query text
          r'       round(sum(data_uncompressed_bytes) / nullIf(sum(data_compressed_bytes), 0), 2) AS ratio,', // l10n-ignore: query text
          r'       count() AS parts', // l10n-ignore: query text
          r'FROM system.parts', // l10n-ignore: query text
          r'WHERE active', // l10n-ignore: query text
          r'GROUP BY database, table', // l10n-ignore: query text
          r'ORDER BY sum(data_compressed_bytes) DESC', // l10n-ignore: query text
          r'LIMIT {{limit}};', // l10n-ignore: query text
        ],
        variables: const [SnippetVariable(name: 'limit', defaultValue: '20')],
      ),
      db(
        clickhouse,
        'db.clickhouse.database_sizes',
        l.snippetDbClickhouseDatabaseSizes,
        l.snippetDbClickhouseDatabaseSizesNote,
        [
          r'SELECT database, formatReadableSize(sum(bytes_on_disk)) AS on_disk,', // l10n-ignore: query text
          r'       sum(rows) AS total_rows, count() AS parts', // l10n-ignore: query text
          r'FROM system.parts', // l10n-ignore: query text
          r'WHERE active', // l10n-ignore: query text
          r'GROUP BY database', // l10n-ignore: query text
          r'ORDER BY sum(bytes_on_disk) DESC;', // l10n-ignore: query text
        ],
      ),
      db(
        clickhouse,
        'db.clickhouse.partitions',
        l.snippetDbClickhousePartitions,
        l.snippetDbClickhousePartitionsNote,
        [
          r'SELECT partition, count() AS parts, sum(rows) AS total_rows,', // l10n-ignore: query text
          r'       formatReadableSize(sum(bytes_on_disk)) AS on_disk,', // l10n-ignore: query text
          r'       max(modification_time) AS last_modified', // l10n-ignore: query text
          r'FROM system.parts', // l10n-ignore: query text
          r"WHERE active AND database = '{{database}}' AND table = '{{table}}'", // l10n-ignore: query text
          r'GROUP BY partition', // l10n-ignore: query text
          r'ORDER BY partition;', // l10n-ignore: query text
        ],
        variables: const [
          SnippetVariable(name: 'database'),
          SnippetVariable(name: 'table'),
        ],
      ),
      db(
        clickhouse,
        'db.clickhouse.parts_count',
        l.snippetDbClickhousePartsCount,
        l.snippetDbClickhousePartsCountNote,
        [
          r'SELECT database, table, partition, count() AS active_parts', // l10n-ignore: query text
          r'FROM system.parts', // l10n-ignore: query text
          r'WHERE active', // l10n-ignore: query text
          r'GROUP BY database, table, partition', // l10n-ignore: query text
          r'ORDER BY active_parts DESC', // l10n-ignore: query text
          r'LIMIT {{limit}};', // l10n-ignore: query text
        ],
        variables: const [SnippetVariable(name: 'limit', defaultValue: '20')],
      ),
      db(
        clickhouse,
        'db.clickhouse.column_sizes',
        l.snippetDbClickhouseColumnSizes,
        l.snippetDbClickhouseColumnSizesNote,
        [
          r'SELECT name, type,', // l10n-ignore: query text
          r'       formatReadableSize(data_compressed_bytes) AS compressed,', // l10n-ignore: query text
          r'       formatReadableSize(data_uncompressed_bytes) AS uncompressed,', // l10n-ignore: query text
          r'       round(data_uncompressed_bytes / nullIf(data_compressed_bytes, 0), 2) AS ratio,', // l10n-ignore: query text
          r'       compression_codec', // l10n-ignore: query text
          r'FROM system.columns', // l10n-ignore: query text
          r"WHERE database = '{{database}}' AND table = '{{table}}'", // l10n-ignore: query text
          r'ORDER BY data_compressed_bytes DESC;', // l10n-ignore: query text
        ],
        variables: const [
          SnippetVariable(name: 'database'),
          SnippetVariable(name: 'table'),
        ],
      ),
      db(clickhouse, 'db.clickhouse.merges', l.snippetDbClickhouseMerges, l.snippetDbClickhouseMergesNote, [
        r'SELECT database, table, round(elapsed, 1) AS elapsed_s,', // l10n-ignore: query text
        r'       round(progress * 100, 1) AS progress_pct, num_parts,', // l10n-ignore: query text
        r'       formatReadableSize(total_size_bytes_compressed) AS size,', // l10n-ignore: query text
        r'       result_part_name, is_mutation', // l10n-ignore: query text
        r'FROM system.merges', // l10n-ignore: query text
        r'ORDER BY elapsed DESC;', // l10n-ignore: query text
      ]),
      db(clickhouse, 'db.clickhouse.mutations', l.snippetDbClickhouseMutations, l.snippetDbClickhouseMutationsNote, [
        r'SELECT database, table, mutation_id, substring(command, 1, 200) AS command,', // l10n-ignore: query text
        r'       create_time, parts_to_do, latest_fail_reason', // l10n-ignore: query text
        r'FROM system.mutations', // l10n-ignore: query text
        r'WHERE NOT is_done', // l10n-ignore: query text
        r'ORDER BY create_time;', // l10n-ignore: query text
      ]),
      db(
        clickhouse,
        'db.clickhouse.replication_queue',
        l.snippetDbClickhouseReplicationQueue,
        l.snippetDbClickhouseReplicationQueueNote,
        [
          r'SELECT database, table, type, create_time, num_tries,', // l10n-ignore: query text
          r'       substring(last_exception, 1, 200) AS last_exception, postpone_reason', // l10n-ignore: query text
          r'FROM system.replication_queue', // l10n-ignore: query text
          r'ORDER BY create_time', // l10n-ignore: query text
          r'LIMIT {{limit}};', // l10n-ignore: query text
        ],
        variables: const [SnippetVariable(name: 'limit', defaultValue: '20')],
      ),
      db(clickhouse, 'db.clickhouse.replicas', l.snippetDbClickhouseReplicas, l.snippetDbClickhouseReplicasNote, [
        r'SELECT database, table, is_readonly, is_session_expired, absolute_delay,', // l10n-ignore: query text
        r'       queue_size, inserts_in_queue, merges_in_queue, active_replicas, total_replicas', // l10n-ignore: query text
        r'FROM system.replicas', // l10n-ignore: query text
        r'ORDER BY absolute_delay DESC;', // l10n-ignore: query text
      ]),
      db(clickhouse, 'db.clickhouse.disks', l.snippetDbClickhouseDisks, l.snippetDbClickhouseDisksNote, [
        r'SELECT name, path, formatReadableSize(free_space) AS free,', // l10n-ignore: query text
        r'       formatReadableSize(total_space) AS total,', // l10n-ignore: query text
        r'       round(100 * (1 - free_space / nullIf(total_space, 0)), 1) AS used_pct', // l10n-ignore: query text
        r'FROM system.disks;', // l10n-ignore: query text
      ]),
      db(
        clickhouse,
        'db.clickhouse.errors',
        l.snippetDbClickhouseErrors,
        l.snippetDbClickhouseErrorsNote,
        [
          r'SELECT name, code, value AS error_count, last_error_time,', // l10n-ignore: query text
          r'       substring(last_error_message, 1, 200) AS last_message', // l10n-ignore: query text
          r'FROM system.errors', // l10n-ignore: query text
          r'ORDER BY last_error_time DESC', // l10n-ignore: query text
          r'LIMIT {{limit}};', // l10n-ignore: query text
        ],
        variables: const [SnippetVariable(name: 'limit', defaultValue: '20')],
      ),
      db(
        clickhouse,
        'db.clickhouse.settings_changed',
        l.snippetDbClickhouseSettingsChanged,
        l.snippetDbClickhouseSettingsChangedNote,
        [
          r'SELECT name, value, type', // l10n-ignore: query text
          r'FROM system.settings', // l10n-ignore: query text
          r'WHERE changed', // l10n-ignore: query text
          r'ORDER BY name;', // l10n-ignore: query text
        ],
      ),
      db(clickhouse, 'db.clickhouse.metrics', l.snippetDbClickhouseMetrics, l.snippetDbClickhouseMetricsNote, [
        r'SELECT metric, value, description', // l10n-ignore: query text
        r'FROM system.metrics', // l10n-ignore: query text
        r'WHERE value != 0', // l10n-ignore: query text
        r'ORDER BY metric;', // l10n-ignore: query text
      ]),
      db(
        clickhouse,
        'db.clickhouse.explain',
        l.snippetDbClickhouseExplain,
        l.snippetDbClickhouseExplainNote,
        [
          r'EXPLAIN indexes = 1', // l10n-ignore: query text
          r"SELECT * FROM {{table}} WHERE {{column}} = '{{value}}';", // l10n-ignore: query text
        ],
        variables: const [
          SnippetVariable(name: 'table'),
          SnippetVariable(name: 'column'),
          SnippetVariable(name: 'value'),
        ],
        terminalOnly: true,
      ),
      db(
        clickhouse,
        'db.clickhouse.kill_query',
        l.snippetDbClickhouseKillQuery,
        l.snippetDbClickhouseKillQueryNote,
        [
          r"KILL QUERY WHERE query_id = '{{query_id}}';", // l10n-ignore: query text
        ],
        variables: const [SnippetVariable(name: 'query_id')],
        risk: RiskLevel.modifying,
        terminalOnly: true,
      ),
    ]),
    StarterSnippetPackage('redis', redis.name, l.snippetPackRedisDescription, [
      db(
        redis,
        'db.redis.info',
        l.snippetDbRedisInfo,
        l.snippetDbRedisInfoNote,
        [
          r'INFO {{section}}', // l10n-ignore: query text
        ],
        variables: const [SnippetVariable(name: 'section', defaultValue: 'server')],
      ),
      db(redis, 'db.redis.memory', l.snippetDbRedisMemory, l.snippetDbRedisMemoryNote, [
        r'INFO memory', // l10n-ignore: query text
      ]),
      db(redis, 'db.redis.memory_doctor', l.snippetDbRedisMemoryDoctor, l.snippetDbRedisMemoryDoctorNote, [
        r'MEMORY DOCTOR', // l10n-ignore: query text
      ], terminalOnly: true),
      db(
        redis,
        'db.redis.slowlog',
        l.snippetDbRedisSlowlog,
        l.snippetDbRedisSlowlogNote,
        [
          r'SLOWLOG GET {{count}}', // l10n-ignore: query text
        ],
        variables: const [SnippetVariable(name: 'count', defaultValue: '10')],
        terminalOnly: true,
      ),
      db(redis, 'db.redis.clients', l.snippetDbRedisClients, l.snippetDbRedisClientsNote, [
        r'CLIENT LIST', // l10n-ignore: query text
      ], terminalOnly: true),
      db(redis, 'db.redis.dbsize', l.snippetDbRedisDbsize, l.snippetDbRedisDbsizeNote, [
        r'DBSIZE', // l10n-ignore: query text
      ]),
      db(redis, 'db.redis.keyspace', l.snippetDbRedisKeyspace, l.snippetDbRedisKeyspaceNote, [
        r'INFO keyspace', // l10n-ignore: query text
      ]),
      db(
        redis,
        'db.redis.scan',
        l.snippetDbRedisScan,
        l.snippetDbRedisScanNote,
        [
          r'SCAN {{cursor}} MATCH {{pattern}} COUNT {{count}}', // l10n-ignore: query text
        ],
        variables: const [
          SnippetVariable(name: 'cursor', defaultValue: '0'),
          SnippetVariable(name: 'pattern', defaultValue: '*'),
          SnippetVariable(name: 'count', defaultValue: '100'),
        ],
      ),
      db(
        redis,
        'db.redis.type',
        l.snippetDbRedisType,
        l.snippetDbRedisTypeNote,
        [
          r'TYPE {{key}}', // l10n-ignore: query text
        ],
        variables: const [SnippetVariable(name: 'key')],
      ),
      db(
        redis,
        'db.redis.ttl',
        l.snippetDbRedisTtl,
        l.snippetDbRedisTtlNote,
        [
          r'TTL {{key}}', // l10n-ignore: query text
        ],
        variables: const [SnippetVariable(name: 'key')],
      ),
      db(
        redis,
        'db.redis.memory_usage',
        l.snippetDbRedisMemoryUsage,
        l.snippetDbRedisMemoryUsageNote,
        [
          r'MEMORY USAGE {{key}}', // l10n-ignore: query text
        ],
        variables: const [SnippetVariable(name: 'key')],
      ),
      db(redis, 'db.redis.latency', l.snippetDbRedisLatency, l.snippetDbRedisLatencyNote, [
        r'LATENCY DOCTOR', // l10n-ignore: query text
      ], terminalOnly: true),
      db(
        redis,
        'db.redis.config',
        l.snippetDbRedisConfig,
        l.snippetDbRedisConfigNote,
        [
          r'CONFIG GET {{parameter}}', // l10n-ignore: query text
        ],
        variables: const [SnippetVariable(name: 'parameter', defaultValue: 'maxmemory*')],
        terminalOnly: true,
      ),
      db(redis, 'db.redis.replication', l.snippetDbRedisReplication, l.snippetDbRedisReplicationNote, [
        r'INFO replication', // l10n-ignore: query text
      ]),
      db(redis, 'db.redis.commandstats', l.snippetDbRedisCommandstats, l.snippetDbRedisCommandstatsNote, [
        r'INFO commandstats', // l10n-ignore: query text
      ]),
    ]),
    StarterSnippetPackage('mongodb', mongodb.name, l.snippetPackMongodbDescription, [
      db(mongodb, 'db.mongodb.current_ops', l.snippetDbMongodbCurrentOps, l.snippetDbMongodbCurrentOpsNote, [
        r'db.currentOp({ "active": true })', // l10n-ignore: query text
      ], terminalOnly: true),
      db(
        mongodb,
        'db.mongodb.long_ops',
        l.snippetDbMongodbLongOps,
        l.snippetDbMongodbLongOpsNote,
        [
          r'db.currentOp({ "active": true, "secs_running": { "$gte": {{seconds}} } })', // l10n-ignore: query text
        ],
        variables: const [SnippetVariable(name: 'seconds', defaultValue: '10')],
        terminalOnly: true,
      ),
      db(mongodb, 'db.mongodb.connections', l.snippetDbMongodbConnections, l.snippetDbMongodbConnectionsNote, [
        r'db.serverStatus().connections', // l10n-ignore: query text
      ], terminalOnly: true),
      db(mongodb, 'db.mongodb.opcounters', l.snippetDbMongodbOpcounters, l.snippetDbMongodbOpcountersNote, [
        r'db.serverStatus().opcounters', // l10n-ignore: query text
      ], terminalOnly: true),
      db(mongodb, 'db.mongodb.memory', l.snippetDbMongodbMemory, l.snippetDbMongodbMemoryNote, [
        r'db.serverStatus().mem', // l10n-ignore: query text
      ], terminalOnly: true),
      db(mongodb, 'db.mongodb.db_stats', l.snippetDbMongodbDbStats, l.snippetDbMongodbDbStatsNote, [
        r'db.runCommand({ "dbStats": 1, "scale": 1048576 })', // l10n-ignore: query text
      ]),
      db(
        mongodb,
        'db.mongodb.collection_stats',
        l.snippetDbMongodbCollectionStats,
        l.snippetDbMongodbCollectionStatsNote,
        [
          r'db.runCommand({ "collStats": "{{collection}}", "scale": 1048576 })', // l10n-ignore: query text
        ],
        variables: const [SnippetVariable(name: 'collection')],
      ),
      db(
        mongodb,
        'db.mongodb.indexes',
        l.snippetDbMongodbIndexes,
        l.snippetDbMongodbIndexesNote,
        [
          r'db.getCollection("{{collection}}").getIndexes()', // l10n-ignore: query text
        ],
        variables: const [SnippetVariable(name: 'collection')],
      ),
      db(
        mongodb,
        'db.mongodb.index_stats',
        l.snippetDbMongodbIndexStats,
        l.snippetDbMongodbIndexStatsNote,
        [
          r'db.getCollection("{{collection}}").aggregate([{ "$indexStats": {} }])', // l10n-ignore: query text
        ],
        variables: const [SnippetVariable(name: 'collection')],
        terminalOnly: true,
      ),
      db(
        mongodb,
        'db.mongodb.explain',
        l.snippetDbMongodbExplain,
        l.snippetDbMongodbExplainNote,
        [
          r'db.getCollection("{{collection}}").find({ "{{field}}": "{{value}}" }).explain("executionStats")', // l10n-ignore: query text
        ],
        variables: const [
          SnippetVariable(name: 'collection'),
          SnippetVariable(name: 'field'),
          SnippetVariable(name: 'value'),
        ],
        terminalOnly: true,
      ),
      db(mongodb, 'db.mongodb.rs_status', l.snippetDbMongodbRsStatus, l.snippetDbMongodbRsStatusNote, [
        r'rs.status()', // l10n-ignore: query text
      ], terminalOnly: true),
      db(mongodb, 'db.mongodb.rs_lag', l.snippetDbMongodbRsLag, l.snippetDbMongodbRsLagNote, [
        r'rs.printSecondaryReplicationInfo()', // l10n-ignore: query text
      ], terminalOnly: true),
      db(
        mongodb,
        'db.mongodb.profiler_status',
        l.snippetDbMongodbProfilerStatus,
        l.snippetDbMongodbProfilerStatusNote,
        [
          r'db.getProfilingStatus()', // l10n-ignore: query text
        ],
        terminalOnly: true,
      ),
      db(
        mongodb,
        'db.mongodb.slow_ops',
        l.snippetDbMongodbSlowOps,
        l.snippetDbMongodbSlowOpsNote,
        [
          r'db.getCollection("system.profile").find({ "millis": { "$gte": {{ms}} } }).sort({ "ts": -1 }).limit({{limit}})', // l10n-ignore: query text
        ],
        variables: const [
          SnippetVariable(name: 'ms', defaultValue: '100'),
          SnippetVariable(name: 'limit', defaultValue: '20'),
        ],
      ),
      db(
        mongodb,
        'db.mongodb.kill_op',
        l.snippetDbMongodbKillOp,
        l.snippetDbMongodbKillOpNote,
        [
          r'db.killOp({{opid}})', // l10n-ignore: query text
        ],
        variables: const [SnippetVariable(name: 'opid')],
        risk: RiskLevel.modifying,
        terminalOnly: true,
      ),
    ]),
  ];
}
