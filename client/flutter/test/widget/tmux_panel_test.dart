import 'dart:typed_data';

import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/terminal_service.dart';
import 'package:consolecrypt/core/services/tmux_service.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

class _Sessions extends Fake implements TerminalService, TmuxService {
  final rows = <int, String>{1: 'main', 2: 'build'};
  final closed = <TerminalSessionId>[];
  final parents = <TerminalSessionId>[];

  TerminalSessionHandle handle() => TerminalSessionHandle(
    id: TerminalSessionId.generate(),
    output: const Stream.empty(),
    events: Stream.value(const TerminalStateChanged(SessionConnectionState.connected)),
  );
  @override
  Future<TerminalSessionHandle> open({required ObjectId hostId, required TerminalSize size}) async => handle();
  @override
  Future<void> resize(TerminalSessionId id, TerminalSize size) async {}
  @override
  Future<void> write(TerminalSessionId id, Uint8List data) async {}
  @override
  Future<void> close(TerminalSessionId id) async => closed.add(id);

  @override
  Future<String?> tmuxVersion(TerminalSessionId parent) async => 'tmux 3.6';
  @override
  Future<List<TmuxSession>> tmuxSessions(TerminalSessionId parent) async {
    parents.add(parent);
    return [
      for (final e in rows.entries)
        TmuxSession.fromJson({
          'id': e.key,
          'pid': 44,
          'created': 10,
          'name': e.value,
          'path': '/work',
          'windows': 1,
          'clients': 0,
          'activity': 25,
        }),
    ];
  }

  @override
  Future<TerminalSessionHandle> attachTmux(TerminalSessionId parent, TmuxSession session) async {
    parents.add(parent);
    return handle();
  }

  @override
  Future<void> createTmux(TerminalSessionId parent, String name) async {
    rows[3] = name;
  }

  @override
  Future<void> renameTmux(TerminalSessionId parent, TmuxSession session, String name) async {
    rows[session.id] = name;
  }

  @override
  Future<void> killTmux(TerminalSessionId parent, TmuxSession session) async {
    rows.remove(session.id);
  }
}

void main() {
  testWidgets('tmux switches only its own client and confirms termination', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    final service = _Sessions();
    await pumpApp(tester, backend, overrides: [terminalServiceProvider.overrideWithValue(service)]);
    await enterKey(tester, 'hosts-search', 'staging-web');
    await tapKey(tester, 'connect-staging-web');
    final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    final root = container.read(terminalTabsProvider).active!;
    await tapKey(tester, 'nav-tmux');
    expect(find.text('build'), findsOneWidget);
    Future<void> button(String label, {bool last = false}) async {
      final matches = find.byTooltip(label);
      await tester.tap(last ? matches.last : matches.first);
      await settle(tester);
    }

    await button('Attach to session');
    final first = container.read(terminalTabsProvider).active!;
    expect(first.tmuxParent, same(root));
    await button('Attach to session');
    final second = container.read(terminalTabsProvider).active!;
    expect(second.tmuxSession!.name, 'build');
    expect(service.closed, [first.sessionId]);
    await button('Detach from session');
    expect(container.read(terminalTabsProvider).active, same(root));
    expect(service.closed, [first.sessionId, second.sessionId]);
    expect(service.parents.toSet(), {root.sessionId});
    await button('End session', last: true);
    await tapKey(tester, 'confirm-cancel');
    expect(service.rows.length, 2);
    await button('End session', last: true);
    await tapKey(tester, 'confirm-ok');
    expect(service.rows, {1: 'main'});
    await button('Create session');
    await enterKey(tester, 'text-input-dialog-field', 'worker');
    await tester.testTextInput.receiveAction(TextInputAction.done);
    await settle(tester);
    expect(find.text('worker'), findsOneWidget);
    await button('Rename', last: true);
    await enterKey(tester, 'text-input-dialog-field', 'deploy');
    await tester.testTextInput.receiveAction(TextInputAction.done);
    await settle(tester);
    expect(find.text('deploy'), findsOneWidget);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
}
