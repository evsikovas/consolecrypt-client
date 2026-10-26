import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_snippet_picker.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/snippets/run_flow.dart';
import 'package:consolecrypt/snippets/starter_catalog.dart';
import 'package:consolecrypt/snippets/starter_catalog_db.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  final en = lookupAppLocalizations(const Locale('en'));
  final ru = lookupAppLocalizations(const Locale('ru'));
  test('editable starter packages: unique ids, complete variables, lossless mapping', () {
    final packages = starterSnippetPackages(en);
    expect(packages.map((p) => p.id), [
      'linux',
      'docker',
      'kubernetes',
      'postgresql',
      'mysql',
      'clickhouse',
      'redis',
      'mongodb',
      'sqlite',
      'cassandra',
    ]);
    expect(
      {for (final p in packages) p.id: p.snippets.length},
      {
        'linux': 5,
        'docker': 5,
        'kubernetes': 5,
        'postgresql': 20,
        'mysql': 20,
        'clickhouse': 20,
        'redis': 15,
        'mongodb': 15,
        'sqlite': 15,
        'cassandra': 17,
      },
    );
    final snippets = packages.expand((p) => p.snippets).toList();
    expect(snippets.map((s) => s.catalogId).toSet(), hasLength(snippets.length));
    for (final s in snippets) {
      expect(s.packageName, isNotEmpty);
      expect(s.name, isNotEmpty);
      expect(s.description, isNotEmpty);
      expect(s.source, SnippetSource.imported);
      expect(s.catalogId, matches(RegExp(r'^consolecrypt\.[a-z0-9_.]+\.v1$')));
      expect(s.variables.map((v) => v.name).toSet(), templateVariables(s.template).toSet(), reason: s.catalogId);
      expect(s.variables.map((v) => v.name).toSet(), hasLength(s.variables.length), reason: s.catalogId);
      final copy = snippetFromJson(snippetToJson(s));
      expect(copy.packageName, s.packageName);
      expect(copy.catalogId, s.catalogId);
      expect(copy.template, s.template);
      expect(copy.shell, s.shell);
      expect(copy.riskLevel, s.riskLevel);
      expect(copy.tags, s.tags);
    }
    for (final s in packages.take(3).expand((p) => p.snippets)) {
      expect(s.riskLevel, RiskLevel.readOnly);
    }
  });

  test('database packages use the Databases history type / dialect mapping', () {
    final byId = {for (final p in starterSnippetPackages(en)) p.id: p.snippets};
    const engines = {
      'postgresql': (DbEngine.postgresql, SnippetType.postgresql, null),
      'mysql': (DbEngine.mysql, SnippetType.sql, 'mysql'),
      'clickhouse': (DbEngine.clickhouse, SnippetType.sql, 'clickhouse'),
      'redis': (DbEngine.redis, SnippetType.redisCli, null),
      'mongodb': (DbEngine.mongodb, SnippetType.shell, 'mongosh'),
      'sqlite': (DbEngine.sqlite, SnippetType.sql, 'sqlite'),
      'cassandra': (DbEngine.cassandra, SnippetType.cql, null),
    };
    for (final MapEntry(key: id, value: (engine, type, shell)) in engines.entries) {
      for (final s in byId[id]!) {
        expect(s.snippetType, type, reason: s.catalogId);
        expect(s.shell, shell, reason: s.catalogId);
        expect(s.tags, contains(databaseSnippetTag));
        expect(isClientInputSnippet(s), isTrue, reason: s.catalogId);
        final terminalOnly = s.tags.contains(terminalOnlySnippetTag);
        expect(snippetMatchesEngine(s, engine), !terminalOnly, reason: s.catalogId);
        for (final other in DbEngine.values.where((e) => e != engine)) {
          final shared = {engine, other}.containsAll({DbEngine.mysql, DbEngine.mariadb});
          expect(snippetMatchesEngine(s, other), shared && !terminalOnly, reason: '${s.catalogId} on $other');
        }
        if (engine.sql) {
          expect(s.template.trimRight(), endsWith(';'), reason: s.catalogId);
          expect(s.template.contains('\\'), isFalse, reason: 'no psql meta-commands: ${s.catalogId}');
        } else {
          expect(s.template, isNot(contains('\n')), reason: 'one command per line: ${s.catalogId}');
        }
        expect(
          s.description,
          contains(switch (engine) {
            DbEngine.postgresql => 'psql',
            DbEngine.mysql || DbEngine.mariadb => 'mysql',
            DbEngine.clickhouse => 'clickhouse-client',
            DbEngine.redis => 'redis-cli',
            DbEngine.mongodb => 'mongosh',
            DbEngine.sqlite => 'sqlite3',
            DbEngine.cassandra => 'cqlsh',
          }),
        );
      }
    }
    // PostgreSQL catalog views are schema-qualified: the workspace guard
    // qualifies bare relation names with the session's default schema.
    for (final s in byId['postgresql']!) {
      expect(
        RegExp(r'(FROM|JOIN)\s+pg_(?!stat_statements|catalog\.)').hasMatch(s.template),
        isFalse,
        reason: s.catalogId,
      );
    }
    // Server-changing snippets declare their risk.
    final risks = {
      for (final s in byId.values.expand((v) => v))
        if (s.riskLevel != RiskLevel.readOnly) s.catalogId: s.riskLevel,
    };
    expect(risks, {
      'consolecrypt.db.pg.explain_analyze.v1': RiskLevel.modifying,
      'consolecrypt.db.pg.cancel.v1': RiskLevel.modifying,
      'consolecrypt.db.pg.terminate.v1': RiskLevel.destructive,
      'consolecrypt.db.mysql.explain_analyze.v1': RiskLevel.modifying,
      'consolecrypt.db.mysql.kill_query.v1': RiskLevel.modifying,
      'consolecrypt.db.clickhouse.kill_query.v1': RiskLevel.modifying,
      'consolecrypt.db.mongodb.kill_op.v1': RiskLevel.modifying,
      'consolecrypt.db.sqlite.analyze.v1': RiskLevel.modifying,
      'consolecrypt.db.sqlite.vacuum.v1': RiskLevel.destructive,
      'consolecrypt.db.cql.create_table.v1': RiskLevel.modifying,
      'consolecrypt.db.cql.insert_json.v1': RiskLevel.modifying,
    });
  });

  test('generic SQL snippets match every SQL engine; shell snippets none', () {
    final now = DateTime.utc(2026);
    Snippet s(SnippetType type, String? shell) => Snippet(
      id: ObjectId.generate(),
      name: 'x',
      snippetType: type,
      shell: shell,
      template: 'SELECT 1;',
      createdAt: now,
      updatedAt: now,
    );
    for (final engine in DbEngine.values) {
      expect(snippetMatchesEngine(s(SnippetType.sql, null), engine), engine.sql);
      expect(snippetMatchesEngine(s(SnippetType.bash, 'bash'), engine), isFalse);
    }
    expect(snippetMatchesEngine(s(SnippetType.sql, 'mariadb'), DbEngine.mysql), isTrue);
    expect(snippetMatchesEngine(s(SnippetType.sql, 'mariadb'), DbEngine.clickhouse), isFalse);
    expect(isClientInputSnippet(s(SnippetType.bash, 'bash')), isFalse);
  });

  test('reimport skips edited entries across locale and package renames, and can restore a deletion', () {
    final english = starterSnippetPackages(en).first;
    final saved = english.snippets
        .map((s) => s.copyWith(name: 'Custom', template: 'df -h /', packageName: 'Mine'))
        .toList();
    final russian = starterSnippetPackages(ru).first;
    expect(russian.missingFrom(saved), isEmpty);
    final removed = saved.removeAt(0);
    expect(russian.missingFrom(saved).single.catalogId, removed.catalogId);
    final edited = saved.first.copyWith(usageCount: 5, clearPackage: true);
    expect(edited.packageName, isNull);
    expect(edited.catalogId, saved.first.catalogId);
    expect(edited.template, saved.first.template);
  });

  test('Russian catalog localizes names and notes, keeps the query text', () {
    final english = starterSnippetPackages(en).expand((p) => p.snippets).toList();
    final russian = starterSnippetPackages(ru).expand((p) => p.snippets).toList();
    for (final (i, s) in english.indexed) {
      expect(russian[i].catalogId, s.catalogId);
      expect(russian[i].template, s.template);
    }
    final pg = russian.firstWhere((s) => s.catalogId == 'consolecrypt.db.pg.activity.v1');
    expect(pg.name, 'Активные запросы');
    expect(pg.description, contains('Для psql'));
  });
}
