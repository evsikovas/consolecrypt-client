import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/db/db_ddl_dialog.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_providers.dart';
import 'package:consolecrypt/db/db_query_service.dart';
import 'package:consolecrypt/db/db_result_controller.dart';
import 'package:consolecrypt/db/db_result_grid.dart';
import 'package:consolecrypt/db/db_value_dialog.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

const _columns = [
  DbColumn(name: 'id', typeName: 'int4', nullable: false, primaryKey: true),
  DbColumn(name: 'name', typeName: 'text'),
  DbColumn(name: 'total', typeName: 'numeric'),
];

DbResultSet _result() => DbResultSet(index: 0, columns: _columns)
  ..rows.addAll(const [
    [DbCell(DbCellKind.intValue, '1'), DbCell(DbCellKind.text, 'alpha'), DbCell(DbCellKind.decimal, '1.50')],
    [DbCell(DbCellKind.intValue, '2'), DbCell(DbCellKind.text, 'beta'), DbCell(DbCellKind.decimal, '2.50')],
    [DbCell(DbCellKind.intValue, '3'), DbCell(DbCellKind.text, 'gamma'), DbCell.nullValue()],
    [DbCell(DbCellKind.intValue, '4'), DbCell(DbCellKind.text, 'delta'), DbCell(DbCellKind.decimal, '4')],
  ]);

/// A grid with the callbacks the workspace passes; [editable] enables
/// in-place editing.
final class _Grid {
  final edits = <(int, int, DbCell)>[];
  final copiedCells = <(int, int)>[];
  final copied = <DbResultSelection>[];
  List<int> selection = const [];
  final pending = <(int, int), DbCell>{};
}

Future<_Grid> _pumpGrid(WidgetTester tester, {bool editable = true}) async {
  final grid = _Grid();
  final backend = testBackend();
  addTearDown(backend.dispose);
  tester.view.physicalSize = const Size(1000, 700);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  final result = _result();
  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        appServicesProvider.overrideWithValue(backend.services),
        dbScopeProvider.overrideWithValue(const DbScope('profile', true)),
      ],
      child: MaterialApp(
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        supportedLocales: AppLocalizations.supportedLocales,
        home: Scaffold(
          body: StatefulBuilder(
            builder: (context, setState) => DbResultGrid(
              result: result,
              pending: Map.of(grid.pending),
              onCopy: grid.copied.add,
              onCopyCell: (row, column) => grid.copiedCells.add((row, column)),
              onSelection: (rows) => grid.selection = rows,
              onCellEdit: editable
                  ? (row, column, value) => setState(() {
                      grid.edits.add((row, column, value));
                      grid.pending[(row, column)] = value;
                    })
                  : null,
            ),
          ),
        ),
      ),
    ),
  );
  await settle(tester);
  return grid;
}

Finder _cell(int row, int column) => find.byKey(ValueKey('db-cell-$row-$column'));

Future<void> _click(WidgetTester tester, Finder finder, {LogicalKeyboardKey? modifier}) async {
  if (modifier != null) await tester.sendKeyDownEvent(modifier);
  await tester.tap(finder);
  // A single click is told from a double click after the double-tap timeout.
  await tester.pump(const Duration(milliseconds: 400));
  if (modifier != null) await tester.sendKeyUpEvent(modifier);
  await tester.pump();
}

Future<void> _doubleClick(WidgetTester tester, Finder finder) async {
  await tester.tap(finder);
  await tester.pump(const Duration(milliseconds: 60));
  await tester.tap(finder);
  await settle(tester, steps: 4);
}

void main() {
  group('cell values bind with the column type', () {
    test('types from result metadata', () {
      DbCellKind kind(String type, [DbCell? sample]) => dbColumnKind(
        DbColumn(name: 'c', typeName: type),
        sample: sample,
      );
      expect(kind('int4'), DbCellKind.intValue);
      expect(kind('MYSQL_TYPE_LONGLONG'), DbCellKind.intValue);
      expect(kind('Nullable(UInt64)'), DbCellKind.uint);
      expect(kind('numeric'), DbCellKind.decimal);
      expect(kind('Decimal(10, 2)'), DbCellKind.decimal);
      expect(kind('float8'), DbCellKind.float);
      expect(kind('bool'), DbCellKind.boolValue);
      expect(kind('jsonb'), DbCellKind.json);
      expect(kind('timestamptz'), DbCellKind.timestamp);
      expect(kind('MYSQL_TYPE_DATETIME'), DbCellKind.timestamp);
      expect(kind('date'), DbCellKind.date);
      expect(kind('bytea'), DbCellKind.binary);
      expect(kind('interval'), DbCellKind.text);
      expect(kind('_int4'), DbCellKind.text);
      expect(kind('uuid'), DbCellKind.text);
      expect(kind('MYSQL_TYPE_BLOB', const DbCell(DbCellKind.text, 'x')), DbCellKind.text);
      expect(kind('MYSQL_TYPE_BLOB', const DbCell(DbCellKind.binary, 'eA==')), DbCellKind.binary);
      expect(kind('mystery', const DbCell(DbCellKind.json, '{}')), DbCellKind.json);
    });
    test('typed input is validated, never interpolated', () {
      expect(dbCellFromInput(DbCellKind.intValue, ' 42 ')?.text, '42');
      expect(dbCellFromInput(DbCellKind.intValue, '4.2'), isNull);
      expect(dbCellFromInput(DbCellKind.uint, '-1'), isNull);
      expect(dbCellFromInput(DbCellKind.decimal, '-1.25e3')?.kind, DbCellKind.decimal);
      expect(dbCellFromInput(DbCellKind.decimal, '1,5'), isNull);
      expect(dbCellFromInput(DbCellKind.float, 'NaN')?.kind, DbCellKind.float);
      expect(dbCellFromInput(DbCellKind.boolValue, 'Yes')?.value, true);
      expect(dbCellFromInput(DbCellKind.boolValue, 'maybe'), isNull);
      expect(dbCellFromInput(DbCellKind.json, '{"a": 1}')?.kind, DbCellKind.json);
      expect(dbCellFromInput(DbCellKind.json, '{a: 1}'), isNull);
      expect(dbCellFromInput(DbCellKind.binary, 'not base64!'), isNull);
      expect(dbCellFromInput(DbCellKind.text, "  it's; DROP  ")?.text, "  it's; DROP  ");
    });
  });

  test('DDL splits into statements at top-level semicolons only', () {
    const ddl =
        'CREATE TABLE "a;b" (\n  id int DEFAULT \';\'\n);\n-- note; here\nCREATE INDEX i ON t (x);\n'
        r'CREATE FUNCTION f() RETURNS int AS $body$ SELECT 1; $body$ LANGUAGE sql;';
    final parts = splitSqlStatements(ddl);
    expect(parts, hasLength(3));
    expect(parts[0], startsWith('CREATE TABLE "a;b"'));
    expect(parts[1], startsWith('-- note; here\nCREATE INDEX'));
    expect(parts[2], endsWith(r'$body$ LANGUAGE sql;'));
  });

  testWidgets('DDL dialog is a numbered code block per statement with copy and open in editor', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    tester.view.physicalSize = const Size(1200, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    final value = SecretText(
      'CREATE TABLE public.items (\n    id integer NOT NULL,\n    note text\n);\n'
      'CREATE INDEX items_note ON public.items USING btree (note);\nCREATE UNIQUE INDEX items_id ON public.items (id);',
    );
    bool? opened;
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          appServicesProvider.overrideWithValue(backend.services),
          dbScopeProvider.overrideWithValue(const DbScope('profile', true)),
        ],
        child: MaterialApp(
          localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
          supportedLocales: AppLocalizations.supportedLocales,
          home: Scaffold(
            body: Builder(
              builder: (context) => TextButton(
                onPressed: () async => opened = await showDialog<bool>(
                  context: context,
                  builder: (_) => DbDdlDialog(title: 'items', value: value, engine: DbEngine.postgresql),
                ),
                child: const Text('open'),
              ),
            ),
          ),
        ),
      ),
    );
    await tester.tap(find.text('open'));
    await settle(tester);
    expect(find.byKey(const ValueKey('db-ddl-dialog')), findsOneWidget);
    for (var b = 0; b < 3; b++) {
      expect(find.byKey(ValueKey('db-code-block-$b')), findsOneWidget);
    }
    // Lines wrap by default; every source line is numbered once.
    expect(find.byKey(const ValueKey('db-code-horizontal')), findsNothing);
    for (var n = 1; n <= 6; n++) {
      expect(find.text('$n'), findsOneWidget);
    }
    // The switch turns wrapping off: the code scrolls sideways.
    await tester.tap(find.byKey(const ValueKey('db-code-wrap')));
    await settle(tester);
    expect(backend.services.settings.currentLocal.dbCodeWrap, isFalse, reason: 'remembered locally');
    expect(find.byKey(const ValueKey('db-code-horizontal')), findsOneWidget);
    final block = tester.widget<Text>(find.byKey(const ValueKey('db-code-block-0')));
    expect(block.softWrap, isFalse);
    expect(find.text('1\n2\n3\n4'), findsOneWidget);
    expect(find.text('5'), findsOneWidget);
    expect(find.text('6'), findsOneWidget);
    // Keywords are coloured like in the editor.
    final spans = <String>[];
    block.textSpan!.visitChildren((span) {
      if (span is TextSpan && span.style?.color != null) spans.add(span.text ?? '');
      return true;
    });
    expect(spans, containsAll(['CREATE', 'TABLE', 'NOT', 'NULL']));
    await tester.tap(find.byKey(const ValueKey('db-ddl-copy')));
    await settle(tester);
    expect(tester.takeException(), isNull);
    await tester.tap(find.byKey(const ValueKey('db-ddl-open')));
    await settle(tester);
    expect(opened, isTrue);
  });

  testWidgets('DDL dialog fits a narrow window: long lines wrap, or scroll sideways when wrapping is off', (
    tester,
  ) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    tester.view.physicalSize = const Size(420, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    final value = SecretText(
      'CREATE TABLE public.team (\n    id bigint NOT NULL,\n'
      '    created_at timestamp(6) with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,\n    note text\n);\n'
      'CREATE UNIQUE INDEX team_access_code_key ON public.team USING btree (access_code) WHERE (deleted_at IS NULL);',
    );
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          appServicesProvider.overrideWithValue(backend.services),
          dbScopeProvider.overrideWithValue(const DbScope('profile', true)),
        ],
        child: MaterialApp(
          localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
          supportedLocales: AppLocalizations.supportedLocales,
          home: Scaffold(
            body: Builder(
              builder: (context) => TextButton(
                onPressed: () => showDialog<bool>(
                  context: context,
                  builder: (_) => DbDdlDialog(title: 'team', value: value, engine: DbEngine.postgresql),
                ),
                child: const Text('open'),
              ),
            ),
          ),
        ),
      ),
    );
    await tester.tap(find.text('open'));
    await settle(tester);
    expect(tester.takeException(), isNull, reason: 'no overflow');
    // The end of the long column line is on screen, on a continuation line.
    final tail = find.textContaining('CURRENT_TIMESTAMP NOT NULL', findRichText: true);
    expect(tail, findsWidgets);
    expect(tester.getRect(tail.last).right, lessThanOrEqualTo(420));
    // The code area is as tall as the code: no empty space below it.
    final code = find.byKey(const ValueKey('db-code-vertical'));
    final position = Scrollable.of(tester.element(find.descendant(of: code, matching: find.byType(Column)).first))
        .position;
    expect(position.maxScrollExtent, 0);
    expect(tester.getSize(code).height, lessThan(800 * .85));
    final content = find.descendant(of: code, matching: find.byType(Stack)).first;
    expect(tester.getSize(code).height, closeTo(tester.getSize(content).height, 1));

    await tester.tap(find.byKey(const ValueKey('db-code-wrap')));
    await settle(tester);
    expect(tester.takeException(), isNull);
    expect(find.byKey(const ValueKey('db-code-horizontal')), findsOneWidget);
    expect(find.byType(Scrollbar), findsWidgets, reason: 'a visible horizontal scrollbar');
  });

  group('selection', () {
    testWidgets('a cell click focuses the cell without selecting its row', (tester) async {
      final grid = await _pumpGrid(tester);
      await _click(tester, _cell(1, 1));
      expect(grid.selection, isEmpty);
      final box = tester.widget<Container>(find.descendant(of: _cell(1, 1), matching: find.byType(Container)).first);
      expect((box.decoration! as BoxDecoration).border, isA<Border>().having((b) => b.top.width, 'top', 1.5));
      // ⌘C / Ctrl+C copies the focused cell.
      await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
      await tester.sendKeyEvent(LogicalKeyboardKey.keyC);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
      await tester.pump();
      expect(grid.copiedCells, [(1, 1)]);
    });

    testWidgets('row numbers select, Shift extends, Ctrl toggles and Esc clears', (tester) async {
      final grid = await _pumpGrid(tester);
      await _click(tester, find.byKey(const ValueKey('db-row-number-0')));
      expect(grid.selection, [0]);
      await _click(tester, find.byKey(const ValueKey('db-row-number-2')), modifier: LogicalKeyboardKey.shiftLeft);
      expect(grid.selection, [0, 1, 2]);
      await _click(tester, find.byKey(const ValueKey('db-row-number-1')), modifier: LogicalKeyboardKey.controlLeft);
      expect(grid.selection, [0, 2]);
      await _click(tester, find.byKey(const ValueKey('db-row-number-3')), modifier: LogicalKeyboardKey.controlLeft);
      expect(grid.selection, [0, 2, 3]);
      // A plain click on a row number selects just that row.
      await _click(tester, find.byKey(const ValueKey('db-row-number-1')));
      expect(grid.selection, [1]);
      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pump();
      expect(grid.selection, isEmpty);
      // The toolbar copy uses the whole result again.
      await tester.tap(find.byTooltip('Copy loaded result'));
      await tester.pump();
      expect(grid.copied.single.rows, isNull);
    });
  });

  group('in-place editing', () {
    testWidgets('double-click edits, Enter adds a typed pending value, Esc cancels', (tester) async {
      final grid = await _pumpGrid(tester);
      await _doubleClick(tester, _cell(0, 2));
      final editor = find.byKey(const ValueKey('db-cell-editor'));
      expect(editor, findsOneWidget);
      expect(tester.widget<TextField>(editor).controller!.text, '1.50');
      // No value-type picker: the numeric column only takes numbers.
      await tester.enterText(editor, 'twelve');
      await tester.testTextInput.receiveAction(TextInputAction.done);
      await tester.pump();
      expect(grid.edits, isEmpty);
      expect(editor, findsOneWidget);
      await tester.enterText(editor, '12.75');
      await tester.testTextInput.receiveAction(TextInputAction.done);
      await settle(tester, steps: 3);
      expect(grid.edits.single.$3.kind, DbCellKind.decimal);
      expect(grid.edits.single.$3.text, '12.75');
      expect(editor, findsNothing);
      // The pending value is shown (highlighted) in place of the loaded one.
      expect(find.text('12.75'), findsOneWidget);
      expect(find.byTooltip('Changed, not applied yet'), findsOneWidget);
      // Esc leaves a cell unchanged.
      await _doubleClick(tester, _cell(1, 1));
      await tester.enterText(editor, 'changed');
      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await settle(tester, steps: 3);
      expect(editor, findsNothing);
      expect(grid.edits, hasLength(1));
      expect(find.text('beta'), findsOneWidget);
    });

    testWidgets('Tab moves to the next cell and NULL is an explicit toggle', (tester) async {
      final grid = await _pumpGrid(tester);
      await _doubleClick(tester, _cell(1, 1));
      final editor = find.byKey(const ValueKey('db-cell-editor'));
      await tester.enterText(editor, 'bravo');
      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      await settle(tester, steps: 3);
      expect(grid.edits.single.$1, 1);
      expect(grid.edits.single.$3.text, 'bravo');
      // Now editing (1, 2): set it to NULL.
      expect(tester.widget<TextField>(editor).controller!.text, '2.50');
      await tester.tap(find.byKey(const ValueKey('db-cell-editor-null')));
      await tester.pump();
      await tester.testTextInput.receiveAction(TextInputAction.done);
      await settle(tester, steps: 3);
      expect(grid.edits.last.$2, 2);
      expect(grid.edits.last.$3.kind, DbCellKind.nullValue);
    });

    testWidgets('without write access double-click opens the value viewer', (tester) async {
      await _pumpGrid(tester, editable: false);
      await _doubleClick(tester, _cell(0, 1));
      expect(find.byKey(const ValueKey('db-cell-editor')), findsNothing);
      expect(find.byType(DbValueDialog), findsOneWidget);
    });
  });
}
