import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/mock/mock_cloud.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/db/db_designer_models.dart';
import 'package:consolecrypt/db/db_designer_view.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_providers.dart';
import 'package:consolecrypt/db/db_query_service.dart';
import 'package:consolecrypt/db/db_service.dart';
import 'package:consolecrypt/db/db_workspace.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

final _scope = NotifierProvider<_Scope, DbScope>(_Scope.new);

class _Scope extends Notifier<DbScope> {
  @override
  DbScope build() => const DbScope('profile', true);
}

DbConnection _connection(DbEngine engine) => DbConnection(
  id: 'connection',
  name: 'fixture-db',
  engine: engine,
  host: 'db.example.test',
  port: engine.port,
  database: 'demo',
  options: DbDriverOptions(engine: engine),
);

/// Dialects as the core describes them (the parts the UI reads).
DbDesignDialect _dialect(DbEngine engine) => switch (engine) {
  DbEngine.clickhouse => const DbDesignDialect(
    engine: DbEngine.clickhouse,
    features: {'primary_key', 'nullable_wrapper', 'low_cardinality', 'column_comments', 'alter'},
    types: [
      DbDesignType(name: 'UInt64'),
      DbDesignType(name: 'String'),
    ],
    defaultType: 'UInt64',
    options: [
      DbDesignOption(id: 'engine', kind: DbDesignOptionKind.choice, choices: ['MergeTree', 'Memory']),
      DbDesignOption(id: 'order_by', kind: DbDesignOptionKind.text),
    ],
  ),
  _ => const DbDesignDialect(
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
    types: [
      DbDesignType(name: 'integer'),
      DbDesignType(name: 'bigint'),
      DbDesignType(name: 'varchar', params: DbDesignTypeParams.length),
      DbDesignType(name: 'text'),
    ],
    defaultType: 'integer',
    identityKinds: ['by_default', 'always', 'serial'],
    defaultPresets: ['now()', '0'],
    indexMethods: ['btree', 'gin'],
    fkActions: ['CASCADE', 'SET NULL'],
  ),
};

class _Service implements DbService, DbQueryService, DbQueryReviewService, DbTableDesignService {
  _Service({this.engine = DbEngine.postgresql});
  final DbEngine engine;
  final reviews = <String>[], applied = <DbQueryRequest>[], plans = <DbTableDesign>[], discarded = <String>[];
  bool failApply = false;

  @override
  Future<DbSessionInfo> connect(String connectionId, {SecretText? temporaryPassword}) async => DbSessionInfo(
    id: 'session',
    connectionId: 'connection',
    engine: engine,
    serverVersion: '1',
    production: false,
    readOnly: false,
    serverReadOnly: DbServerReadOnly.enforced,
  );
  @override
  Future<void> disconnect(String sessionId) async {}
  @override
  Future<DbTreePage> tree(String sessionId, {DbObjectRef? parent, String? cursor, bool refresh = false}) async {
    if (parent == null) {
      return DbTreePage(
        nodes: [
          engine == DbEngine.postgresql
              ? const DbTreeNode(
                  reference: DbObjectRef(database: '', kind: DbObjectKind.schema, name: 'public'),
                  hasChildren: true,
                )
              : const DbTreeNode(
                  reference: DbObjectRef(database: 'demo', kind: DbObjectKind.database, name: 'demo'),
                  hasChildren: true,
                ),
        ],
      );
    }
    if (parent.kind == DbObjectKind.schema || parent.kind == DbObjectKind.database) {
      return DbTreePage(
        nodes: [
          DbTreeNode(
            reference: DbObjectRef(
              database: parent.kind == DbObjectKind.schema ? '' : 'demo',
              schema: parent.kind == DbObjectKind.schema ? 'public' : null,
              kind: DbObjectKind.table,
              name: 'items',
            ),
            hasChildren: true,
          ),
        ],
      );
    }
    return DbTreePage(
      nodes: [
        DbTreeNode(
          reference: DbObjectRef(
            database: parent.database,
            schema: parent.schema,
            kind: DbObjectKind.column,
            name: 'id',
            parentName: parent.name,
          ),
          hasChildren: false,
          detail: 'integer',
          primaryKey: true,
        ),
      ],
    );
  }

  @override
  Future<DbPreparedQuery> prepareQuery(
    String sessionId,
    String text, {
    required DbRunMode mode,
    required int selectionStart,
    required int selectionEnd,
  }) async => DbPreparedQuery(text: text);
  @override
  Future<DbQueryReview> reviewQuery(String sessionId, DbQueryRequest request) async {
    reviews.add(request.text);
    return DbQueryReview(
      ticket: 'design-ticket',
      risk: DbQueryRisk.dangerous,
      expiresAt: DateTime.now().add(const Duration(minutes: 2)),
    );
  }

  @override
  Future<void> discardQueryReview(String sessionId, String ticket) async => discarded.add(ticket);

  // Designer -------------------------------------------------------------------
  @override
  Future<DbDesignDialect> designDialect(DbEngine engine) async => _dialect(engine);

  /// A stand-in for the core: one CREATE (or ALTER) listing the columns.
  @override
  Future<DbDesignPlan> planDesign(DbEngine engine, DbTableDesign? original, DbTableDesign design) async {
    plans.add(design);
    if (design.name.trim().isEmpty) {
      return const DbDesignPlan(
        issues: [DbDesignIssue(error: true, code: 'name_required', target: 'table.name')],
      );
    }
    final columns = [for (final c in design.columns) '"${c.name}" ${c.typeLabel}'].join(', ');
    final sql = original == null
        ? 'CREATE TABLE "${design.schema ?? design.database}"."${design.name}" ($columns)'
        : 'ALTER TABLE "${original.name}" /* $columns */';
    if (original != null && original.encode() == design.encode()) return const DbDesignPlan();
    return DbDesignPlan(
      statements: [DbDesignStatement(sql: sql, destructive: original != null)],
      script: '$sql;',
      transactional: engine == DbEngine.postgresql,
    );
  }

  @override
  Future<DbTableDesign> loadTableDesign(String sessionId, DbObjectRef table) async => DbTableDesign(
    schema: table.schema,
    database: table.database,
    name: table.name,
    original: table.name,
    columns: [DbDesignColumn(name: 'id', dataType: 'integer', nullable: false, original: 'id')],
    primaryKey: DbDesignKey(name: '${table.name}_pkey', columns: ['id'], original: '${table.name}_pkey'),
  );

  @override
  Future<DbDesignApplyResult> applyDesign(String sessionId, DbQueryRequest request) async {
    applied.add(request);
    return DbDesignApplyResult(
      committed: !failApply,
      statements: [
        DbDesignStatementResult(
          status: failApply ? DbDesignStatementStatus.failed : DbDesignStatementStatus.done,
          errorCode: failApply ? 'query_failed' : null,
        ),
      ],
    );
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => throw UnimplementedError();
}

Future<void> _pump(WidgetTester tester, _Service service) async {
  final backend = testBackend();
  addTearDown(backend.dispose);
  await backend.debugSignInDemoAndUnlock();
  tester.view.physicalSize = const Size(1600, 1000);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        appServicesProvider.overrideWithValue(backend.services),
        dbServiceProvider.overrideWithValue(service),
        dbScopeProvider.overrideWith((ref) => ref.watch(_scope)),
      ],
      child: MaterialApp(
        locale: const Locale('en'),
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        supportedLocales: AppLocalizations.supportedLocales,
        home: Scaffold(
          body: DbWorkspace(connection: _connection(service.engine), onClose: () {}),
        ),
      ),
    ),
  );
  await settle(tester);
}

const _public = DbObjectRef(database: '', kind: DbObjectKind.schema, name: 'public');
const _items = DbObjectRef(database: '', schema: 'public', kind: DbObjectKind.table, name: 'items');

/// Opens an object's menu in the tree and picks [item].
Future<void> _menu(WidgetTester tester, DbObjectRef object, String item) async {
  await tester.tap(find.byKey(ValueKey('db-object-${object.id}')), buttons: kSecondaryButton);
  await settle(tester);
  await tester.tap(find.byKey(ValueKey(item)));
  await settle(tester);
}

Future<void> _createTable(WidgetTester tester) async {
  await _menu(tester, _public, 'db-design-create');
  expect(find.byKey(const ValueKey('db-designer-tab-1')), findsOneWidget);
}

String _ddl(WidgetTester tester) {
  final view = find.byKey(const ValueKey('db-design-ddl'));
  return tester
      .widgetList<Text>(find.descendant(of: view, matching: find.byType(Text)))
      .map((t) {
        return t.textSpan?.toPlainText() ?? t.data ?? '';
      })
      .join('\n');
}

Future<void> _section(WidgetTester tester, String name) async {
  await tester.tap(find.byKey(ValueKey('db-design-section-$name')));
  await settle(tester);
}

void main() {
  testWidgets('create table: columns, live DDL and a draft in the tree; nothing runs while editing', (tester) async {
    final service = _Service();
    await _pump(tester, service);
    await _createTable(tester);
    final l = tester.element(find.byType(DbWorkspace)).l10n;
    // The new table shows in the tree as a draft right away.
    const draft = DbObjectRef(database: '', schema: 'public', kind: DbObjectKind.table, name: 'new_table');
    expect(find.byKey(ValueKey('db-draft-${draft.id}')), findsOneWidget);
    expect(find.byKey(const ValueKey('db-design-mode')), findsOneWidget);
    expect(find.text(l.dbDesignModeCreate), findsOneWidget);

    await tester.enterText(find.byKey(const ValueKey('db-design-name-input')), 'orders');
    await tester.tap(find.byKey(const ValueKey('db-design-add-column')));
    await settle(tester);
    await tester.enterText(find.byKey(const ValueKey('db-design-col-1-name-input')), 'title');
    await tester.enterText(find.byKey(const ValueKey('db-design-col-1-type-input')), 'varchar');
    await tester.enterText(find.byKey(const ValueKey('db-design-col-1-length-input')), '80');
    await settle(tester);
    await _section(tester, 'ddl');
    expect(_ddl(tester), contains('CREATE TABLE "public"."orders" ("id" bigint, "title" varchar(80))'));
    // The draft follows the name and its columns appear under it.
    const orders = DbObjectRef(database: '', schema: 'public', kind: DbObjectKind.table, name: 'orders');
    expect(find.byKey(ValueKey('db-draft-${orders.id}')), findsOneWidget);
    await tester.tap(find.byKey(ValueKey('db-object-${orders.id}')));
    await settle(tester);
    const title = DbObjectRef(
      database: '',
      schema: 'public',
      kind: DbObjectKind.column,
      name: 'title',
      parentName: 'orders',
    );
    expect(find.byKey(ValueKey('db-draft-${title.id}')), findsOneWidget);
    expect(service.reviews, isEmpty);
    expect(service.applied, isEmpty);
  });

  testWidgets('validation errors are shown and block saving', (tester) async {
    final service = _Service();
    await _pump(tester, service);
    await _createTable(tester);
    final l = tester.element(find.byType(DbWorkspace)).l10n;
    await tester.enterText(find.byKey(const ValueKey('db-design-name-input')), '');
    await settle(tester);
    expect(find.text(l.dbDesignIssueNameRequired), findsOneWidget);
    expect(tester.widget<GlassButton>(find.byKey(const ValueKey('db-design-save'))).onPressed, isNull);
  });

  testWidgets('ClickHouse hides foreign keys, unique keys and checks', (tester) async {
    final service = _Service(engine: DbEngine.clickhouse);
    await _pump(tester, service);
    const demo = DbObjectRef(database: 'demo', kind: DbObjectKind.database, name: 'demo');
    await _menu(tester, demo, 'db-design-create');
    expect(find.byKey(const ValueKey('db-design-section-columns')), findsOneWidget);
    expect(find.byKey(const ValueKey('db-design-section-options')), findsOneWidget);
    expect(find.byKey(const ValueKey('db-design-section-foreignKeys')), findsNothing);
    expect(find.byKey(const ValueKey('db-design-section-checks')), findsNothing);
    expect(find.byKey(const ValueKey('db-design-section-indexes')), findsNothing);
    expect(find.byKey(const ValueKey('db-design-col-0-unique')), findsNothing);
    final l = tester.element(find.byType(DbWorkspace)).l10n;
    expect(find.text(l.dbDesignNullable), findsWidgets);
  });

  testWidgets('save shows the SQL preview; cancel keeps the draft; open in editor runs nothing', (tester) async {
    final service = _Service();
    await _pump(tester, service);
    await _createTable(tester);
    await tester.tap(find.byKey(const ValueKey('db-design-save')));
    await settle(tester);
    expect(find.byKey(const ValueKey('db-design-save-dialog')), findsOneWidget);
    final l = tester.element(find.byType(DbWorkspace)).l10n;
    expect(find.text(l.dbDesignerSaveTitle('new_table')), findsOneWidget);
    expect(find.text(l.dbDesignerSqlPreview), findsOneWidget);
    expect(
      find.descendant(
        of: find.byKey(const ValueKey('db-design-save-sql')),
        matching: find.textContaining('CREATE TABLE "public"."new_table"'),
      ),
      findsOneWidget,
    );
    expect(find.text(l.dbDesignerTransactional), findsOneWidget);
    // «Run» waits for the vault passphrase.
    expect(tester.widget<GlassButton>(find.byKey(const ValueKey('db-design-run'))).onPressed, isNull);
    await tester.tap(find.byKey(const ValueKey('db-design-cancel')));
    await settle(tester);
    expect(find.byKey(const ValueKey('db-design-save-dialog')), findsNothing);
    expect(find.byKey(const ValueKey('db-designer-view-1')), findsOneWidget);
    expect(service.reviews, isEmpty);
    expect(service.applied, isEmpty);

    // Open in editor appends the script to the editor tab, without running.
    await tester.tap(find.byKey(const ValueKey('db-design-save')));
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('db-design-open-editor')));
    await settle(tester);
    final editor = tester.widget<TextField>(find.byKey(const ValueKey('db-query-editor')));
    expect(editor.controller!.text, contains('CREATE TABLE "public"."new_table"'));
    expect(service.applied, isEmpty);
  });

  testWidgets('run goes through the passphrase confirmation and the review ticket', (tester) async {
    final service = _Service();
    await _pump(tester, service);
    await _createTable(tester);
    await tester.tap(find.byKey(const ValueKey('db-design-save')));
    await settle(tester);
    final passphrase = find.descendant(
      of: find.byKey(const ValueKey('db-design-save-dialog')),
      matching: find.byType(TextField),
    );
    await tester.enterText(passphrase, 'wrong');
    await tester.pump();
    await tester.tap(find.byKey(const ValueKey('db-design-run')));
    await settle(tester);
    expect(service.reviews, isEmpty, reason: 'a wrong passphrase stops before the review');
    expect(service.applied, isEmpty);

    await tester.enterText(passphrase, MockCloud.demoPassphrase);
    await tester.pump();
    await tester.tap(find.byKey(const ValueKey('db-design-run')));
    await settle(tester);
    expect(service.reviews.single, contains('CREATE TABLE "public"."new_table"'));
    expect(service.applied.single.approvalTicket, 'design-ticket');
    expect(service.applied.single.text, service.reviews.single);
    expect(service.discarded, ['design-ticket']);
    expect(find.byKey(const ValueKey('db-design-save-dialog')), findsNothing);
    // Saved: the designer now edits the table on the server; no draft left.
    final l = tester.element(find.byType(DbWorkspace)).l10n;
    expect(find.text(l.dbDesignModeAlter), findsOneWidget);
    const draft = DbObjectRef(database: '', schema: 'public', kind: DbObjectKind.table, name: 'new_table');
    expect(find.byKey(ValueKey('db-draft-${draft.id}')), findsNothing);
  });

  testWidgets('a failed statement keeps the dialog open with the result', (tester) async {
    final service = _Service()..failApply = true;
    await _pump(tester, service);
    await _createTable(tester);
    await tester.tap(find.byKey(const ValueKey('db-design-save')));
    await settle(tester);
    await tester.enterText(
      find.descendant(of: find.byKey(const ValueKey('db-design-save-dialog')), matching: find.byType(TextField)),
      MockCloud.demoPassphrase,
    );
    await tester.pump();
    await tester.tap(find.byKey(const ValueKey('db-design-run')));
    await settle(tester);
    final l = tester.element(find.byType(DbWorkspace)).l10n;
    expect(find.byKey(const ValueKey('db-design-results')), findsOneWidget);
    expect(find.text(l.dbDesignerFailedRolledBack), findsOneWidget);
    expect(find.text(l.dbDesignerStatementResult(1, l.dbDesignerStatementFailed)), findsOneWidget);
  });

  testWidgets('alter mode: pending marker, changed tree node, undo and the unsaved-changes prompt', (tester) async {
    final service = _Service();
    await _pump(tester, service);
    await tester.tap(find.byKey(ValueKey('db-object-${_public.id}')));
    await settle(tester);
    await _menu(tester, _items, 'db-design-alter');
    final l = tester.element(find.byType(DbWorkspace)).l10n;
    expect(find.text(l.dbDesignModeAlter), findsOneWidget);
    expect(find.byKey(const ValueKey('db-design-pending')), findsNothing);
    expect(find.byKey(ValueKey('db-draft-${_items.id}')), findsNothing);

    await tester.tap(find.byKey(const ValueKey('db-design-add-column')));
    await settle(tester);
    expect(find.byKey(const ValueKey('db-design-pending')), findsOneWidget);
    expect(find.byKey(ValueKey('db-draft-${_items.id}')), findsOneWidget, reason: 'the table is marked changed');
    expect(find.textContaining('•'), findsOneWidget, reason: 'the tab carries the unsaved marker');

    await tester.tap(find.byKey(const ValueKey('db-design-undo')));
    await settle(tester);
    expect(find.byKey(const ValueKey('db-design-pending')), findsNothing);

    await tester.tap(find.byKey(const ValueKey('db-design-add-column')));
    await settle(tester);
    // Closing asks Save / Don't save / Cancel.
    final close = find.descendant(
      of: find.byKey(const ValueKey('db-designer-tab-1')),
      matching: find.byIcon(Icons.close_rounded),
    );
    await tester.tap(close.first);
    await settle(tester);
    expect(find.byKey(const ValueKey('db-design-unsaved')), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('db-design-unsaved-cancel')));
    await settle(tester);
    expect(find.byKey(const ValueKey('db-designer-tab-1')), findsOneWidget);
    await tester.tap(close.first);
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('db-design-unsaved-discard')));
    await settle(tester);
    expect(find.byKey(const ValueKey('db-designer-tab-1')), findsNothing);
    expect(find.byKey(ValueKey('db-draft-${_items.id}')), findsNothing);
    expect(service.applied, isEmpty);
  });

  testWidgets('primary key and unique checkboxes edit the keys; renames follow', (tester) async {
    final service = _Service();
    await _pump(tester, service);
    await _createTable(tester);
    await tester.tap(find.byKey(const ValueKey('db-design-col-0-unique')));
    await settle(tester);
    await tester.enterText(find.byKey(const ValueKey('db-design-col-0-name-input')), 'order_id');
    await settle(tester);
    final last = service.plans.last;
    expect(last.primaryKey.columns, ['order_id']);
    expect(last.uniques.single.columns, ['order_id']);
    await _section(tester, 'keys');
    expect(find.text('order_id'), findsWidgets);
  });

  testWidgets('Redis has no designer entries', (tester) async {
    expect(dbDesignerSupported(DbEngine.redis), isFalse);
    expect(dbDesignerSupported(DbEngine.mongodb), isTrue);
    expect(dbDesignerSupported(DbEngine.sqlite), isTrue);
    expect(dbDesignerSupported(DbEngine.cassandra), isTrue);
    unawaited(Future<void>.value());
  });

  test('what the design does not carry survives editing and is named in the refusal', () async {
    final design = DbTableDesign.fromJson({
      'name': 'logged',
      'columns': [
        {'name': 'id', 'data_type': 'INTEGER'},
      ],
      'unmodeled': ['triggers', 'generated_columns'],
    });
    expect(design.copy().unmodeled, ['triggers', 'generated_columns']);
    expect(DbTableDesign(name: 'plain').toJson().containsKey('unmodeled'), isFalse);
    final l = await AppLocalizations.delegate.load(const Locale('ru'));
    final text = dbDesignIssueText(
      const DbDesignIssue(error: true, code: 'rebuild_blocked', target: 'table', value: 'triggers, generated_columns'),
      l,
    );
    expect(text, contains('триггеры, вычисляемые столбцы'));
    expect(dbDesignOptionLabel('partition_key_columns', l), 'Столбцов ключа в ключе партиции');
  });
}
