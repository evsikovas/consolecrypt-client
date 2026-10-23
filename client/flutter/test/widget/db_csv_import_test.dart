import 'dart:async';
import 'dart:io';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/mock/mock_cloud.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/file_dialog_service.dart';
import 'package:consolecrypt/db/db_csv_import.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_providers.dart';
import 'package:consolecrypt/db/db_query_service.dart';
import 'package:consolecrypt/db/db_result_edits.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

const _table = DbObjectRef(database: '', schema: 'public', kind: DbObjectKind.table, name: 'items');
const _session = DbSessionInfo(
  id: 'session',
  connectionId: 'connection',
  engine: DbEngine.postgresql,
  serverVersion: '17.0',
  production: false,
  readOnly: false,
  serverReadOnly: DbServerReadOnly.unverified,
);

class _Files implements FileDialogService {
  _Files(this.path);
  final String path;
  @override
  Future<String?> chooseOpenFile({List<String> extensions = const []}) async => path;
  @override
  dynamic noSuchMethod(Invocation invocation) => throw UnimplementedError();
}

class _Service implements DbCsvImportService {
  final previews = <DbCsvOptions>[];
  final checks = <List<DbCsvMapping>>[];
  final discarded = <String>[], started = <String>[];
  DbCsvPlan? reviewed;
  // ignore: close_sinks (closed by the test tear-down)
  final progress = StreamController<DbCsvProgress>();

  @override
  Future<DbCsvPreview> csvPreview(String path, DbCsvOptions options) async {
    previews.add(options);
    return DbCsvPreview(
      delimiter: options.delimiter ?? ',',
      encoding: DbCsvEncoding.utf8,
      columns: const ['ID', 'Name', 'amount'],
      rows: const [
        ['1', 'alpha', '10'],
        ['2', null, 'abc'],
      ],
    );
  }

  @override
  Future<List<DbEditColumn>> editColumns(String sessionId, DbObjectRef table) async => const [
    DbEditColumn(name: 'id', typeName: 'integer', nullable: false, key: true),
    DbEditColumn(name: 'name', typeName: 'text', nullable: false),
    DbEditColumn(name: 'amount', typeName: 'integer'),
    DbEditColumn(name: 'total', typeName: 'integer', generated: true),
  ];

  @override
  Future<List<DbCsvCheck>> csvCheck(
    String sessionId,
    DbObjectRef table,
    String path,
    DbCsvOptions options,
    List<DbCsvMapping> mapping,
  ) async {
    checks.add(mapping);
    return [
      for (final m in mapping)
        DbCsvCheck(
          target: m.target,
          kind: 'int',
          bad: m.target == 'amount' ? 1 : 0,
          samples: m.target == 'amount' ? const ['abc'] : const [],
        ),
    ];
  }

  @override
  Future<DbCsvReview> reviewCsvImport(String sessionId, String path, DbCsvPlan plan) async {
    reviewed = plan;
    return DbCsvReview(
      ticket: 'csv',
      sql: SecretText(
        '${plan.truncate ? 'DELETE FROM "public"."items";\n' : ''}'
        'INSERT INTO "public"."items" ("id", "name") VALUES ({p0}, {p1}) ON CONFLICT ("id") DO UPDATE SET "name" = EXCLUDED."name";',
      ),
      risk: plan.truncate ? DbQueryRisk.dangerous : DbQueryRisk.write,
      expiresAt: DateTime.now().add(const Duration(minutes: 5)),
    );
  }

  @override
  Future<void> discardCsvImport(String ticket) async => discarded.add(ticket);
  @override
  Future<String> startCsvImport(String sessionId, String ticket) async {
    started.add(ticket);
    return 'import';
  }

  @override
  Stream<DbCsvProgress> watchCsvImport(String importId) => progress.stream;
  @override
  Future<void> cancelCsvImport(String importId) async {}
  @override
  dynamic noSuchMethod(Invocation invocation) => throw UnimplementedError();
}

Future<bool?> Function() _pump(WidgetTester tester, _Service service, String path) {
  bool? closed;
  return () async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    tester.view.physicalSize = const Size(1400, 1000);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          appServicesProvider.overrideWithValue(backend.services),
          fileDialogServiceProvider.overrideWithValue(_Files(path)),
          dbScopeProvider.overrideWithValue(const DbScope('profile', true)),
        ],
        child: MaterialApp(
          localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
          supportedLocales: AppLocalizations.supportedLocales,
          home: Scaffold(
            body: Builder(
              builder: (context) => TextButton(
                onPressed: () async => closed = await showDialog<bool>(
                  context: context,
                  builder: (_) => DbCsvImportDialog(service: service, session: _session, table: _table),
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
    return closed;
  };
}

Finder _key(String key) => find.byKey(ValueKey(key));

Future<void> _tap(WidgetTester tester, String key) async {
  await tester.tap(_key(key));
  await settle(tester);
}

Future<void> _choose(WidgetTester tester, String key, String label) async {
  await _tap(tester, key);
  await tester.tap(find.text(label).last);
  await settle(tester);
}

void main() {
  testWidgets('file → settings → mapping → options → review with passphrase → progress and summary', (tester) async {
    final service = _Service();
    addTearDown(service.progress.close);
    await _pump(tester, service, '/data/items.csv')();
    expect(find.text('Step 1 of 5: File'), findsOneWidget);
    await _tap(tester, 'db-csv-choose');
    expect(find.text('items.csv · 1 KB'), findsOneWidget);
    await _tap(tester, 'db-csv-next');

    // Settings with a live preview; a new delimiter reads the file again.
    expect(find.text('Step 2 of 5: Reading settings'), findsOneWidget);
    expect(find.text('alpha'), findsOneWidget);
    expect(find.text('[NULL]'), findsOneWidget);
    await _choose(tester, 'db-csv-delimiter', 'Semicolon ;');
    await tester.pump(const Duration(milliseconds: 400));
    await settle(tester);
    expect(service.previews.last.delimiter, ';');
    await _tap(tester, 'db-csv-next');

    // Mapping by name (case-insensitive); the check shows bad samples.
    expect(find.text('Step 3 of 5: Columns'), findsOneWidget);
    expect(service.checks.last.map((m) => (m.source, m.target)), [(0, 'id'), (1, 'name'), (2, 'amount')]);
    expect(find.text('1 value does not fit: abc'), findsOneWidget);
    await _choose(tester, 'db-csv-target-2', 'Skip');
    expect(service.checks.last.map((m) => m.target), ['id', 'name']);
    expect(find.text('1 value does not fit: abc'), findsNothing);
    await _tap(tester, 'db-csv-next');

    // Options: upsert by key, delete first (flagged in the review).
    expect(find.text('Step 4 of 5: Import options'), findsOneWidget);
    await _choose(tester, 'db-csv-mode', 'Insert or update by key');
    await _tap(tester, 'db-csv-truncate');
    await _tap(tester, 'db-csv-next');
    expect(_key('db-query-review'), findsOneWidget);
    expect(find.textContaining('DELETE FROM "public"."items"'), findsWidgets);
    expect(service.reviewed!.mode, DbCsvMode.upsert);
    expect(service.reviewed!.truncate, isTrue);
    expect(service.started, isEmpty);
    await tester.enterText(_key('vault-passphrase-confirm'), MockCloud.demoPassphrase);
    await tester.pump();
    await tester.tap(_key('vault-passphrase-confirm-action'));
    await settle(tester);
    expect(service.started, ['csv']);
    expect(find.text('Step 5 of 5: Import'), findsOneWidget);

    service.progress.add(const DbCsvProgress(bytesRead: 50, totalBytes: 100, rowsRead: 1, imported: 1));
    await settle(tester);
    expect(find.textContaining('Read: 1 · imported: 1'), findsOneWidget);
    expect(_key('db-csv-cancel'), findsOneWidget);
    service.progress.add(
      const DbCsvProgress(
        bytesRead: 100,
        totalBytes: 100,
        rowsRead: 3,
        imported: 2,
        failed: 1,
        done: true,
        report: '/data/items.csv.errors.csv',
      ),
    );
    await settle(tester);
    expect(find.text('Done: 2 imported, 0 skipped, 1 with errors.'), findsOneWidget);
    expect(find.text('/data/items.csv.errors.csv'), findsOneWidget);
    await _tap(tester, 'db-csv-close');
    expect(_key('db-csv-dialog'), findsNothing);
  });

  testWidgets('a declined review starts nothing and drops the ticket', (tester) async {
    final service = _Service();
    await _pump(tester, service, '/data/items.csv')();
    await _tap(tester, 'db-csv-choose');
    await _tap(tester, 'db-csv-next');
    await _tap(tester, 'db-csv-next');
    await _tap(tester, 'db-csv-next');
    await _tap(tester, 'db-csv-next');
    expect(_key('db-query-review'), findsOneWidget);
    await tester.tap(find.text('Cancel').last);
    await settle(tester);
    expect(service.started, isEmpty);
    expect(service.discarded, ['csv']);
    expect(find.text('Step 4 of 5: Import options'), findsOneWidget);
  });

  testWidgets('a file over the limit is refused before it is read', (tester) async {
    final directory = Directory.systemTemp.createTempSync('cc-csv');
    addTearDown(() => directory.deleteSync(recursive: true));
    final big = File('${directory.path}/big.csv');
    final handle = big.openSync(mode: FileMode.write)
      ..setPositionSync(dbCsvMaxBytes + 1)
      ..writeByteSync(0);
    handle.closeSync();
    final service = _Service();
    await _pump(tester, service, big.path)();
    await _tap(tester, 'db-csv-choose');
    expect(_key('db-csv-file-error'), findsOneWidget);
    expect(find.textContaining('larger than 200 MB'), findsOneWidget);
    expect(service.previews, isEmpty);
    expect(tester.widget<GlassButton>(_key('db-csv-next')).onPressed, isNull);
  });
}
