import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/mock/mock_cloud.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/db/db_ai_service.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_object_details.dart';
import 'package:consolecrypt/db/db_providers.dart';
import 'package:consolecrypt/db/db_query_service.dart';
import 'package:consolecrypt/db/db_service.dart';
import 'package:consolecrypt/db/db_workspace.dart';
import 'package:consolecrypt/snippets/starter_catalog.dart';
import 'package:consolecrypt/vault/user_presence.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/fake_presence.dart';
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
  transportSecurity: DbTransportSecurity.unverified,
);

class _Service implements DbService, DbQueryService, DbQueryReviewService, DbAiService, DbObjectInspectService {
  _Service({this.production = true, this.readOnly = true});
  final bool production;
  bool readOnly;
  final executed = <DbQueryRequest>[], discarded = <String>[], disconnected = <String>[], cancelled = <String>[];
  final preparations = <(String, DbRunMode, int, int)>[];
  final treeParents = <DbObjectRef?>[];
  final consents = <DbAiConsent>[];
  final aiCancelled = <String>[];
  DbQueryRisk risk = DbQueryRisk.read;
  String? suggestion;
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
      DbQueryReview(
        ticket: 'review',
        risk: risk,
        expiresAt: DateTime.now().add(const Duration(minutes: 2)),
        suggestion: suggestion,
      );
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
  Future<DbJson> inspectObject(String sessionId, DbObjectRef object, {int? sampleDocuments}) async {
    if (object.name != 'items') throw const DbWrongPassphrase();
    return {
      'sections': [
        {
          'id': 'general',
          'fields': [
            {'id': 'kind', 'value': 'table', 'kind': 'keyword'},
            {'id': 'owner', 'value': 'app', 'kind': 'code'},
          ],
        },
        {
          'id': 'size',
          'fields': [
            {'id': 'rows', 'value': '1234', 'kind': 'estimate'},
            {'id': 'total_size', 'value': '81920', 'kind': 'bytes'},
          ],
        },
      ],
    };
  }

  @override
  Future<String> objectQuery(String sessionId, DbObjectRef object, {int limit = 100}) async =>
      'SELECT * FROM "public"."${object.name}" LIMIT $limit;';
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

/// A production backend with the temporary write window and cell edits.
class _ProdService extends _Service implements DbProductionWriteService, DbCellEditService {
  _ProdService() : super(production: true, readOnly: true);
  DateTime? until;
  final unlocks = <int>[], locks = <String>[], commits = <String>[];
  final proofs = <StepUpProof>[];
  List<DbCellEdit> reviewed = const [];
  bool roleCanWrite = false;

  DbSessionInfo _live() => DbSessionInfo(
    id: 'session',
    connectionId: 'connection',
    engine: DbEngine.postgresql,
    serverVersion: '17.0',
    production: true,
    readOnly: until == null,
    serverReadOnly: DbServerReadOnly.enforced,
    transportSecurity: DbTransportSecurity.unverified,
    canUnlockProduction: true,
    productionWriteUntil: until,
  );
  @override
  Future<DbSessionInfo> connect(String connectionId, {SecretText? temporaryPassword}) async => _live();
  @override
  Future<DbSessionInfo> sessionState(String sessionId) async => _live();
  @override
  Future<DbSessionInfo> unlockProduction(String sessionId, {required StepUpProof proof, required int minutes}) async {
    proofs.add(proof);
    final ok = switch (proof) {
      PassphraseProof(:final passphrase) => passphrase.expose() == 'vault-pass',
      PresenceProof(:final token) => token == 'presence:db_production_write:$sessionId',
    };
    if (!ok) throw const DbWrongPassphrase();
    unlocks.add(minutes);
    until = DateTime.now().add(Duration(minutes: minutes));
    return _live();
  }

  @override
  Future<DbSessionInfo> lockProduction(String sessionId) async {
    locks.add(sessionId);
    until = null;
    return _live();
  }

  @override
  Future<DbPrivilegeReport> checkPrivileges(String sessionId) async =>
      DbPrivilegeReport(serverReadOnly: DbServerReadOnly.enforced, hasWritePrivileges: roleCanWrite);

  @override
  Stream<DbQueryEvent> watchQuery(String sessionId, String queryId) => Stream.fromIterable(const [
    DbQueryEvent(
      kind: DbQueryEventKind.started,
      columns: [
        DbColumn(name: 'id', typeName: 'int4', nullable: false, primaryKey: true),
        DbColumn(name: 'name', typeName: 'text'),
      ],
      editableTable: DbObjectRef(database: '', schema: 'public', kind: DbObjectKind.table, name: 'items'),
    ),
    DbQueryEvent(
      kind: DbQueryEventKind.rows,
      rows: [
        [DbCell(DbCellKind.intValue, '1'), DbCell(DbCellKind.text, 'alpha')],
        [DbCell(DbCellKind.intValue, '2'), DbCell(DbCellKind.text, 'beta')],
      ],
    ),
    DbQueryEvent(kind: DbQueryEventKind.done),
  ]);

  @override
  Future<DbCellEditReview> reviewCellEdits(String sessionId, DbObjectRef table, List<DbCellEdit> edits) async {
    reviewed = edits;
    // The core writes the values out as literals for the review.
    final edit = edits.single;
    return DbCellEditReview(
      ticket: 'edits',
      sql: SecretText(
        'UPDATE "public"."items" SET "${edit.column}" = \'${edit.value.text}\' '
        'WHERE "id" = ${edit.originalPrimaryKey['id']!.text};',
      ),
      expiresAt: DateTime.now().add(const Duration(minutes: 5)),
      rows: edits.length,
    );
  }

  @override
  Future<void> commitCellEdits(String sessionId, String ticket) async => commits.add(ticket);
  @override
  Future<void> discardCellEdits(String sessionId, String ticket) async {}
}

Future<ProviderContainer> _pump(
  WidgetTester tester,
  _Service service, {
  String language = 'en',
  Size size = const Size(1200, 900),
  double scale = 1,
  List<Snippet> snippets = const [],
  UserPresence? presence,
}) async {
  final backend = testBackend();
  addTearDown(backend.dispose);
  // Changing statements are confirmed with the vault passphrase.
  await backend.debugSignInDemoAndUnlock();
  for (final snippet in snippets) {
    await backend.snippets.saveSnippet(snippet);
  }
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        appServicesProvider.overrideWithValue(backend.services),
        dbServiceProvider.overrideWithValue(service),
        dbScopeProvider.overrideWith((ref) => ref.watch(_scope)),
        if (presence != null) userPresenceProvider.overrideWithValue(presence),
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
  testWidgets('the header shows what protects the session and the card remembers it', (tester) async {
    final container = await _pump(tester, _Service());
    final l = tester.element(find.byType(DbWorkspace)).l10n;
    expect(find.byKey(const ValueKey('db-transport-badge')), findsOneWidget);
    expect(find.text(l.dbTransportUnverified), findsOneWidget);
    expect(container.read(dbLastSecurityProvider), {'connection': DbTransportSecurity.unverified});
  });
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

  testWidgets('the snippets button and Ctrl+J put a snippet below the text without running it', (tester) async {
    final en = lookupAppLocalizations(const Locale('en'));
    final activity = starterSnippetPackages(en)
        .firstWhere((p) => p.id == 'postgresql')
        .snippets
        .firstWhere((s) => s.catalogId == 'consolecrypt.db.pg.activity.v1');
    final service = _Service();
    await _pump(tester, service, snippets: [activity]);
    final controller = tester.widget<TextField>(find.byKey(const ValueKey('db-query-editor'))).controller!;
    Finder row() =>
        find.descendant(of: find.byKey(const ValueKey('db-snippet-picker')), matching: find.text(activity.name));
    await _type(tester, 'SELECT 1');
    expect(find.byTooltip('Snippets: insert into the editor (Ctrl+J)'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('db-snippets')));
    await settle(tester);
    expect(find.byKey(const ValueKey('db-snippet-picker')), findsOneWidget);
    await tester.tap(row());
    await settle(tester);
    expect(find.byKey(const ValueKey('db-snippet-picker')), findsNothing);
    final query = activity.template.trim();
    expect(controller.text, 'SELECT 1;\n\n$query');
    expect(controller.selection.baseOffset, 'SELECT 1;\n\n'.length, reason: 'the caret is at the inserted text');

    // The shortcut opens the same picker; cancelling leaves the text alone.
    await tester.tap(find.byKey(const ValueKey('db-query-editor')));
    await tester.pump();
    await tester.sendKeyDownEvent(LogicalKeyboardKey.control);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyJ);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.control);
    await settle(tester);
    expect(find.byKey(const ValueKey('db-snippet-picker')), findsOneWidget);
    await tester.tap(find.text('Cancel').last);
    await settle(tester);
    expect(controller.text, 'SELECT 1;\n\n$query');
    expect(service.preparations, isEmpty, reason: 'an inserted snippet never runs');
    expect(service.executed, isEmpty);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('a blocked query offers its safe alternative for the editor, never runs it', (tester) async {
    final service = _Service()
      ..risk = DbQueryRisk.blocked
      ..suggestion = 'SCAN 0 MATCH * COUNT 100';
    await _pump(tester, service);
    await _type(tester, 'KEYS *');
    await tester.tap(find.byKey(const ValueKey('db-run')));
    await settle(tester);
    expect(find.text('Insert into the editor'), findsOneWidget);
    await tester.tap(find.text('Insert into the editor'));
    await settle(tester);
    final controller = tester.widget<TextField>(find.byKey(const ValueKey('db-query-editor'))).controller!;
    expect(controller.text, 'KEYS *;\n\nSCAN 0 MATCH * COUNT 100', reason: 'appended to the current tab');
    expect(find.byKey(const ValueKey('db-query-tabs')), findsOneWidget);
    expect(service.executed, isEmpty);
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

  group('data preview tab', () {
    Finder tabs() => find.byWidgetPredicate(
      (w) => w.key is ValueKey<String> && (w.key! as ValueKey<String>).value.startsWith('db-query-tab-'),
    );
    String editorText(WidgetTester tester) =>
        tester.widget<TextField>(find.byKey(const ValueKey('db-query-editor'))).controller!.text;
    Future<void> doubleClick(WidgetTester tester, DbObjectRef object) async {
      final row = find.byKey(ValueKey('db-object-${object.id}'));
      await tester.tap(row);
      await tester.pump(const Duration(milliseconds: 60));
      await tester.tap(row);
      // Releasing the previous query's stream finishes a few frames later.
      for (var i = 0; i < 5; i++) {
        await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 10)));
        await tester.pump(const Duration(milliseconds: 20));
      }
      await settle(tester);
    }

    const items = DbObjectRef(database: 'fixture', kind: DbObjectKind.table, name: 'items');
    const view = DbObjectRef(database: 'fixture', kind: DbObjectKind.view, name: 'orders_view');

    testWidgets('double clicks reuse one tab and show the rows at once', (tester) async {
      final service = _Service()..mixedTree = true;
      await _pump(tester, service);
      await tester.tap(find.text('Views'));
      await settle(tester);
      expect(tabs(), findsOneWidget);

      await doubleClick(tester, items);
      expect(tabs(), findsNWidgets(2));
      expect(find.text('items — data'), findsOneWidget);
      expect(editorText(tester), 'SELECT * FROM "public"."items" LIMIT 100;');
      expect(service.executed.single.text, 'SELECT * FROM "public"."items" LIMIT 100;');
      expect(find.text('42'), findsOneWidget, reason: 'the rows are shown without pressing Run');

      await doubleClick(tester, view);
      expect(tabs(), findsNWidgets(2), reason: 'the preview tab is replaced, not added');
      expect(find.text('items — data'), findsNothing);
      expect(find.text('orders_view — data'), findsOneWidget);
      expect(service.executed.map((r) => r.text), [
        'SELECT * FROM "public"."items" LIMIT 100;',
        'SELECT * FROM "public"."orders_view" LIMIT 100;',
      ]);

      // Editing the preview turns it into an ordinary tab; the next preview
      // gets a tab of its own and the typed text stays.
      await _type(tester, 'SELECT count(*) FROM orders_view');
      expect(find.text('orders_view — data'), findsNothing);
      await doubleClick(tester, items);
      expect(tabs(), findsNWidgets(3));
      expect(find.text('items — data'), findsOneWidget);
      await tester.tap(find.byKey(const ValueKey('db-query-tab-2')));
      await settle(tester);
      expect(editorText(tester), 'SELECT count(*) FROM orders_view');
    });

    testWidgets('generated SQL goes below the current text without running', (tester) async {
      final service = _Service()..mixedTree = true;
      await _pump(tester, service);
      await _type(tester, 'SELECT 1');
      // Showing a table first: the draft returns to the editor tab.
      await doubleClick(tester, items);
      expect(service.executed, hasLength(1));
      await tester.tap(find.byTooltip('AI assistant').first);
      await settle(tester);
      await tester.enterText(find.byKey(const ValueKey('db-ai-question')), 'Count rows');
      await tester.tap(find.text('AI assistant').last);
      await settle(tester);
      await tester.tap(find.byKey(const ValueKey('db-ai-consent-submit')));
      await settle(tester);
      await tester.ensureVisible(find.byKey(const ValueKey('db-ai-accept')));
      await tester.tap(find.byKey(const ValueKey('db-ai-accept')));
      await settle(tester);
      expect(tabs(), findsNWidgets(2), reason: 'no new tab for generated text');
      expect(editorText(tester), 'SELECT 1;\n\nSELECT 42');
      final selection = tester.widget<TextField>(find.byKey(const ValueKey('db-query-editor'))).controller!.selection;
      expect(selection.baseOffset, 'SELECT 1;\n\n'.length, reason: 'the caret is at the inserted text');
      expect(service.executed, hasLength(1), reason: 'generated text never runs');
    });
  });

  group('object details', () {
    Future<void> menu(WidgetTester tester, String name) async {
      await tester.tap(find.text(name), buttons: kSecondaryButton);
      await settle(tester);
      await tester.tap(find.text('Object details'));
      await settle(tester);
    }

    testWidgets('show grouped, formatted facts with a copy button', (tester) async {
      final service = _Service()..mixedTree = true;
      await _pump(tester, service);
      await menu(tester, 'items');
      expect(find.byKey(const ValueKey('db-details-dialog')), findsOneWidget);
      expect(find.text('General'), findsOneWidget);
      expect(find.text('Size'), findsOneWidget);
      expect(find.text('Table'), findsOneWidget, reason: 'the relation kind is translated');
      expect(find.text('≈ 1,234'), findsOneWidget, reason: 'estimates are marked');
      expect(find.text('80.0 KiB'), findsOneWidget, reason: 'sizes are human-readable');
      expect(find.byKey(const ValueKey('db-details-copy')), findsOneWidget);
      expect(find.textContaining('"sections"'), findsNothing, reason: 'no raw JSON');
    });

    testWidgets('a failure is a glass toast, never a Material snackbar', (tester) async {
      final service = _Service()..mixedTree = true;
      await _pump(tester, service);
      await tester.tap(find.text('Views'));
      await settle(tester);
      await menu(tester, 'orders_view');
      expect(find.byKey(const ValueKey('db-details-dialog')), findsNothing);
      expect(find.byType(SnackBar), findsNothing);
      expect(find.textContaining('The database operation failed'), findsOneWidget, reason: 'shown by the glass toast');
      expect(find.byWidgetPredicate((w) => '${w.runtimeType}' == '_GlassToastView'), findsOneWidget);
    });

    testWidgets('a raw MongoDB description is shown as an expandable JSON block', (tester) async {
      await _pump(tester, _Service());
      unawaited(
        showAppDialog<void>(
          tester.element(find.byType(DbWorkspace)),
          secure: true,
          builder: (_) => DbObjectDetailsDialog(
            title: 'orders',
            document: SecretText('{"database": "shop", "indexes": [{"name": "_id_"}]}'),
          ),
        ),
      );
      await settle(tester);
      final json = find.byKey(const ValueKey('db-details-json'));
      expect(json, findsOneWidget);
      expect(find.descendant(of: json, matching: find.textContaining('"database": "shop"')), findsOneWidget);
      await tester.tap(find.byKey(const ValueKey('db-details-json-toggle')));
      await settle(tester);
      expect(json, findsNothing);
    });
  });

  testWidgets('production blocks write review even if a backend mistakenly offers a ticket', (tester) async {
    final service = _Service()..risk = DbQueryRisk.write;
    await _pump(tester, service);
    await _type(tester, 'DELETE FROM items');
    await tester.tap(find.byKey(const ValueKey('db-run')));
    await settle(tester);
    expect(service.executed, isEmpty);
    expect(service.discarded, ['review']);
    expect(find.byKey(const ValueKey('db-query-review')), findsNothing);
  });
  testWidgets('non-production changing query requires the vault passphrase, not a typed name', (tester) async {
    final service = _Service(production: false, readOnly: false)..risk = DbQueryRisk.dangerous;
    await _pump(tester, service);
    await _type(tester, 'DROP TABLE items');
    await tester.tap(find.byKey(const ValueKey('db-run')));
    await settle(tester);
    expect(service.executed, isEmpty);
    expect(find.byKey(const ValueKey('db-query-review')), findsOneWidget);
    expect(
      find.descendant(of: find.byKey(const ValueKey('db-query-review-sql')), matching: find.text('DROP TABLE items')),
      findsOneWidget,
      reason: 'the query is shown as a code block',
    );
    final field = find.byKey(const ValueKey('vault-passphrase-confirm'));
    final run = find.byKey(const ValueKey('vault-passphrase-confirm-action'));
    expect(tester.widget<GlassButton>(run).onPressed, isNull);
    // The connection name is no longer a confirmation.
    await tester.enterText(field, 'fixture-db');
    await tester.pump();
    await tester.tap(run);
    await settle(tester);
    expect(service.executed, isEmpty);
    expect(find.text('Wrong vault passphrase.'), findsOneWidget);
    expect(tester.widget<TextField>(find.descendant(of: field, matching: find.byType(TextField))).controller!.text, '');
    // The right passphrase approves with Enter.
    await tester.enterText(field, MockCloud.demoPassphrase);
    await tester.testTextInput.receiveAction(TextInputAction.done);
    await settle(tester);
    expect(find.byKey(const ValueKey('db-query-review')), findsNothing);
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
  testWidgets('production unlock needs only the passphrase, then writes go through review', (tester) async {
    final service = _ProdService()..risk = DbQueryRisk.write;
    await _pump(tester, service);
    // Locked: changing statements are blocked before any review dialog.
    await _type(tester, 'UPDATE items SET name = 1');
    await tester.tap(find.byKey(const ValueKey('db-run')));
    await settle(tester);
    expect(service.executed, isEmpty);
    expect(find.byKey(const ValueKey('db-query-review')), findsNothing);

    await tester.tap(find.byKey(const ValueKey('db-prod-unlock')));
    await settle(tester);
    expect(find.byKey(const ValueKey('db-prod-unlock-dialog')), findsOneWidget);
    expect(find.byKey(const ValueKey('db-prod-name')), findsNothing, reason: 'no typed connection name');
    final submit = find.byKey(const ValueKey('db-prod-unlock-submit'));
    await tester.enterText(find.byKey(const ValueKey('db-prod-passphrase')).first, 'wrong-pass');
    await tester.pump();
    await tester.tap(submit);
    await settle(tester);
    expect(find.text('Wrong vault passphrase.'), findsOneWidget);
    expect(service.unlocks, isEmpty);
    await tester.enterText(find.byKey(const ValueKey('db-prod-passphrase')).first, 'vault-pass');
    await tester.tap(find.text('5 minutes'));
    await tester.pump();
    await tester.tap(submit);
    await settle(tester);
    expect(service.unlocks, [5]);
    expect(find.byKey(const ValueKey('db-prod-unlock-dialog')), findsNothing);
    expect(find.byKey(const ValueKey('db-prod-write-badge')), findsOneWidget);
    expect(find.textContaining('Production writes until'), findsWidgets);
    expect(find.byKey(const ValueKey('db-prod-write-countdown')), findsOneWidget);
    // The role cannot write: the privilege check says so.
    expect(find.byKey(const ValueKey('db-prod-role-read-only')), findsOneWidget);

    // A change still needs the review with the vault passphrase.
    await tester.tap(find.byKey(const ValueKey('db-run')));
    await settle(tester);
    expect(find.byKey(const ValueKey('db-query-review')), findsOneWidget);
    await tester.enterText(find.byKey(const ValueKey('vault-passphrase-confirm')), MockCloud.demoPassphrase);
    await tester.pump();
    await tester.tap(find.byKey(const ValueKey('vault-passphrase-confirm-action')));
    await settle(tester);
    expect(service.executed, hasLength(1));

    await tester.tap(find.byKey(const ValueKey('db-prod-lock')));
    await settle(tester);
    expect(service.locks, ['session']);
    expect(find.byKey(const ValueKey('db-prod-write-badge')), findsNothing);
    expect(find.byKey(const ValueKey('db-prod-unlock')), findsOneWidget);
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));

  testWidgets('production unlock with Touch ID sends a token bound to the session, never a passphrase', (tester) async {
    final service = _ProdService();
    final presence = FakePresence();
    await _pump(tester, service, presence: presence);
    await tester.tap(find.byKey(const ValueKey('db-prod-unlock')));
    await settle(tester);
    expect(find.byKey(const ValueKey('step-up-presence')), findsOneWidget);
    expect(find.text('You will confirm with Touch ID.'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('db-prod-unlock-submit')));
    await settle(tester);
    final (action, object, reason) = presence.verified.single;
    expect((action, object), (StepUpAction.dbProductionWrite, 'session'));
    expect(reason, 'allow changes on fixture-db');
    expect((service.proofs.single as PresenceProof).token, 'presence:db_production_write:session');
    expect(service.unlocks, [15]);
    expect(find.byKey(const ValueKey('db-prod-unlock-dialog')), findsNothing);

    // A refused token (or failed biometry) leaves the passphrase.
    await tester.tap(find.byKey(const ValueKey('db-prod-lock')));
    await settle(tester);
    presence.failure = AppErrorCode.osAuthFailed;
    await tester.tap(find.byKey(const ValueKey('db-prod-unlock')));
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('db-prod-unlock-submit')));
    await settle(tester);
    expect(find.byKey(const ValueKey('step-up-presence-failed')), findsOneWidget);
    expect(service.proofs, hasLength(1), reason: 'nothing sent after a failed prompt');
    await tester.enterText(find.byKey(const ValueKey('db-prod-passphrase')).first, 'vault-pass');
    await tester.pump();
    await tester.tap(find.byKey(const ValueKey('db-prod-unlock-submit')));
    await settle(tester);
    expect(service.proofs.last, isA<PassphraseProof>());
    expect(service.unlocks, [15, 15]);
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));

  testWidgets('cells of a production result are edited in place only while unlocked', (tester) async {
    final service = _ProdService()..roleCanWrite = true;
    await _pump(tester, service);
    await _type(tester, 'SELECT * FROM items');
    await tester.tap(find.byKey(const ValueKey('db-run')));
    await settle(tester);
    final cell = find.byKey(const ValueKey('db-cell-1-1'));
    Future<void> doubleClick() async {
      await tester.tap(cell);
      await tester.pump(const Duration(milliseconds: 60));
      await tester.tap(cell);
      await settle(tester, steps: 4);
    }

    await doubleClick();
    expect(find.byKey(const ValueKey('db-cell-editor')), findsNothing, reason: 'locked production opens the viewer');
    Navigator.of(tester.element(find.byType(DbWorkspace))).pop();
    await settle(tester);

    await tester.tap(find.byKey(const ValueKey('db-prod-unlock')));
    await settle(tester);
    await tester.enterText(find.byKey(const ValueKey('db-prod-passphrase')).first, 'vault-pass');
    await tester.pump();
    await tester.tap(find.byKey(const ValueKey('db-prod-unlock-submit')));
    await settle(tester);
    expect(find.byKey(const ValueKey('db-prod-role-read-only')), findsNothing);

    // Row numbers select; the status line counts the selection.
    await tester.tap(find.byKey(const ValueKey('db-row-number-0')));
    await tester.pump(const Duration(milliseconds: 400));
    expect(find.textContaining('Selected: 1 row'), findsOneWidget);

    await doubleClick();
    final editor = find.byKey(const ValueKey('db-cell-editor'));
    expect(editor, findsOneWidget);
    await tester.enterText(editor, 'bravo');
    await tester.testTextInput.receiveAction(TextInputAction.done);
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('db-apply-edits')));
    await settle(tester);
    expect(service.reviewed.single.column, 'name');
    expect(service.reviewed.single.value.text, 'bravo');
    expect(service.reviewed.single.originalPrimaryKey['id']!.text, '2');
    expect(service.commits, isEmpty);
    // The review shows the values, not placeholders.
    final sql = find.byKey(const ValueKey('db-query-review-sql'));
    for (final part in ['"name" = \'bravo\'', '"id" = 2;']) {
      expect(
        find.descendant(of: sql, matching: find.textContaining(part)),
        findsOneWidget,
        reason: part,
      );
    }
    expect(find.descendant(of: sql, matching: find.textContaining('{{p')), findsNothing);
    await tester.enterText(find.byKey(const ValueKey('vault-passphrase-confirm')), 'wrong');
    await tester.pump();
    await tester.tap(find.byKey(const ValueKey('vault-passphrase-confirm-action')));
    await settle(tester);
    expect(service.commits, isEmpty);
    await tester.enterText(find.byKey(const ValueKey('vault-passphrase-confirm')), MockCloud.demoPassphrase);
    await tester.pump();
    await tester.tap(find.byKey(const ValueKey('vault-passphrase-confirm-action')));
    await settle(tester);
    expect(service.commits, ['edits']);
    expect(find.byKey(const ValueKey('db-apply-edits')), findsNothing);
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));

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
