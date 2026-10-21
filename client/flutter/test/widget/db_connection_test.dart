import 'dart:async';

import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/db/db_connection_dialog.dart';
import 'package:consolecrypt/db/db_import_dialog.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_providers.dart';
import 'package:consolecrypt/db/db_service.dart';
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

Future<ProviderContainer> _pump(
  WidgetTester tester,
  _Service service, {
  String language = 'en',
  Size size = const Size(1100, 900),
  double scale = 1,
  bool importing = false,
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
          body: Builder(
            builder: (context) => TextButton(
              key: const ValueKey('open'),
              onPressed: () => showAppDialog<void>(
                context,
                secure: true,
                builder: (_) => importing ? const DbImportDialog() : const DbConnectionDialog(),
              ),
              child: const Text('Open'),
            ),
          ),
        ),
      ),
    ),
  );
  await tester.tap(find.byKey(const ValueKey('open')));
  await settle(tester);
  return ProviderScope.containerOf(tester.element(find.byType(importing ? DbImportDialog : DbConnectionDialog)));
}

Future<void> _fill(WidgetTester tester) async {
  await enterKey(tester, 'db-field-name', 'fixture');
  await enterKey(tester, 'db-field-host', 'db.example.test');
}

void main() {
  testWidgets('new database connections default to production and verified TLS; secrets are wiped after save', (
    tester,
  ) async {
    final service = _Service();
    await _pump(tester, service);
    await _fill(tester);
    await enterKey(tester, 'db-field-password', 'canary-password');
    await tapKey(tester, 'db-save');
    expect(service.saved, hasLength(1));
    expect(service.saved.single.metadata.production, isTrue);
    expect(service.saved.single.metadata.tls.mode, DbTlsMode.verify);
    expect(service.saved.single.password!.isWiped, isTrue);
    expect(service.passwordAtSave, 'canary-password');
    expect(find.byType(DbConnectionDialog), findsNothing);
    expect(tester.takeException(), isNull);
  });

  for (final language in ['en', 'ru']) {
    testWidgets('connection form is usable at phone width with large text in $language', (tester) async {
      final service = _Service();
      await _pump(tester, service, language: language, size: const Size(412, 915), scale: 1.5);
      await _fill(tester);
      await tapKey(tester, 'db-save');
      expect(service.saved, hasLength(1));
      expect(tester.takeException(), isNull);
    });
  }

  testWidgets('lock dismisses password form and late saves do not reopen it', (tester) async {
    final service = _Service()..savePending = Completer<DbConnection>();
    final container = await _pump(tester, service);
    await _fill(tester);
    await enterKey(tester, 'db-field-password', 'canary-password');
    await tapKey(tester, 'db-save');
    container.read(_scope.notifier).lock();
    await settle(tester);
    service.savePending!.complete(service.saved.single.metadata);
    await settle(tester);
    expect(find.byType(DbConnectionDialog), findsNothing);
    expect(service.saved.single.password!.isWiped, isTrue);
    expect(tester.takeException(), isNull);
  });

  testWidgets('PostgreSQL "require" binds the password to TLS unless the user opts out', (tester) async {
    final service = _Service();
    await _pump(tester, service);
    final l = tester.element(find.byType(DbConnectionDialog)).l10n;
    await _fill(tester);
    await tester.ensureVisible(find.text(l.dbTls));
    await tester.tap(find.text(l.dbTls));
    await settle(tester);
    expect(find.byKey(const ValueKey('db-tls-unbound')), findsNothing);
    await tester.ensureVisible(find.byKey(const ValueKey('db-tls')));
    await tester.tap(find.byKey(const ValueKey('db-tls')));
    await settle(tester);
    await tester.tap(find.text(l.dbTlsRequire).last);
    await settle(tester);
    expect(find.text(l.dbTlsRequireHelp), findsOneWidget);
    final toggle = find.byKey(const ValueKey('db-tls-unbound'));
    expect(tester.widget<CheckboxListTile>(toggle).value, isFalse);
    await tapKey(tester, 'db-test');
    expect(service.tested.single.tls.toJson().containsKey('allow_unbound_auth'), isFalse);
    await tester.ensureVisible(toggle);
    await tester.tap(toggle);
    await settle(tester);
    await tapKey(tester, 'db-save');
    expect(service.saved.single.metadata.tls.allowUnboundAuth, isTrue);
    expect(service.saved.single.metadata.tls.toJson()['allow_unbound_auth'], isTrue);
    expect(tester.takeException(), isNull);
  });

  testWidgets('"prefer" explains when it may continue without TLS; a test shows the protection', (tester) async {
    final service = _Service()..check = DbTransportSecurity.tunnel;
    await _pump(tester, service);
    final l = tester.element(find.byType(DbConnectionDialog)).l10n;
    await enterKey(tester, 'db-field-name', 'fixture');
    await enterKey(tester, 'db-field-host', '127.0.0.1');
    await tester.ensureVisible(find.text(l.dbTls));
    await tester.tap(find.text(l.dbTls));
    await settle(tester);
    expect(find.text(l.dbTlsPreferHelp), findsOneWidget);
    expect(find.byKey(const ValueKey('db-tls-unbound')), findsNothing);
    await tapKey(tester, 'db-test');
    expect(find.text(l.dbTransportTunnel), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('connection strings cannot be put into a plain host field', (tester) async {
    final service = _Service();
    await _pump(tester, service);
    await _fill(tester);
    await enterKey(tester, 'db-field-host', 'postgres://user:password@db');
    await tapKey(tester, 'db-save');
    expect(service.saved, isEmpty);
    expect(tester.takeException(), isNull);
  });

  testWidgets('import review contains only metadata and consumes opaque ticket after explicit save', (tester) async {
    final service = _Service();
    await _pump(tester, service, importing: true);
    await enterKey(tester, 'db-import-uri', 'postgres://user:canary-password@db');
    await tapKey(tester, 'db-import-review');
    expect(service.importSecret!.isWiped, isTrue);
    expect(find.textContaining('canary-password'), findsNothing);
    expect(service.committed, isEmpty);
    await tapKey(tester, 'db-import-save');
    expect(service.committed.single.$1, 'opaque-ticket');
    expect(service.committed.single.$2.single.production, isTrue);
    expect(tester.takeException(), isNull);
  });

  testWidgets('late import result after lock is discarded without rendering or committing', (tester) async {
    final service = _Service()..importPending = Completer<DbImportReview>();
    final container = await _pump(tester, service, importing: true);
    await enterKey(tester, 'db-import-uri', 'postgres://user:canary-password@db');
    await tapKey(tester, 'db-import-review');
    container.read(_scope.notifier).lock();
    await settle(tester);
    service.importPending!.complete(_review());
    await settle(tester);
    expect(service.discarded, ['opaque-ticket']);
    expect(service.committed, isEmpty);
    expect(service.importSecret!.isWiped, isTrue);
    expect(find.byType(DbImportDialog), findsNothing);
    expect(tester.takeException(), isNull);
  });
}

DbImportReview _review() => DbImportReview(
  ticket: 'opaque-ticket',
  expiresAt: DateTime.now().add(const Duration(minutes: 5)),
  entries: const [
    DbImportEntry(
      index: 0,
      name: 'fixture',
      engine: DbEngine.postgresql,
      host: 'db.example.test',
      port: 5432,
      hasPassword: true,
    ),
  ],
);

class _Service implements DbService {
  final saved = <DbConnectionDraft>[];
  final committed = <(String, List<DbImportChoice>)>[];
  final discarded = <String>[];
  SecretText? importSecret;
  String? passwordAtSave;
  Completer<DbConnection>? savePending;
  Completer<DbImportReview>? importPending;
  @override
  Stream<void> changes() => const Stream.empty();
  @override
  Future<List<DbConnection>> connections() async => const [];
  @override
  Future<DbConnectionChoices> connectionChoices() async => const DbConnectionChoices();
  @override
  Future<DbConnection> saveConnection(DbConnectionDraft draft) async {
    saved.add(draft);
    passwordAtSave = draft.password?.expose();
    return savePending?.future ?? draft.metadata;
  }

  @override
  Future<void> deleteConnection(String id) async {}
  @override
  Future<DbConnectionCheck> testConnection(DbConnectionDraft draft) async {
    tested.add(draft.metadata);
    return DbConnectionCheck(serverVersion: '17', serverReadOnly: DbServerReadOnly.enforced, transportSecurity: check);
  }

  final tested = <DbConnection>[];
  DbTransportSecurity? check;
  @override
  Future<DbImportReview> importConnectionString(SecretText uri) async {
    importSecret = uri;
    return importPending?.future ?? _review();
  }

  @override
  Future<DbImportReview> importPgpass(SecretText contents) async => throw UnimplementedError();
  @override
  Future<void> confirmImport(String ticket, List<DbImportChoice> choices) async => committed.add((ticket, choices));
  @override
  Future<void> discardImport(String ticket) async => discarded.add(ticket);
}
