import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/app/app.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/core/widgets/risk_badge.dart';
import 'package:consolecrypt/db/db_editor.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_snippet_picker.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/snippets/snippet_picker.dart';
import 'package:consolecrypt/snippets/snippets_screen.dart';
import 'package:consolecrypt/snippets/starter_catalog.dart';
import 'package:consolecrypt/terminal/terminal_screen.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

final _en = lookupAppLocalizations(const Locale('en'));

Iterable<Snippet> _package(String id) => starterSnippetPackages(_en).firstWhere((p) => p.id == id).snippets;

void main() {
  testWidgets('Databases picker lists the engine snippets, fills variables and returns the query', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    for (final id in ['postgresql', 'mongodb']) {
      for (final s in _package(id)) {
        await backend.snippets.saveSnippet(s);
      }
    }
    String idOf(String catalogId) =>
        backend.snippets.currentSnippets.firstWhere((s) => s.catalogId == 'consolecrypt.$catalogId.v1').id.value;
    await pumpApp(tester, backend);
    await tapKey(tester, 'nav-snippets');
    final element = tester.element(find.byType(SnippetsScreen));

    final picked = pickDbSnippet(element, element as WidgetRef, engine: DbEngine.postgresql);
    await settle(tester);
    expect(find.byKey(const ValueKey('db-snippet-picker')), findsOneWidget);
    expect(find.byKey(ValueKey('db-snippet-${idOf('db.pg.activity')}')), findsOneWidget);
    Finder inPicker(String text) =>
        find.descendant(of: find.byKey(const ValueKey('db-snippet-picker')), matching: find.text(text));
    expect(inPicker('Active queries'), findsOneWidget);
    expect(inPicker('Disk usage'), findsNothing);
    expect(inPicker('Active operations'), findsNothing);
    await enterKey(tester, 'db-snippet-search', 'terminate');
    await settle(tester);
    expect(find.byKey(ValueKey('db-snippet-${idOf('db.pg.activity')}')), findsNothing);
    final terminate = find.byKey(ValueKey('db-snippet-${idOf('db.pg.terminate')}'));
    expect(
      tester.widget<RiskBadge>(find.descendant(of: terminate, matching: find.byType(RiskBadge))).risk,
      RiskLevel.destructive,
    );
    await enterKey(tester, 'db-snippet-search', 'long-running');
    await settle(tester);
    await tapKey(tester, 'db-snippet-${idOf('db.pg.long_running')}');
    expect(find.byKey(const ValueKey('variables-dialog')), findsOneWidget);
    await enterKey(tester, 'var-seconds', '5');
    await tapKey(tester, 'variables-continue');
    final text = (await picked)!;
    expect(text, contains("query_start < now() - interval '1 second' * 5"));
    expect(text, endsWith(';'));
    expect(backend.snippets.currentSnippets.firstWhere((s) => s.id.value == idOf('db.pg.long_running')).usageCount, 1);

    final editor = DbEditorController(text: 'SELECT 1;');
    editor.appendSnippet(text);
    expect(editor.text, 'SELECT 1;\n\n$text');
    expect(editor.selection.baseOffset, editor.text.length);
    final empty = DbEditorController()..appendSnippet(text);
    expect(empty.text, text);

    // MongoDB: workspace-compatible snippets only (server admin ones are terminal-only).
    final mongo = pickDbSnippet(element, element as WidgetRef, engine: DbEngine.mongodb);
    await settle(tester);
    expect(find.byKey(ValueKey('db-snippet-${idOf('db.mongodb.db_stats')}')), findsOneWidget);
    expect(find.byKey(ValueKey('db-snippet-${idOf('db.mongodb.current_ops')}')), findsNothing);
    await tapKey(tester, 'db-snippet-${idOf('db.mongodb.db_stats')}');
    expect(await mongo, 'db.runCommand({ "dbStats": 1, "scale": 1048576 })');

    // No snippets for the engine: an explanatory empty state.
    final none = pickDbSnippet(element, element as WidgetRef, engine: DbEngine.redis);
    await settle(tester);
    expect(find.byKey(const ValueKey('db-snippet-empty')), findsOneWidget);
    await tester.tap(find.text('Cancel').last);
    await settle(tester);
    expect(await none, isNull);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('terminal picker lists SQL snippets and sends a multi-line query as one bracketed paste', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    final active = _package('postgresql').firstWhere((s) => s.catalogId == 'consolecrypt.db.pg.activity.v1');
    await backend.snippets.saveSnippet(active);
    final service = _RecordingTerminal();
    setTestLocale(backend, AppLocale.en);
    tester.view.physicalSize = const Size(1600, 1000);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          appServicesProvider.overrideWithValue(backend.services),
          terminalServiceProvider.overrideWithValue(service),
        ],
        retry: (_, _) => null,
        child: const ConsoleCryptApp(),
      ),
    );
    await settle(tester);
    final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    final tab = await container.read(terminalTabsProvider.notifier).open(backend.inventory.currentHosts.first);
    container.read(routerProvider).go(AppRoutes.terminal);
    await settle(tester);
    expect(tab.isConnected, isTrue);
    // psql (readline) enables bracketed paste in its session.
    tab.terminal.write('\x1b[?2004h');
    expect(tab.terminal.bracketedPasteMode, isTrue);
    service.writes.clear();

    final element = tester.element(find.byType(TerminalScreen));
    unawaited(showSnippetPickerAndRun(element, element as WidgetRef));
    await settle(tester);
    await enterKey(tester, 'snippet-picker-search', 'Active queries');
    await settle(tester);
    await tapKey(tester, 'snippet-pick-Active queries');
    expect(find.byKey(const ValueKey('run-confirmation')), findsOneWidget);
    expect(find.text(active.template), findsOneWidget);
    await tapKey(tester, 'run-confirm');
    await settle(tester);

    final sent = service.writes.map(utf8.decode).join();
    expect(sent, '\x1b[200~${active.template.replaceAll('\n', '\r')}\x1b[201~\r');
    expect(sent, isNot(contains('sh -c')));
    expect(tester.takeException(), isNull);
  });

  testWidgets('a client snippet never opens a new shell session', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    final active = _package('mysql').first;
    await backend.snippets.saveSnippet(active);
    await pumpApp(tester, backend);
    await tapKey(tester, 'nav-snippets');
    final element = tester.element(find.byType(SnippetsScreen));
    final container = ProviderScope.containerOf(element);
    unawaited(showSnippetPickerAndRun(element, element as WidgetRef));
    await settle(tester);
    await enterKey(tester, 'snippet-picker-search', active.name);
    await settle(tester);
    await tapKey(tester, 'snippet-pick-${active.name}');
    expect(find.byKey(const ValueKey('run-confirmation')), findsNothing);
    expect(find.text(_en.snippetClientSessionRequired), findsOneWidget);
    expect(container.read(terminalTabsProvider).tabs, isEmpty);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
}

final class _RecordingTerminal implements TerminalService {
  final output = StreamController<Uint8List>();
  final events = StreamController<TerminalEvent>();
  final writes = <List<int>>[];

  @override
  Future<TerminalSessionHandle> open({required ObjectId hostId, required TerminalSize size}) async {
    events.add(const TerminalStateChanged(SessionConnectionState.connected));
    return TerminalSessionHandle(id: TerminalSessionId.generate(), output: output.stream, events: events.stream);
  }

  @override
  Future<void> write(TerminalSessionId id, Uint8List data) async => writes.add(List<int>.of(data));

  @override
  Future<void> close(TerminalSessionId id) async {
    await output.close();
    await events.close();
  }

  @override
  Future<void> resize(TerminalSessionId id, TerminalSize size) async {}
  @override
  Future<void> reconnect(TerminalSessionId id) async {}
  @override
  Future<void> answerHostKey(TerminalSessionId id, HostKeyDecision decision) async {}
  @override
  Future<void> answerPassword(TerminalSessionId id, SecretText? password) async => password?.wipe();
}
