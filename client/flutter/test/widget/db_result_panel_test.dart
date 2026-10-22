import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/mock/mock_cloud.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_providers.dart';
import 'package:consolecrypt/db/db_query_service.dart';
import 'package:consolecrypt/db/db_result_controller.dart';
import 'package:consolecrypt/db/db_result_edits.dart';
import 'package:consolecrypt/db/db_result_views.dart';
import 'package:consolecrypt/db/db_service.dart';
import 'package:consolecrypt/db/db_workspace.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

const _connection = DbConnection(
  id: 'connection',
  name: 'fixture-db',
  engine: DbEngine.postgresql,
  host: 'db.example.test',
  port: 5432,
  options: DbDriverOptions(engine: DbEngine.postgresql),
);

const _items = DbObjectRef(database: '', schema: 'public', kind: DbObjectKind.table, name: 'items');

/// A writable PostgreSQL session whose reads return two rows of `items`.
class _Service implements DbService, DbQueryService, DbQueryReviewService, DbRowEditService {
  _Service({this.editable = true});
  final bool editable;
  final executed = <DbQueryRequest>[], commits = <String>[], discarded = <String>[];
  List<DbRowChange> reviewed = const [];
  int reviews = 0;

  @override
  Future<DbSessionInfo> connect(String connectionId, {SecretText? temporaryPassword}) async => const DbSessionInfo(
    id: 'session',
    connectionId: 'connection',
    engine: DbEngine.postgresql,
    serverVersion: '17.0',
    production: false,
    readOnly: false,
    serverReadOnly: DbServerReadOnly.unverified,
  );
  @override
  Future<void> disconnect(String sessionId) async {}
  @override
  Future<DbTreePage> tree(String sessionId, {DbObjectRef? parent, String? cursor, bool refresh = false}) async =>
      const DbTreePage(nodes: []);
  @override
  Future<DbPreparedQuery> prepareQuery(
    String sessionId,
    String text, {
    required DbRunMode mode,
    required int selectionStart,
    required int selectionEnd,
  }) async => DbPreparedQuery(text: text);
  @override
  Future<DbQueryReview> reviewQuery(String sessionId, DbQueryRequest request) async => DbQueryReview(
    ticket: 'review-${executed.length}',
    risk: DbQueryRisk.read,
    expiresAt: DateTime.now().add(const Duration(minutes: 2)),
  );
  @override
  Future<void> discardQueryReview(String sessionId, String ticket) async => discarded.add(ticket);
  @override
  Future<String> execute(String sessionId, DbQueryRequest request) async {
    executed.add(request);
    return 'query-${executed.length}';
  }

  @override
  Stream<DbQueryEvent> watchQuery(String sessionId, String queryId) => Stream.fromIterable([
    DbQueryEvent(
      kind: DbQueryEventKind.started,
      columns: [
        DbColumn(name: 'id', typeName: 'int4', nullable: false, primaryKey: editable),
        const DbColumn(name: 'name', typeName: 'text', nullable: false),
        const DbColumn(name: 'note', typeName: 'text'),
      ],
      editableTable: editable ? _items : null,
    ),
    const DbQueryEvent(
      kind: DbQueryEventKind.rows,
      rows: [
        [DbCell(DbCellKind.intValue, '1'), DbCell(DbCellKind.text, 'alpha'), DbCell.nullValue()],
        [DbCell(DbCellKind.intValue, '2'), DbCell(DbCellKind.text, 'beta'), DbCell(DbCellKind.text, 'b')],
      ],
    ),
    const DbQueryEvent(kind: DbQueryEventKind.done),
  ]);
  @override
  Future<void> cancelQuery(String sessionId, String queryId) async {}
  @override
  Future<void> releaseQuery(String sessionId, String queryId) async {}

  @override
  Future<List<DbEditColumn>> editColumns(String sessionId, DbObjectRef table) async => const [
    DbEditColumn(name: 'id', typeName: 'integer', nullable: false, hasDefault: true, key: true),
    DbEditColumn(name: 'name', typeName: 'text', nullable: false),
    DbEditColumn(name: 'note', typeName: 'text'),
  ];

  @override
  Future<DbRowChangeReview> reviewRowChanges(String sessionId, DbObjectRef table, List<DbRowChange> changes) async {
    reviews++;
    reviewed = changes;
    final sql = [
      for (final change in changes)
        switch (change.kind) {
          DbRowChangeKind.delete =>
            'DELETE FROM "public"."items" WHERE "id" = ${change.originalPrimaryKey['id']!.text};',
          DbRowChangeKind.update =>
            'UPDATE "public"."items" SET ${change.values.keys.join(', ')} '
                'WHERE "id" = ${change.originalPrimaryKey['id']!.text};',
          DbRowChangeKind.insert => 'INSERT INTO "public"."items" (${change.values.keys.join(', ')});',
        },
    ].join('\n');
    return DbRowChangeReview(
      ticket: 'rows-$reviews',
      sql: SecretText(sql),
      expiresAt: DateTime.now().add(const Duration(minutes: 5)),
    );
  }

  @override
  Future<DbCellEditReview> reviewCellEdits(String sessionId, DbObjectRef table, List<DbCellEdit> edits) =>
      throw UnimplementedError();
  @override
  Future<void> commitCellEdits(String sessionId, String ticket) async => commits.add(ticket);
  @override
  Future<void> discardCellEdits(String sessionId, String ticket) async {}
  @override
  dynamic noSuchMethod(Invocation invocation) => throw UnimplementedError();
}

Future<void> _pump(WidgetTester tester, _Service service) async {
  final backend = testBackend();
  addTearDown(backend.dispose);
  await backend.debugSignInDemoAndUnlock();
  tester.view.physicalSize = const Size(1700, 1000);
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
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        supportedLocales: AppLocalizations.supportedLocales,
        home: Scaffold(
          body: DbWorkspace(connection: _connection, onClose: () {}),
        ),
      ),
    ),
  );
  await settle(tester);
}

/// Releasing the previous query's stream finishes a few frames later.
Future<void> _finish(WidgetTester tester) async {
  for (var i = 0; i < 5; i++) {
    await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 10)));
    await tester.pump(const Duration(milliseconds: 20));
  }
  await settle(tester);
}

Future<void> _runSelect(WidgetTester tester) async {
  await tester.enterText(find.byKey(const ValueKey('db-query-editor')), 'SELECT * FROM items');
  await tester.pump();
  await tester.tap(find.byKey(const ValueKey('db-run')));
  await _finish(tester);
}

Finder _key(String key) => find.byKey(ValueKey(key));
Finder _cell(int row, int column) => _key('db-cell-$row-$column');

Future<void> _tap(WidgetTester tester, String key) async {
  await tester.tap(_key(key));
  await settle(tester);
}

Future<void> _doubleClick(WidgetTester tester, Finder finder) async {
  await tester.tap(finder);
  await tester.pump(const Duration(milliseconds: 60));
  await tester.tap(finder);
  await settle(tester, steps: 4);
}

Future<void> _typeCell(WidgetTester tester, int row, int column, String text) async {
  await _doubleClick(tester, _cell(row, column));
  await tester.enterText(_key('db-cell-editor'), text);
  await tester.testTextInput.receiveAction(TextInputAction.done);
  await settle(tester, steps: 3);
}

Future<void> _confirm(WidgetTester tester) async {
  await tester.enterText(_key('vault-passphrase-confirm'), MockCloud.demoPassphrase);
  await tester.pump();
  await tester.tap(_key('vault-passphrase-confirm-action'));
  await _finish(tester);
}

VoidCallback? _pressed(WidgetTester tester, String key) => tester.widget<GlassIconButton>(_key(key)).onPressed;

void main() {
  testWidgets('add, duplicate and delete stay pending; save reviews with the passphrase and refreshes', (tester) async {
    final service = _Service();
    await _pump(tester, service);
    await _runSelect(tester);
    expect(find.textContaining('2 rows fetched'), findsOneWidget);
    expect(find.text('[NULL]'), findsOneWidget, reason: 'NULL is a dim [NULL]');
    expect(find.byTooltip('Key column'), findsOneWidget);

    // Add: a green "+" row with placeholders; the key is left to its default.
    await _tap(tester, 'db-row-add');
    expect(find.descendant(of: _key('db-row-number-2'), matching: find.text('+')), findsOneWidget);
    expect(find.descendant(of: _cell(2, 0), matching: find.text('[DEFAULT]')), findsOneWidget);
    expect(find.descendant(of: _cell(2, 2), matching: find.text('[NULL]')), findsOneWidget);
    expect(find.textContaining('1 change not saved'), findsOneWidget);

    // The required name is missing: nothing is reviewed.
    await _tap(tester, 'db-apply-edits');
    expect(service.reviews, 0);
    expect(find.byKey(const ValueKey('db-query-review')), findsNothing);
    expect(find.textContaining('Fill in the required columns: name'), findsOneWidget);
    await tester.pump(const Duration(seconds: 5));
    await settle(tester);
    await _typeCell(tester, 2, 1, 'gamma');

    // Duplicate the first row: values without the key.
    await tester.tap(_cell(0, 1));
    await tester.pump(const Duration(milliseconds: 400));
    await _tap(tester, 'db-row-duplicate');
    expect(find.descendant(of: _cell(3, 1), matching: find.text('alpha')), findsOneWidget);
    expect(find.descendant(of: _cell(3, 0), matching: find.text('[DEFAULT]')), findsOneWidget);

    // Delete the second loaded row: struck through until saved.
    await tester.tap(_cell(1, 1));
    await tester.pump(const Duration(milliseconds: 400));
    await _tap(tester, 'db-row-delete');
    final beta = tester.widget<Text>(find.descendant(of: _cell(1, 1), matching: find.text('beta')));
    expect(beta.style?.decoration, TextDecoration.lineThrough);
    // A changed cell of a loaded row joins the save.
    await _typeCell(tester, 0, 2, 'first');
    expect(find.textContaining('4 changes not saved'), findsOneWidget);
    expect(service.executed, hasLength(1), reason: 'nothing runs before Save');

    await _tap(tester, 'db-apply-edits');
    expect(find.byKey(const ValueKey('db-query-review')), findsOneWidget);
    expect(service.reviewed.map((c) => c.kind), [
      DbRowChangeKind.delete,
      DbRowChangeKind.update,
      DbRowChangeKind.insert,
      DbRowChangeKind.insert,
    ]);
    expect(service.reviewed[2].values.keys, ['name'], reason: 'the key is left to the server');
    expect(service.reviewed[3].values['name']!.text, 'alpha');
    expect(service.reviewed[3].values.containsKey('id'), isFalse);
    expect(service.commits, isEmpty);
    await _confirm(tester);
    expect(service.commits, ['rows-1']);
    expect(service.executed, hasLength(2), reason: 'the read query runs again');
    expect(find.textContaining('not saved'), findsNothing);
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));

  testWidgets('cancel discards; Show SQL opens the statements and adds them to the editor', (tester) async {
    final service = _Service();
    await _pump(tester, service);
    await _runSelect(tester);
    await _typeCell(tester, 1, 1, 'bravo');
    await _tap(tester, 'db-result-save-menu');
    await tester.tap(find.text('Show SQL'));
    await settle(tester);
    expect(find.byKey(const ValueKey('db-ddl-dialog')), findsOneWidget);
    expect(find.textContaining('UPDATE "public"."items"'), findsWidgets);
    await tester.tap(_key('db-ddl-open'));
    await settle(tester);
    final editor = tester.widget<TextField>(_key('db-query-editor')).controller!.text;
    expect(editor, contains('UPDATE "public"."items" SET name WHERE "id" = 2;'));
    expect(service.commits, isEmpty, reason: 'showing never commits');

    expect(find.textContaining('1 change not saved'), findsOneWidget);
    await _tap(tester, 'db-discard-edits');
    expect(find.textContaining('not saved'), findsNothing);
    expect(find.descendant(of: _cell(1, 1), matching: find.text('beta')), findsOneWidget);
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));

  testWidgets('without a key editing actions are off and say why', (tester) async {
    await _pump(tester, _Service(editable: false));
    await _runSelect(tester);
    for (final key in ['db-row-add', 'db-row-duplicate', 'db-row-delete', 'db-row-edit']) {
      expect(_pressed(tester, key), isNull, reason: key);
    }
    expect(
      find.byTooltip('Add a row — Editing needs a query of one table with a primary or unique key'),
      findsOneWidget,
    );
    await _doubleClick(tester, _cell(0, 1));
    expect(_key('db-cell-editor'), findsNothing);
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));

  testWidgets('text and record views; navigation moves the record and edits stay pending', (tester) async {
    await _pump(tester, _Service());
    await _runSelect(tester);
    await _tap(tester, 'db-view-text');
    final text = tester.widget<SelectableText>(_key('db-result-text-body')).data!;
    expect(text, contains('| id | name  | note   |'));
    expect(text, contains('|  1 | alpha | [NULL] |'));

    await _tap(tester, 'db-view-record');
    expect(find.text('Row 1 of 2'), findsOneWidget);
    await _tap(tester, 'db-row-next');
    expect(find.text('Row 2 of 2'), findsOneWidget);
    expect(_pressed(tester, 'db-row-next'), isNull);
    await tester.tap(_key('db-record-value-1'));
    await settle(tester);
    await tester.enterText(find.descendant(of: _key('db-record-editor-1'), matching: find.byType(TextField)), 'b2');
    await tester.testTextInput.receiveAction(TextInputAction.done);
    await settle(tester);
    expect(find.textContaining('1 change not saved'), findsOneWidget);
    await _tap(tester, 'db-row-first');
    expect(find.text('Row 1 of 2'), findsOneWidget);

    // The view is remembered per tab.
    await tester.tap(_key('glass-tab-add'));
    await settle(tester);
    expect(_key('db-result-record'), findsNothing);
    await tester.tap(_key('db-query-tab-1'));
    await settle(tester);
    expect(_key('db-result-record'), findsOneWidget);
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));

  testWidgets('running again with unsaved changes asks: cancel keeps them, discard runs', (tester) async {
    final service = _Service();
    await _pump(tester, service);
    await _runSelect(tester);
    await _typeCell(tester, 0, 1, 'changed');
    await tester.tap(_key('db-run'));
    await settle(tester);
    expect(_key('db-pending-prompt'), findsOneWidget);
    await _tap(tester, 'db-pending-cancel');
    expect(service.executed, hasLength(1));
    expect(find.textContaining('1 change not saved'), findsOneWidget);

    await tester.tap(_key('db-result-refresh'));
    await settle(tester);
    expect(_key('db-pending-prompt'), findsOneWidget);
    await tester.tap(_key('db-pending-discard'));
    await _finish(tester);
    expect(service.executed, hasLength(2));
    expect(find.textContaining('not saved'), findsNothing);
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));

  testWidgets('auto-refresh repeats a read and pauses while changes are pending', (tester) async {
    final service = _Service();
    await _pump(tester, service);
    await _runSelect(tester);
    await _tap(tester, 'db-result-auto-refresh');
    await tester.tap(_key('db-auto-refresh-5'));
    await settle(tester);
    await tester.pump(const Duration(seconds: 6));
    await _finish(tester);
    expect(service.executed, hasLength(2));
    await _typeCell(tester, 0, 1, 'changed');
    await tester.pump(const Duration(seconds: 11));
    await _finish(tester);
    expect(service.executed, hasLength(2), reason: 'pending changes pause auto-refresh');
    await _tap(tester, 'db-discard-edits');
    await _tap(tester, 'db-result-auto-refresh');
    await tester.tap(_key('db-auto-refresh-0'));
    await settle(tester);
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));

  test('the text table aligns columns, right-aligns numbers and masks secrets', () {
    final result =
        DbResultSet(
            index: 0,
            columns: const [
              DbColumn(name: 'n', typeName: 'int4'),
              DbColumn(name: 'word', typeName: 'text'),
              DbColumn(name: 'pin', typeName: 'text', sensitive: true),
            ],
          )
          ..rows.addAll(const [
            [DbCell(DbCellKind.intValue, '7'), DbCell(DbCellKind.text, 'a\nb'), DbCell(DbCellKind.text, '1234')],
            [DbCell(DbCellKind.intValue, '1000'), DbCell.nullValue(), DbCell.nullValue()],
          ]);
    final edits = DbResultEdits()..addRow(result);
    final text = dbResultText(result, edits, nullText: '[NULL]', placeholder: '[DEFAULT]', locale: 'en');
    final lines = text.split('\n');
    expect(lines[1].startsWith('| n '), isTrue);
    expect(lines[3], contains('a b'));
    expect(lines[3], contains('••••'));
    expect(lines[3], isNot(contains('1234')));
    expect(lines[4], contains('[NULL]'));
    final widths = lines.map((line) => line.length).toSet();
    expect(widths, hasLength(1), reason: 'every line has the same width');
    expect(RegExp(r'\|\s+7 \|').hasMatch(lines[3]), isTrue, reason: 'numbers are right-aligned');
  });
}
