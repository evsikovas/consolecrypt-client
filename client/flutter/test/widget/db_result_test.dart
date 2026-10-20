import 'dart:async';

import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_query_service.dart';
import 'package:consolecrypt/db/db_result_controller.dart';
import 'package:consolecrypt/db/db_result_grid.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

const _columns = [DbColumn(name: 'value', typeName: 'numeric')];

class _Service implements DbQueryService {
  // Every test registers close as teardown, including cancelled subscriptions.
  // ignore: close_sinks
  final events = StreamController<DbQueryEvent>.broadcast();
  final executed = <DbQueryRequest>[], cancelled = <String>[], released = <String>[];
  final loaded = <(String, int)>[];
  Completer<String>? pending;
  @override
  Future<String> execute(String sessionId, DbQueryRequest request) async {
    executed.add(request);
    return pending?.future ?? 'cursor-1';
  }

  @override
  Stream<DbQueryEvent> watchQuery(String sessionId, String queryId) => events.stream;
  @override
  Future<void> loadMore(String sessionId, String queryId, {int rows = 1000}) async => loaded.add((queryId, rows));
  @override
  Future<void> cancelQuery(String sessionId, String queryId) async => cancelled.add(queryId);
  @override
  Future<void> releaseQuery(String sessionId, String queryId) async => released.add(queryId);
  @override
  dynamic noSuchMethod(Invocation invocation) => throw UnimplementedError();
}

void main() {
  test('numeric sort never collapses adjacent integers or exact decimal fractions', () {
    for (final values in [
      ['18446744073709551614', '18446744073709551615'],
      ['99999999999999999999.00000000000000000001', '99999999999999999999.00000000000000000002'],
      ['-123.00001', '-123.000001'],
      ['1e-999999999999999999999', '1e-999999999999999999998'],
    ]) {
      expect(compareDbCells(DbCell(DbCellKind.decimal, values[0]), DbCell(DbCellKind.decimal, values[1])), lessThan(0));
    }
    expect(compareDbCells(const DbCell(DbCellKind.decimal, '001.2300'), const DbCell(DbCellKind.decimal, '1.23')), 0);
    expect(compareDbCells(const DbCell(DbCellKind.intValue, '-0'), const DbCell(DbCellKind.intValue, '0')), 0);
  });

  testWidgets('load more advances the same cursor and scripts keep separate result sets', (tester) async {
    final service = _Service();
    final result = DbResultController(service: service, sessionId: 'session', isCurrent: () => true);
    addTearDown(service.events.close);
    addTearDown(result.dispose);
    await result.execute(const DbQueryRequest(text: 'SELECT 1; SELECT 2'));
    service.events.add(const DbQueryEvent(kind: DbQueryEventKind.started, columns: _columns));
    service.events.add(
      const DbQueryEvent(
        kind: DbQueryEventKind.rows,
        rows: [
          [DbCell(DbCellKind.intValue, '1')],
        ],
      ),
    );
    service.events.add(const DbQueryEvent(kind: DbQueryEventKind.paused, hasMore: true, loadedRows: 1));
    await tester.pump(const Duration(milliseconds: 20));
    expect(result.running, isFalse);
    await result.more(rows: 10000);
    expect(service.loaded, [('cursor-1', 1000)]);
    expect(service.executed, hasLength(1));
    service.events.add(
      const DbQueryEvent(
        kind: DbQueryEventKind.rows,
        rows: [
          [DbCell(DbCellKind.intValue, '2')],
        ],
      ),
    );
    service.events.add(const DbQueryEvent(kind: DbQueryEventKind.statementDone, elapsedMs: 25));
    service.events.add(const DbQueryEvent(kind: DbQueryEventKind.started, resultIndex: 1, columns: _columns));
    service.events.add(
      const DbQueryEvent(
        kind: DbQueryEventKind.rows,
        resultIndex: 1,
        rows: [
          [DbCell(DbCellKind.intValue, '3')],
        ],
      ),
    );
    service.events.add(const DbQueryEvent(kind: DbQueryEventKind.done));
    await tester.pump(const Duration(milliseconds: 20));
    expect(result.results.map((r) => r.rows.length), [2, 1]);
    expect(result.running, isFalse);
  });

  testWidgets('result memory cap cancels upstream before retaining the oversized row', (tester) async {
    final service = _Service();
    final result = DbResultController(service: service, sessionId: 'session', isCurrent: () => true, memoryLimit: 1024);
    addTearDown(service.events.close);
    addTearDown(result.dispose);
    await result.execute(const DbQueryRequest(text: 'SELECT value'));
    service.events.add(const DbQueryEvent(kind: DbQueryEventKind.started, columns: _columns));
    service.events.add(
      DbQueryEvent(
        kind: DbQueryEventKind.rows,
        rows: [
          [DbCell(DbCellKind.text, 'x' * 2000)],
        ],
      ),
    );
    await tester.pump(const Duration(milliseconds: 20));
    expect(result.truncated, isTrue);
    expect(result.results.single.rows, isEmpty);
    expect(service.cancelled, ['cursor-1']);
  });

  testWidgets('late native handle after disposal is cancelled and released', (tester) async {
    final service = _Service()..pending = Completer<String>();
    final result = DbResultController(service: service, sessionId: 'session', isCurrent: () => true);
    addTearDown(service.events.close);
    final running = result.execute(const DbQueryRequest(text: 'SELECT value'));
    await tester.pump();
    result.dispose();
    service.pending!.complete('late-cursor');
    await running;
    await tester.pump();
    expect(service.cancelled, ['late-cursor']);
    expect(service.released, ['late-cursor']);
    expect(result.results, isEmpty);
  });

  testWidgets('revoked scope drops queued row events and erases results on disposal', (tester) async {
    var current = true;
    final service = _Service();
    final result = DbResultController(service: service, sessionId: 'session', isCurrent: () => current);
    addTearDown(service.events.close);
    await result.execute(const DbQueryRequest(text: 'SELECT value'));
    service.events.add(const DbQueryEvent(kind: DbQueryEventKind.started, columns: _columns));
    await tester.pump();
    current = false;
    service.events.add(
      const DbQueryEvent(
        kind: DbQueryEventKind.rows,
        rows: [
          [DbCell(DbCellKind.text, 'canary')],
        ],
      ),
    );
    await tester.pump(const Duration(milliseconds: 20));
    expect(result.results.single.rows, isEmpty);
    result.dispose();
    await tester.pump();
    expect(result.results, isEmpty);
    expect(service.cancelled, ['cursor-1']);
  });

  testWidgets('grid virtualizes 100000 rows and 1000 columns in both directions', (tester) async {
    final result = DbResultSet(
      index: 0,
      columns: List.generate(1000, (i) => DbColumn(name: 'column-$i', typeName: 'int')),
    );
    final row = List.generate(1000, (i) => DbCell(DbCellKind.intValue, '$i'));
    result.rows.addAll(List.filled(100000, row));
    await _pumpGrid(tester, result);
    final cells = find.byWidgetPredicate(
      (w) => w.key is ValueKey<String> && (w.key! as ValueKey<String>).value.startsWith('db-cell-'),
    );
    expect(cells.evaluate().length, lessThan(250));
    final vertical = tester.widget<ListView>(find.byKey(const ValueKey('db-result-grid'))).controller!;
    vertical.jumpTo(vertical.position.maxScrollExtent);
    await tester.pump();
    expect(find.byKey(const ValueKey('db-cell-99999-0')), findsOneWidget);
    final horizontal = tester
        .widget<SingleChildScrollView>(find.byKey(const ValueKey('db-result-horizontal')))
        .controller!;
    horizontal.jumpTo(horizontal.position.maxScrollExtent);
    await tester.pump();
    expect(find.byKey(const ValueKey('db-cell-99999-999')), findsOneWidget);
    expect(cells.evaluate().length, lessThan(250));
    expect(tester.takeException(), isNull);
  });

  for (final language in ['en', 'ru']) {
    testWidgets('phone result cards remain readable at large text in $language', (tester) async {
      final result = DbResultSet(
        index: 0,
        columns: const [
          DbColumn(name: 'big_number', typeName: 'uint64'),
          DbColumn(name: 'password', typeName: 'text', sensitive: true),
        ],
      );
      result.rows.add(const [
        DbCell(DbCellKind.uint, '18446744073709551615'),
        DbCell(DbCellKind.text, 'canary-secret'),
      ]);
      await _pumpGrid(tester, result, language: language, size: const Size(412, 915), scale: 1.5);
      expect(find.byKey(const ValueKey('db-result-records')), findsOneWidget);
      expect(find.text('18446744073709551615'), findsOneWidget);
      expect(find.text('canary-secret'), findsNothing);
      expect(tester.takeException(), isNull);
    });
  }
}

Future<void> _pumpGrid(
  WidgetTester tester,
  DbResultSet result, {
  String language = 'en',
  Size size = const Size(1000, 700),
  double scale = 1,
}) async {
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    MaterialApp(
      locale: Locale(language),
      localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
      supportedLocales: AppLocalizations.supportedLocales,
      builder: (context, child) => MediaQuery(
        data: MediaQuery.of(context).copyWith(textScaler: TextScaler.linear(scale)),
        child: child!,
      ),
      home: Scaffold(body: DbResultGrid(result: result)),
    ),
  );
  await settle(tester);
}
