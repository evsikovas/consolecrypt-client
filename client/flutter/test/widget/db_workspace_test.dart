import 'dart:async';

import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/db/db_ai_service.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_providers.dart';
import 'package:consolecrypt/db/db_query_service.dart';
import 'package:consolecrypt/db/db_service.dart';
import 'package:consolecrypt/db/db_workspace.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

final _scope = NotifierProvider<_Scope, DbScope>(_Scope.new);

class _Scope extends Notifier<DbScope> {
  @override
  DbScope build() => const DbScope('profile', true);
  void lock() => state = const DbScope('profile', false);
}

const _connection = DbConnection(
  id: 'connection',
  name: 'fixture-db',
  engine: DbEngine.postgresql,
  host: 'db.example.test',
  port: 5432,
  options: DbDriverOptions(engine: DbEngine.postgresql),
);
DbSessionInfo _session({bool production = true, bool readOnly = true}) => DbSessionInfo(
  id: 'session',
  connectionId: 'connection',
  engine: DbEngine.postgresql,
  serverVersion: '17.0',
  production: production,
  readOnly: readOnly,
  serverReadOnly: DbServerReadOnly.enforced,
);

class _Service implements DbService, DbQueryService, DbQueryReviewService, DbAiService {
  _Service({this.production = true, this.readOnly = true});
  final bool production;
  bool readOnly;
  final executed = <DbQueryRequest>[], discarded = <String>[], disconnected = <String>[], cancelled = <String>[];
  final preparations = <(String, DbRunMode, int, int)>[];
  final treeParents = <DbObjectRef?>[];
  final consents = <DbAiConsent>[];
  final aiCancelled = <String>[];
  DbQueryRisk risk = DbQueryRisk.read;
  Completer<DbQueryReview>? pendingReview;
  Completer<DbTreePage>? pendingTree;
  List<String> parameters = [];
  @override
  Future<DbSessionInfo> connect(String connectionId, {SecretText? temporaryPassword}) async =>
      _session(production: production, readOnly: readOnly);
  @override
  Future<void> disconnect(String sessionId) async => disconnected.add(sessionId);
  bool mixedTree = false;
  @override
  Future<DbTreePage> tree(String sessionId, {DbObjectRef? parent, String? cursor, bool refresh = false}) async {
    treeParents.add(parent);
    if (mixedTree) {
      return DbTreePage(
        nodes: parent == null
            ? const [
                DbTreeNode(
                  reference: DbObjectRef(database: 'fixture', kind: DbObjectKind.table, name: 'items'),
                  hasChildren: true,
                ),
                DbTreeNode(
                  reference: DbObjectRef(database: 'fixture', kind: DbObjectKind.view, name: 'orders_view'),
                  hasChildren: false,
                ),
              ]
            : [
                DbTreeNode(
                  reference: DbObjectRef(
                    database: 'fixture',
                    kind: DbObjectKind.column,
                    name: 'id',
                    parentName: parent.name,
                  ),
                  hasChildren: false,
                  primaryKey: true,
                ),
                DbTreeNode(
                  reference: DbObjectRef(
                    database: 'fixture',
                    kind: DbObjectKind.indexObject,
                    name: 'items_pkey',
                    parentName: parent.name,
                  ),
                  hasChildren: false,
                ),
              ],
      );
    }
    return pendingTree?.future ??
        const DbTreePage(
          nodes: [
            DbTreeNode(
              reference: DbObjectRef(database: 'fixture', kind: DbObjectKind.table, name: 'items'),
              hasChildren: false,
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
  }) async {
    preparations.add((text, mode, selectionStart, selectionEnd));
    return DbPreparedQuery(text: text, parameterNames: parameters);
  }

  @override
  Future<DbQueryReview> reviewQuery(String sessionId, DbQueryRequest request) async =>
      pendingReview?.future ??
      DbQueryReview(ticket: 'review', risk: risk, expiresAt: DateTime.now().add(const Duration(minutes: 2)));
  @override
  Future<void> discardQueryReview(String sessionId, String ticket) async => discarded.add(ticket);
  @override
  Future<String> execute(String sessionId, DbQueryRequest request) async {
    executed.add(request);
    return 'query';
  }

  @override
  Stream<DbQueryEvent> watchQuery(String sessionId, String queryId) => Stream.fromIterable(const [
    DbQueryEvent(
      kind: DbQueryEventKind.started,
      columns: [DbColumn(name: 'answer', typeName: 'int')],
    ),
    DbQueryEvent(
      kind: DbQueryEventKind.rows,
      rows: [
        [DbCell(DbCellKind.intValue, '42')],
      ],
    ),
    DbQueryEvent(kind: DbQueryEventKind.done),
  ]);
  @override
  Future<void> cancelQuery(String sessionId, String queryId) async => cancelled.add(queryId);
  @override
  Future<void> releaseQuery(String sessionId, String queryId) async {}
  @override
  Future<DbSessionInfo> allowSessionWrite(String sessionId) async {
    readOnly = false;
    return _session(production: production, readOnly: false);
  }

  @override
  Future<List<DbAiProvider>> aiProviders() async => const [
    DbAiProvider(id: 'provider', name: 'Local fixture', external: false),
  ];
  @override
  Future<DbAiReview> prepareAi(String sessionId, String providerId, DbAiRequest request) async => DbAiReview(
    ticket: 'ai-review',
    provider: const DbAiProvider(id: 'provider', name: 'Local fixture', external: false),
    connectionName: 'fixture-db',
    expiresAt: DateTime.now().add(const Duration(minutes: 1)),
    schemaObjects: ['items'],
  );
  @override
  Stream<DbAiEvent> askAi(String sessionId, String ticket, DbAiConsent consent) {
    consents.add(consent);
    return Stream.fromIterable([
      DbAiEvent(kind: DbAiEventKind.draft, content: SecretText('SELECT 42')),
      const DbAiEvent(kind: DbAiEventKind.done),
    ]);
  }

  @override
  Future<void> cancelAi(String sessionId, String ticket) async => aiCancelled.add(ticket);
  @override
  Future<void> revokeAiConsent(String sessionId) async {}
  @override
  dynamic noSuchMethod(Invocation invocation) => throw UnimplementedError();
}

Future<ProviderContainer> _pump(
  WidgetTester tester,
  _Service service, {
  String language = 'en',
  Size size = const Size(1200, 900),
  double scale = 1,
}) async {
  final backend = testBackend();
  addTearDown(backend.dispose);
  tester.view.physicalSize = size;
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
        locale: Locale(language),
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        supportedLocales: AppLocalizations.supportedLocales,
        builder: (context, child) => MediaQuery(
          data: MediaQuery.of(context).copyWith(textScaler: TextScaler.linear(scale)),
          child: child!,
        ),
        home: Scaffold(
          body: DbWorkspace(connection: _connection, onClose: () {}),
        ),
      ),
    ),
  );
  await settle(tester);
  return ProviderScope.containerOf(tester.element(find.byType(DbWorkspace)));
}

Future<void> _type(WidgetTester tester, String text) async {
  await tester.enterText(find.byKey(const ValueKey('db-query-editor')), text);
  await tester.pump();
}

void main() {
  testWidgets('current statement runs through native parser/review and keeps typed results', (tester) async {
    final service = _Service();
    await _pump(tester, service);
    await _type(tester, "SELECT ';' AS value; -- cursor boundary owned by native");
    await tester.tap(find.byKey(const ValueKey('db-run')));
    await settle(tester);
    expect(service.preparations.single.$2, DbRunMode.current);
    expect(service.executed.single.approvalTicket, 'review');
    expect(find.text('42'), findsOneWidget);
    expect(service.discarded, ['review']);
  });
  testWidgets('run button and shortcut take the selection; Alt+X and the script button run everything', (tester) async {
    final service = _Service();
    // Releasing the previous query's stream finishes a few frames later.
    Future<void> finish() async {
      for (var i = 0; i < 5; i++) {
        await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 10)));
        await tester.pump(const Duration(milliseconds: 20));
      }
      await settle(tester);
    }

    await _pump(tester, service);
    await _type(tester, 'SELECT 1;\nSELECT 2;');
    final field = tester.widget<TextField>(find.byKey(const ValueKey('db-query-editor')));
    field.controller!.selection = const TextSelection(baseOffset: 10, extentOffset: 19);
    await tester.tap(find.byKey(const ValueKey('db-run')));
    await finish();
    expect(service.preparations.last.$2, DbRunMode.selection);
    expect((service.preparations.last.$3, service.preparations.last.$4), (10, 19));

    field.controller!.selection = const TextSelection.collapsed(offset: 3);
    await tester.tap(find.byKey(const ValueKey('db-query-editor')));
    await tester.pump();
    field.controller!.selection = const TextSelection.collapsed(offset: 3);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.alt);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyX);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.alt);
    await finish();
    expect(service.preparations.last.$2, DbRunMode.script);

    await tester.tap(find.byKey(const ValueKey('db-run-script')));
    await finish();
    expect(service.preparations.last.$2, DbRunMode.script);
    expect(service.executed, hasLength(3));
  });

  testWidgets('structure groups mixed children into folders and frames an expanded table', (tester) async {
    final service = _Service()..mixedTree = true;
    await _pump(tester, service);
    expect(find.text('Tables'), findsOneWidget);
    expect(find.text('Views'), findsOneWidget);
    expect(find.text('orders_view'), findsNothing, reason: 'only the tables folder starts open');
    await tester.tap(find.text('Views'));
    await settle(tester);
    expect(find.text('orders_view'), findsOneWidget);
    await tester.tap(find.text('items'));
    await settle(tester);
    expect(find.text('Columns'), findsOneWidget);
    expect(find.text('Indexes'), findsOneWidget);
    expect(find.text('id'), findsOneWidget);
    expect(find.text('items_pkey'), findsNothing);
    await tester.tap(find.byKey(const ValueKey('db-tree-collapse')));
    await settle(tester);
    expect(find.text('id'), findsNothing);
    expect(find.text('orders_view'), findsNothing);
  });

  testWidgets('production blocks write review even if a backend mistakenly offers a ticket', (tester) async {
    final service = _Service()..risk = DbQueryRisk.write;
    await _pump(tester, service);
    await _type(tester, 'DELETE FROM items');
    await tester.tap(find.byKey(const ValueKey('db-run')));
    await settle(tester);
    expect(service.executed, isEmpty);
    expect(service.discarded, ['review']);
    expect(find.byKey(const ValueKey('db-query-confirmation')), findsNothing);
  });
  testWidgets('non-production changing query requires exact connection name', (tester) async {
    final service = _Service(production: false, readOnly: false)..risk = DbQueryRisk.dangerous;
    await _pump(tester, service);
    await _type(tester, 'DROP TABLE items');
    await tester.tap(find.byKey(const ValueKey('db-run')));
    await settle(tester);
    expect(service.executed, isEmpty);
    await tester.enterText(find.byKey(const ValueKey('db-query-confirmation')), 'wrong');
    await tester.pump();
    expect(service.executed, isEmpty);
    await tester.enterText(find.byKey(const ValueKey('db-query-confirmation')), 'fixture-db');
    await tester.pump();
    await tester.tap(find.text('Run').last);
    await settle(tester);
    expect(service.executed, hasLength(1));
  });
  testWidgets('late review and metadata after lock are discarded without executing', (tester) async {
    final service = _Service()
      ..pendingReview = Completer<DbQueryReview>()
      ..pendingTree = Completer<DbTreePage>();
    final container = await _pump(tester, service);
    await _type(tester, 'SELECT secret_value');
    await tester.tap(find.byKey(const ValueKey('db-run')));
    await tester.pump();
    container.read(_scope.notifier).lock();
    await tester.pump();
    service.pendingReview!.complete(
      DbQueryReview(ticket: 'late', risk: DbQueryRisk.read, expiresAt: DateTime.now().add(const Duration(minutes: 1))),
    );
    service.pendingTree!.complete(
      const DbTreePage(
        nodes: [
          DbTreeNode(
            reference: DbObjectRef(database: 'secret-db', kind: DbObjectKind.table, name: 'late-secret-table'),
            hasChildren: false,
          ),
        ],
      ),
    );
    await settle(tester);
    expect(service.executed, isEmpty);
    expect(service.discarded, ['late']);
    expect(service.disconnected, ['session']);
    expect(find.text('late-secret-table'), findsNothing);
  });
  testWidgets('line comment shortcut round-trips without injecting replacement markers', (tester) async {
    await _pump(tester, _Service());
    await _type(tester, '  SELECT 1;\nSELECT 2');
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.slash);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    await tester.pump();
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.slash);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    await tester.pump();
    final field = tester.widget<TextField>(find.byKey(const ValueKey('db-query-editor')));
    expect(field.controller!.text, '  SELECT 1;\nSELECT 2');
  });
  testWidgets('AI consent defaults deny schema and rows; accepting draft never executes', (tester) async {
    final service = _Service();
    await _pump(tester, service);
    await tester.tap(find.byTooltip('AI assistant').first);
    await settle(tester);
    await tester.enterText(find.byKey(const ValueKey('db-ai-question')), 'Count rows');
    await tester.tap(find.text('AI assistant').last);
    await settle(tester);
    for (final key in ['schema', 'rows', 'sample', 'run']) {
      expect(tester.widget<CheckboxListTile>(find.byKey(ValueKey('db-ai-consent-$key'))).value, isFalse);
    }
    await tester.tap(find.byKey(const ValueKey('db-ai-consent-submit')));
    await settle(tester);
    expect(service.consents.single.schema, isFalse);
    expect(service.consents.single.runReadonly, isFalse);
    await tester.ensureVisible(find.byKey(const ValueKey('db-ai-accept')));
    await tester.tap(find.byKey(const ValueKey('db-ai-accept')));
    await settle(tester);
    expect(service.executed, isEmpty);
    expect(tester.widget<TextField>(find.byKey(const ValueKey('db-query-editor'))).controller!.text, 'SELECT 42');
  });
  for (final language in ['en', 'ru']) {
    testWidgets('phone workspace query/results at 150% remains usable in $language', (tester) async {
      final service = _Service();
      await _pump(tester, service, language: language, size: const Size(412, 915), scale: 1.5);
      await _type(tester, 'SELECT 42');
      await tester.tap(find.byKey(const ValueKey('db-run')));
      await settle(tester);
      expect(service.executed, hasLength(1));
      expect(find.text('42'), findsOneWidget);
      expect(tester.takeException(), isNull);
    });
  }
}
