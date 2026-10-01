import 'dart:async';

import 'package:consolecrypt/app/app.dart';
import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

final class _ObservedTerminal implements TerminalService {
  _ObservedTerminal(this.delegate);
  final TerminalService delegate;
  final List<ObjectId> opened = [];
  Completer<void>? holdOpen;
  bool failNext = false;

  @override
  Future<TerminalSessionHandle> open({required ObjectId hostId, required TerminalSize size}) async {
    opened.add(hostId);
    await holdOpen?.future;
    if (failNext) {
      failNext = false;
      throw const AppException(AppErrorCode.notFound, 'Host not found', reason: AppErrorReason.hostNotFound);
    }
    return delegate.open(hostId: hostId, size: size);
  }

  @override
  Future<void> answerHostKey(TerminalSessionId id, HostKeyDecision decision) => delegate.answerHostKey(id, decision);
  @override
  Future<void> answerPassword(TerminalSessionId id, SecretText? password) => delegate.answerPassword(id, password);
  @override
  Future<void> close(TerminalSessionId id) => delegate.close(id);
  @override
  Future<void> reconnect(TerminalSessionId id) => delegate.reconnect(id);
  @override
  Future<void> resize(TerminalSessionId id, TerminalSize size) => delegate.resize(id, size);
  @override
  Future<void> write(TerminalSessionId id, Uint8List data) => delegate.write(id, data);
}

final class _ObservedAi implements AiService {
  _ObservedAi(this.delegate);
  final AiService delegate;
  final List<String> requests = [];
  final List<AiContextSelection> contexts = [];

  @override
  Stream<List<AiProviderConfig>> watchProviders() => delegate.watchProviders();

  @override
  Stream<AiStreamEvent> generateCommand({
    required ObjectId providerId,
    required String request,
    SnippetType? dialect,
    AiContextSelection context = AiContextSelection.none,
  }) {
    requests.add(request);
    contexts.add(context);
    return delegate.generateCommand(providerId: providerId, request: request, dialect: dialect, context: context);
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => throw UnimplementedError();
}

final class _ObservedSnippets implements SnippetService {
  _ObservedSnippets(this.delegate);
  final SnippetService delegate;
  final List<String> queries = [];

  @override
  Future<List<SnippetSearchHit>> search(String query, {int limit = 20}) {
    queries.add(query);
    return delegate.search(query, limit: limit);
  }

  @override
  Stream<List<Snippet>> watchSnippets() => delegate.watchSnippets();
  @override
  Future<Snippet> saveSnippet(Snippet snippet) => delegate.saveSnippet(snippet);
  @override
  Future<void> deleteSnippet(ObjectId id) => delegate.deleteSnippet(id);
  @override
  Future<String> render(Snippet snippet, Map<String, String> values) => delegate.render(snippet, values);
  @override
  Future<void> recordUsage(ObjectId id) => delegate.recordUsage(id);
  @override
  Future<RiskAssessment> assessRisk(
    String command, {
    RiskLevel declared = RiskLevel.unknown,
    SnippetSource source = SnippetSource.user,
  }) => delegate.assessRisk(command, declared: declared, source: source);
}

Host _host(String id, String name, String address, {String? username = 'operator', int? port = 22, ObjectId? groupId}) {
  final now = DateTime.now().toUtc();
  return Host(
    id: ObjectId(id),
    name: name,
    address: address,
    username: username,
    port: port,
    groupId: groupId,
    createdAt: now,
    updatedAt: now,
  );
}

Future<
  ({
    MockBackend backend,
    _ObservedTerminal terminal,
    _ObservedAi ai,
    _ObservedSnippets snippets,
    ProviderContainer container,
  })
>
_pump(
  WidgetTester tester,
  List<Host> hosts, {
  Group? group,
  bool demo = false,
  Size size = const Size(1600, 1000),
}) async {
  final backend = testBackend();
  addTearDown(backend.dispose);
  if (demo) {
    await backend.debugSignInDemoAndUnlock();
  } else {
    await backend.debugCreateUnlockedLocalProfile();
  }
  if (group != null) await backend.inventory.saveGroup(group);
  for (final host in hosts) {
    await backend.inventory.saveHost(host);
  }
  setTestLocale(backend, AppLocale.en);
  final terminal = _ObservedTerminal(backend.terminal);
  final ai = _ObservedAi(backend.ai);
  final snippets = _ObservedSnippets(backend.snippets);
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        appServicesProvider.overrideWithValue(backend.services),
        terminalServiceProvider.overrideWithValue(terminal),
        aiServiceProvider.overrideWithValue(ai),
        snippetServiceProvider.overrideWithValue(snippets),
      ],
      retry: (_, _) => null,
      child: const ConsoleCryptApp(),
    ),
  );
  await settle(tester);
  final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
  final knownHosts = container.listen(knownHostsProvider, (_, _) {});
  addTearDown(knownHosts.close);
  await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
  await tester.sendKeyEvent(LogicalKeyboardKey.keyK);
  await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
  await settle(tester);
  expect(find.byKey(const ValueKey('command-palette')), findsOneWidget);
  return (backend: backend, terminal: terminal, ai: ai, snippets: snippets, container: container);
}

Future<void> _query(WidgetTester tester, String query) async {
  await enterKey(tester, 'palette-input', query);
  await settle(tester);
}

Future<void> _submit(WidgetTester tester) => tester.testTextInput.receiveAction(TextInputAction.done);

Finder _hostRow(Host host) => find.byKey(ValueKey('palette-host-${host.id.value}'));

void main() {
  testWidgets('searches names, IPs, DNS addresses and usernames locally, ignoring case', (tester) async {
    final host = _host('search-host', 'Payments Production', '192.0.2.10', username: 'deploy', port: 2222);
    final dns = _host('dns-host', 'Build agent', 'build.example.org', username: 'builder');
    final fixture = await _pump(tester, [host, dns]);
    for (final query in ['PAYMENTS', '192.0.2.10', 'deploy', 'payments 192.0.2']) {
      await _query(tester, query);
      expect(_hostRow(host), findsOneWidget, reason: 'must find $query');
      expect(find.descendant(of: _hostRow(host), matching: find.text('deploy@192.0.2.10:2222')), findsOneWidget);
      expect(_hostRow(dns), findsNothing);
    }
    await _query(tester, 'BUILD.EXAMPLE');
    expect(_hostRow(dns), findsOneWidget);
    expect(_hostRow(host), findsNothing);
    expect(fixture.terminal.opened, isEmpty, reason: 'typing a query never opens SSH');
    expect(fixture.ai.requests, isEmpty, reason: 'host inventory is never sent to AI');
    expect(fixture.snippets.queries, isEmpty, reason: 'host queries cannot reach semantic/embedding snippet search');
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('uses planner-resolved inherited username and port in search and results', (tester) async {
    final now = DateTime.now().toUtc();
    final group = Group(
      id: const ObjectId('operations'),
      name: 'Operations',
      inheritedUsername: 'group-operator',
      inheritedPort: 2200,
      createdAt: now,
      updatedAt: now,
    );
    final host = _host('inherited', 'Database', 'database.example.org', username: null, port: null, groupId: group.id);
    await _pump(tester, [host], group: group);
    await _query(tester, 'GROUP-OPERATOR');
    expect(_hostRow(host), findsOneWidget);
    expect(
      find.descendant(of: _hostRow(host), matching: find.text('group-operator@database.example.org:2200')),
      findsOneWidget,
    );
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('an immediate query submission never selects an old snippet or AI result', (tester) async {
    final host = _host('immediate', 'Immediate', '192.0.2.40');
    final fixture = await _pump(tester, [host], demo: true);
    await _query(tester, 'disk space');
    expect(find.byKey(const ValueKey('palette-snippet-Disk usage')), findsOneWidget);
    await enterKey(tester, 'palette-input', '192.0.2.40');
    // No asynchronous snippet search can supersede the current host query.
    await _submit(tester);
    await settle(tester);
    expect(fixture.terminal.opened, [host.id]);
    expect(fixture.ai.requests, isEmpty);
    expect(fixture.snippets.queries, isEmpty);
    expect(find.byKey(const ValueKey('run-confirmation')), findsNothing);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('arrow keys select a second host', (tester) async {
    final alpha = _host('alpha', 'Worker Alpha', '192.0.2.41');
    final beta = _host('beta', 'Worker Beta', '192.0.2.42');
    final fixture = await _pump(tester, [alpha, beta]);
    await _query(tester, 'worker');
    await tester.sendKeyEvent(LogicalKeyboardKey.arrowDown);
    await tester.pump();
    await _submit(tester);
    await settle(tester);
    expect(fixture.terminal.opened, [beta.id]);
    expect(fixture.ai.requests, isEmpty);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('search updates when inventory changes while the palette is open', (tester) async {
    final host = _host('live-host', 'Live', '192.0.2.43');
    final fixture = await _pump(tester, [host]);
    await _query(tester, 'live');
    expect(_hostRow(host), findsOneWidget);
    await fixture.backend.inventory.deleteHost(host.id);
    await settle(tester);
    expect(_hostRow(host), findsNothing);
    final replacement = _host('live-replacement', 'Live replacement', '192.0.2.44');
    await fixture.backend.inventory.saveHost(replacement);
    await settle(tester);
    expect(_hostRow(replacement), findsOneWidget);
    await tester.tap(_hostRow(replacement));
    await settle(tester);
    expect(fixture.terminal.opened, [replacement.id]);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('local snippet results update from the stream without invoking semantic search', (tester) async {
    final fixture = await _pump(tester, []);
    await _query(tester, 'rolling-service');
    expect(find.byKey(const ValueKey('palette-snippet-Rolling status')), findsNothing);
    final now = DateTime.now().toUtc();
    final snippet = Snippet(
      id: ObjectId.generate(),
      name: 'Rolling status',
      snippetType: SnippetType.bash,
      template: 'systemctl status rolling-service',
      description: 'Check the demonstration service',
      tags: const ['status'],
      riskLevel: RiskLevel.readOnly,
      createdAt: now,
      updatedAt: now,
    );
    await fixture.backend.snippets.saveSnippet(snippet);
    await settle(tester);
    expect(find.byKey(const ValueKey('palette-snippet-Rolling status')), findsOneWidget);
    expect(fixture.snippets.queries, isEmpty);
    expect(fixture.ai.requests, isEmpty);
    await fixture.backend.snippets.deleteSnippet(snippet.id);
    await settle(tester);
    expect(find.byKey(const ValueKey('palette-snippet-Rolling status')), findsNothing);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('a keyboard-selected host disappearing cannot turn Enter into an AI request', (tester) async {
    final host = _host('deleted-host', 'Disposable demo host', '192.0.2.46');
    final fixture = await _pump(tester, [host], demo: true);
    await _query(tester, 'disposable');
    await tester.sendKeyEvent(LogicalKeyboardKey.arrowDown);
    await tester.sendKeyEvent(LogicalKeyboardKey.arrowUp);
    await tester.pump();
    await fixture.backend.inventory.deleteHost(host.id);
    await settle(tester);
    expect(_hostRow(host), findsNothing);
    await _submit(tester);
    await settle(tester);
    expect(
      fixture.ai.requests,
      isEmpty,
      reason: 'the old host row index is now AI, but AI was not explicitly selected',
    );
    expect(fixture.terminal.opened, isEmpty);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('AI generation remains an explicit action with only the supplied request/context', (tester) async {
    final host = _host('ai-privacy-host', 'Private inventory name', '192.0.2.45');
    final fixture = await _pump(tester, [host], demo: true);
    await _query(tester, '192.0.2.45');
    expect(fixture.ai.requests, isEmpty);
    await _query(tester, 'unknown-host.example.org');
    await _submit(tester);
    await settle(tester);
    expect(fixture.ai.requests, isEmpty, reason: 'Enter on an unmatched host search cannot implicitly request AI');
    await _query(tester, 'disk space');
    expect(fixture.ai.requests, isEmpty);
    await tapKey(tester, 'palette-generate');
    expect(fixture.ai.requests, ['disk space']);
    expect(fixture.ai.contexts.single.hostId, isNull);
    expect(fixture.ai.contexts.single.selectedTerminalText, isNull);
    expect(fixture.terminal.opened, isEmpty);
    expect(fixture.snippets.queries, isEmpty, reason: 'all palette results come from local provider snapshots');
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('explicitly choosing the AI row with arrow keys permits Enter', (tester) async {
    final fixture = await _pump(tester, [], demo: true);
    await _query(tester, 'another explicit request');
    await tester.sendKeyEvent(LogicalKeyboardKey.arrowDown);
    await tester.pump();
    await _submit(tester);
    await settle(tester);
    expect(fixture.ai.requests, ['another explicit request']);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('long duplicate host labels fit a compact window and remain clickable', (tester) async {
    final host = _host(
      'compact-host',
      'Long demonstration database server in the test network',
      'very-long-development-database-hostname.example.org',
      username: 'operator-with-a-long-name',
      port: 2222,
    );
    final fixture = await _pump(tester, [host], size: const Size(390, 844));
    await _query(tester, 'database');
    expect(_hostRow(host), findsOneWidget);
    await tester.tap(_hostRow(host));
    await settle(tester);
    expect(fixture.terminal.opened, [host.id]);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('Enter prefers a name/IP match and keeps host-key and password confirmation', (tester) async {
    final usernameMatch = _host('user-match', 'Alphabetical first', '192.0.2.11', username: 'payments');
    final host = _host('name-match', 'Payments', '192.0.2.12', username: 'operator');
    final fixture = await _pump(tester, [usernameMatch, host]);
    await _query(tester, 'payments');
    await _submit(tester);
    await settle(tester);
    expect(fixture.terminal.opened, [host.id]);
    expect(find.byKey(const ValueKey('command-palette')), findsNothing);
    expect(find.byKey(const ValueKey('host-key-prompt')), findsOneWidget);
    final tab = fixture.container.read(terminalTabsProvider).active!;
    expect(tab.host.id, host.id);
    expect(tab.state, SessionConnectionState.awaitingHostKey);
    expect(fixture.container.read(knownHostsProvider).requireValue, isEmpty);
    await tapKey(tester, 'accept-host-key');
    expect(tab.state, SessionConnectionState.awaitingPassword);
    expect(tab.passwordPrompt, isNotNull, reason: 'palette never supplies or stores a password');
    expect(fixture.ai.requests, isEmpty);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('one click opens a distinct duplicate name; repeated Enter/click waits for the same open', (
    tester,
  ) async {
    final one = _host('duplicate-one', 'Worker', '192.0.2.20');
    final two = _host('duplicate-two', 'Worker', '192.0.2.21');
    final fixture = await _pump(tester, [one, two]);
    await _query(tester, 'worker');
    expect(find.descendant(of: _hostRow(one), matching: find.text('operator@192.0.2.20:22')), findsOneWidget);
    expect(find.descendant(of: _hostRow(two), matching: find.text('operator@192.0.2.21:22')), findsOneWidget);
    final pending = fixture.terminal.holdOpen = Completer<void>();
    await tester.tap(_hostRow(two));
    await tester.pump();
    await _submit(tester);
    await _submit(tester);
    await tester.tap(_hostRow(one));
    await tester.pump();
    expect(fixture.terminal.opened, [two.id]);
    pending.complete();
    await settle(tester);
    expect(fixture.container.read(terminalTabsProvider).tabs, hasLength(1));
    expect(fixture.container.read(terminalTabsProvider).active!.host.id, two.id);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('a failed open keeps the palette and can retry; typing alone never calls AI', (tester) async {
    final host = _host('retry-host', 'Retry', '192.0.2.30');
    final fixture = await _pump(tester, [host], demo: true);
    fixture.terminal.failNext = true;
    await _query(tester, 'retry');
    await tester.tap(_hostRow(host));
    await settle(tester);
    expect(find.byKey(const ValueKey('command-palette')), findsOneWidget);
    expect(fixture.container.read(terminalTabsProvider).tabs, isEmpty);
    expect(fixture.ai.requests, isEmpty);
    await tester.tap(_hostRow(host));
    await settle(tester);
    expect(fixture.terminal.opened, [host.id, host.id]);
    expect(fixture.container.read(terminalTabsProvider).tabs, hasLength(1));
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
}
