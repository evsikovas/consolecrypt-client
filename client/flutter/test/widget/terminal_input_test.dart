import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/app/app.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:xterm/xterm.dart';

import '../helpers/test_app.dart';

Future<(ProviderContainer, TerminalTab)> openTerminal(WidgetTester tester, {TerminalService? service}) async {
  final backend = testBackend();
  addTearDown(backend.dispose);
  await backend.debugSignInDemoAndUnlock();
  if (service == null) {
    await pumpApp(tester, backend);
  } else {
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
  }
  final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
  final tab = await container.read(terminalTabsProvider.notifier).open(backend.inventory.currentHosts.first);
  container.read(routerProvider).go(AppRoutes.terminal);
  await settle(tester);
  return (container, tab);
}

void main() {
  testWidgets('Windows Cyrillic input waits for the first async write before sending the next key or Enter', (
    tester,
  ) async {
    final service = _DelayedEcho();
    addTearDown(service.release);
    final (_, tab) = await openTerminal(tester, service: service);
    await _typeGreeting(tester);
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    await tester.pump();
    expect(service.started.length, 1, reason: 'only one native write may be in flight for this terminal');
    expect(service.started.single, utf8.encode('п'));
    expect(service.completed, isEmpty);
    service.release();
    await settle(tester, steps: 32);
    expect(utf8.decode(service.completed.expand((bytes) => bytes).toList()), 'привет\r');
    expect(tab.terminal.buffer.lines[0].getText(), 'привет');
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('Windows greeting and native Unicode commit survive byte-fragmented SSH echo', (tester) async {
    final service = _DelayedEcho()..release();
    final (_, tab) = await openTerminal(tester, service: service);
    await _typeGreeting(tester);
    tester.testTextInput.enterText(' ёж 🌐');
    await settle(tester, steps: 32);
    final text = tab.terminal.buffer.lines[0].getText();
    expect(text, 'привет ёж 🌐');
    expect(text.contains('\ufffd'), false);
    expect(utf8.decode(service.completed.expand((bytes) => bytes).toList()), 'привет ёж 🌐');
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('reconnection drops queued old input while a new connection remains writable', (tester) async {
    final service = _DelayedEcho(echo: false);
    addTearDown(service.release);
    final (_, tab) = await openTerminal(tester, service: service);
    tab.terminal.textInput('п');
    tab.terminal.textInput('ривет');
    await tester.pump();
    service.events.add(const TerminalStateChanged(SessionConnectionState.disconnected));
    await tester.pump();
    service.events.add(const TerminalStateChanged(SessionConnectionState.connected));
    await tester.pump();
    tab.terminal.textInput('новый');
    await tester.pump();
    service.release();
    await settle(tester);
    expect(service.started.map(utf8.decode).toList(), ['п', 'новый']);
    expect(tester.takeException(), isNull);
  });

  test('closing a terminal drops queued keys and wipes the completed input buffer', () async {
    final backend = testBackend();
    final service = _DelayedEcho(echo: false);
    addTearDown(service.release);
    final container = ProviderContainer(
      overrides: [
        appServicesProvider.overrideWithValue(backend.services),
        terminalServiceProvider.overrideWithValue(service),
      ],
    );
    addTearDown(container.dispose);
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    final profile = container.listen(activeProfileProvider, (_, _) {});
    addTearDown(profile.close);
    await pumpEventQueue();
    final controller = container.read(terminalTabsProvider.notifier);
    final tab = await controller.open(backend.inventory.currentHosts.first);
    await pumpEventQueue();
    tab.terminal.textInput('п');
    tab.terminal.textInput('ривет');
    await pumpEventQueue();
    await controller.close(tab).timeout(const Duration(seconds: 1));
    expect(service.closed, true);
    expect(service.started.map(utf8.decode).toList(), ['п']);
    expect(service.borrowed.single.every((byte) => byte == 0), true);
    expect(container.read(terminalTabsProvider).tabs, isEmpty);
  });

  testWidgets('a failed native write drops later keys without an unhandled future error', (tester) async {
    final service = _DelayedEcho(echo: false, failFirst: true);
    addTearDown(service.release);
    final (_, tab) = await openTerminal(tester, service: service);
    tab.terminal.textInput('п');
    tab.terminal.textInput('ривет');
    await tester.pump();
    service.release();
    await settle(tester);
    expect(service.started.map(utf8.decode).toList(), ['п']);
    expect(service.borrowed.single.every((byte) => byte == 0), true);
    expect(tab.state, SessionConnectionState.disconnected);
    expect(tester.takeException(), isNull);
  });

  testWidgets('an old write failure cannot disconnect or clear input from a new connection', (tester) async {
    final service = _DelayedEcho(echo: false, failFirst: true);
    addTearDown(service.release);
    final (container, tab) = await openTerminal(tester, service: service);
    tab.terminal.textInput('п');
    tab.terminal.textInput('ривет');
    await tester.pump();
    await container.read(terminalTabsProvider.notifier).reconnect(tab);
    tab.terminal.textInput('не отправлять');
    service.events.add(const TerminalStateChanged(SessionConnectionState.connected));
    await tester.pump();
    tab.terminal.textInput('новый');
    await tester.pump();
    service.release();
    await settle(tester);
    expect(service.started.map(utf8.decode).toList(), ['п', 'новый']);
    expect(service.completed.map(utf8.decode).toList(), ['новый']);
    expect(tab.state, SessionConnectionState.connected);
    expect(tester.takeException(), isNull);
  });

  testWidgets('Windows hardware letters use layout Unicode and consume the OS key exactly once', (tester) async {
    final (_, tab) = await openTerminal(tester);
    final output = <String>[];
    tab.terminal.onOutput = output.add;
    expect(await tester.sendKeyDownEvent(LogicalKeyboardKey.keyA, character: 'ф'), isTrue);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.keyA);
    expect(output.join(), 'ф');
    expect(await tester.sendKeyEvent(LogicalKeyboardKey.keyZ, character: 'Я'), isTrue);
    expect(await tester.sendKeyEvent(LogicalKeyboardKey.backquote, character: 'ё'), isTrue);
    expect(await tester.sendKeyEvent(LogicalKeyboardKey.space, character: ' '), isTrue);
    expect(await tester.sendKeyDownEvent(LogicalKeyboardKey.keyL, character: 'l'), isTrue);
    expect(await tester.sendKeyRepeatEvent(LogicalKeyboardKey.keyL, character: 'l'), isTrue);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.keyL);
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    expect(output.join(), 'фЯё ll\r');
    expect(tester.testTextInput.isRegistered, isTrue);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('one click after losing focus and selecting text reopens native Unicode input', (tester) async {
    final (_, tab) = await openTerminal(tester);
    final output = <String>[];
    tab.terminal.onOutput = output.add;
    tab.controller.setSelection(tab.terminal.buffer.createAnchor(0, 0), tab.terminal.buffer.createAnchor(3, 0));
    FocusManager.instance.primaryFocus?.unfocus();
    await tester.pump();
    await tester.tap(find.byType(TerminalView), kind: PointerDeviceKind.mouse);
    await tester.pump();
    expect(tester.testTextInput.isRegistered, isTrue);
    tester.testTextInput.enterText('Привет, ёж!');
    await tester.pump();
    expect(output.join(), 'Привет, ёж!');
    expect(tab.controller.selection, isNull);
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    expect(output.join(), 'Привет, ёж!\r');
    await settle(tester);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('switching tabs sends Cyrillic and IME composition only to the visible terminal', (tester) async {
    final (container, first) = await openTerminal(tester);
    final firstOutput = <String>[];
    first.terminal.onOutput = firstOutput.add;
    final second = await container.read(terminalTabsProvider.notifier).open(first.host);
    final secondOutput = <String>[];
    second.terminal.onOutput = secondOutput.add;
    await settle(tester);
    tester.testTextInput.enterText('вторая');
    await tester.pump();
    expect(firstOutput, isEmpty);
    expect(secondOutput.join(), 'вторая');
    container.read(terminalTabsProvider.notifier).activate(0);
    await settle(tester);
    tester.testTextInput.updateEditingValue(
      const TextEditingValue(
        text: 'первая',
        selection: TextSelection.collapsed(offset: 6),
        composing: TextRange(start: 0, end: 6),
      ),
    );
    await tester.pump();
    expect(firstOutput, isEmpty);
    tester.testTextInput.updateEditingValue(
      const TextEditingValue(text: 'первая', selection: TextSelection.collapsed(offset: 6)),
    );
    await tester.pump();
    expect(firstOutput.join(), 'первая');
    expect(secondOutput.join(), 'вторая');
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyC);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    expect(firstOutput.join(), 'первая\x03');
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
}

Future<void> _typeGreeting(WidgetTester tester) async {
  for (final entry in {
    LogicalKeyboardKey.keyG: 'п',
    LogicalKeyboardKey.keyH: 'р',
    LogicalKeyboardKey.keyB: 'и',
    LogicalKeyboardKey.keyD: 'в',
    LogicalKeyboardKey.keyT: 'е',
    LogicalKeyboardKey.keyN: 'т',
  }.entries) {
    expect(await tester.sendKeyEvent(entry.key, character: entry.value), isTrue);
  }
}

// A bridge write can complete asynchronously; its caller must keep later
// writes out of the bridge until it completes. Echo deliberately splits every
// UTF-8 scalar across frames to exercise the production incremental decoder.
final class _DelayedEcho implements TerminalService {
  _DelayedEcho({this.echo = true, this.failFirst = false});
  final bool echo;
  final bool failFirst;
  final firstWrite = Completer<void>();
  final output = StreamController<Uint8List>();
  final events = StreamController<TerminalEvent>();
  final started = <List<int>>[], completed = <List<int>>[];
  final borrowed = <Uint8List>[];
  bool closed = false;
  void release() {
    if (!firstWrite.isCompleted) firstWrite.complete();
  }

  @override
  Future<TerminalSessionHandle> open({required ObjectId hostId, required TerminalSize size}) async {
    events.add(const TerminalStateChanged(SessionConnectionState.connected));
    return TerminalSessionHandle(id: TerminalSessionId.generate(), output: output.stream, events: events.stream);
  }

  @override
  Future<void> write(TerminalSessionId id, Uint8List data) async {
    final bytes = List<int>.of(data);
    started.add(bytes);
    borrowed.add(data);
    if (started.length == 1) {
      await firstWrite.future;
      if (failFirst) throw StateError('terminal write failed');
    }
    completed.add(bytes);
    if (echo && !closed) {
      for (final byte in bytes) {
        output.add(Uint8List.fromList([byte]));
      }
    }
  }

  @override
  Future<void> close(TerminalSessionId id) async {
    if (closed) return;
    closed = true;
    release();
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
