import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/terminal/terminal_output.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:xterm/xterm.dart';

/// One terminal tab: an xterm [Terminal] bound to a core session.
/// Terminal buffers are local-only UI state (never synced, CLIENT_SPEC §12.3).
final class TerminalTab {
  TerminalTab({required this.sessionId, required this.host, required this.terminal, required this.controller})
    : title = host.name;

  final TerminalSessionId sessionId;
  final Host host;
  final Terminal terminal;
  final TerminalController controller;

  SessionConnectionState state = SessionConnectionState.connecting;

  /// Route while connecting, or the reason for a disconnect.
  String? message;

  /// Pending unknown-host-key question, or a changed-key failure.
  HostKeyInfo? hostKey;

  /// Pending connect-time password question (password not stored).
  TerminalPasswordPrompt? passwordPrompt;
  String title;
  bool exited = false;

  /// Input typed/inserted before the session is connected.
  final List<String> _pending = [];
  // Cancelled by TerminalTabsController.close / closeAll / dispose.
  // ignore: cancel_subscriptions
  StreamSubscription<String>? _output;
  // ignore: cancel_subscriptions
  StreamSubscription<TerminalEvent>? _events;

  bool get isConnected => state == SessionConnectionState.connected;

  /// Currently selected terminal text (allowed AI context, §14).
  String? get rawSelectedText {
    final selection = controller.selection;
    if (selection == null) return null;
    return terminal.buffer.getText(selection);
  }

  /// Nonblank selection for the existing live AI-context opt-in.
  String? get selectedText {
    final text = rawSelectedText?.trim();
    if (text == null) return null;
    return text.isEmpty ? null : text;
  }
}

final class TerminalTabsState {
  const TerminalTabsState({this.tabs = const [], this.activeIndex = 0, this.revision = 0});

  final List<TerminalTab> tabs;
  final int activeIndex;

  /// Bumped when a tab mutates in place (status, prompt, title).
  final int revision;

  TerminalTab? get active => tabs.isEmpty ? null : tabs[activeIndex.clamp(0, tabs.length - 1)];

  TerminalTabsState copyWith({List<TerminalTab>? tabs, int? activeIndex}) =>
      TerminalTabsState(tabs: tabs ?? this.tabs, activeIndex: activeIndex ?? this.activeIndex, revision: revision + 1);
}

class TerminalTabsController extends Notifier<TerminalTabsState> {
  /// Mirror of the open tabs for disposal (Riverpod forbids reading `state`
  /// or `ref` inside `onDispose`).
  final List<TerminalTab> _open = [];
  late TerminalService _service;

  @override
  TerminalTabsState build() {
    _service = ref.read(terminalServiceProvider);
    ref.onDispose(_disposeAll);
    // Sessions belong to the profile that opened them.
    ref.listen(activeProfileProvider.select((p) => p?.id), (prev, next) {
      if (prev != next) closeAll();
    });
    return const TerminalTabsState();
  }

  TerminalTargetPlatform get _platform => switch (defaultTargetPlatform) {
    TargetPlatform.macOS => TerminalTargetPlatform.macos,
    TargetPlatform.windows => TerminalTargetPlatform.windows,
    TargetPlatform.linux => TerminalTargetPlatform.linux,
    _ => TerminalTargetPlatform.unknown,
  };

  void _touch() => state = state.copyWith();

  Future<TerminalTab> open(Host host) async {
    final service = ref.read(terminalServiceProvider);
    final scrollback = ref.read(settingsServiceProvider).currentLocal.terminalScrollback;
    final handle = await service.open(hostId: host.id, size: TerminalSize.initial);
    final terminal = Terminal(maxLines: scrollback, platform: _platform);
    final tab = TerminalTab(sessionId: handle.id, host: host, terminal: terminal, controller: TerminalController());
    terminal.onOutput = (data) => _write(tab, data);
    terminal.onResize = (w, h, _, _) => unawaited(service.resize(tab.sessionId, TerminalSize(w, h)));
    terminal.onTitleChange = (title) {
      tab.title = title;
      _touch();
    };
    // Incremental UTF-8 + bounded parser turns keep a large attach snapshot or
    // dense live output from starving window input and frame scheduling.
    tab
      .._output = yieldingTerminalOutput(handle.output).listen(terminal.write)
      .._events = handle.events.listen((e) => _onEvent(tab, e));
    _open.add(tab);
    final tabs = [...state.tabs, tab];
    state = state.copyWith(tabs: tabs, activeIndex: tabs.length - 1);
    return tab;
  }

  void _onEvent(TerminalTab tab, TerminalEvent event) {
    switch (event) {
      case TerminalStateChanged(:final state, :final message):
        tab
          ..state = state
          ..message = message;
        if (state != SessionConnectionState.awaitingPassword) tab.passwordPrompt = null;
        if (state == SessionConnectionState.connected) {
          tab.exited = false;
          if (tab.hostKey?.changed != true) tab.hostKey = null;
          final pending = List.of(tab._pending);
          tab._pending.clear();
          for (final p in pending) {
            _send(tab, p);
          }
        }
      case TerminalHostKeyPrompt(:final info):
        tab.hostKey = info;
      case TerminalPasswordPrompt():
        tab.passwordPrompt = event;
      case TerminalTitleChanged(:final title):
        tab.title = title;
      case TerminalExited():
        tab.exited = true;
    }
    _touch();
  }

  /// Keyboard input from the xterm widget.
  void _write(TerminalTab tab, String data) {
    if (tab.isConnected) _send(tab, data);
  }

  void _send(TerminalTab tab, String data) =>
      unawaited(ref.read(terminalServiceProvider).write(tab.sessionId, Uint8List.fromList(utf8.encode(data))));

  void _sendOrQueue(TerminalTab tab, String data) {
    if (tab.isConnected) {
      _send(tab, data);
    } else {
      tab._pending.add(data);
    }
  }

  void activate(int index) {
    if (index >= 0 && index < state.tabs.length) state = state.copyWith(activeIndex: index);
  }

  Future<void> answerHostKey(TerminalTab tab, HostKeyDecision decision) async {
    tab.hostKey = null;
    _touch();
    await ref.read(terminalServiceProvider).answerHostKey(tab.sessionId, decision);
  }

  /// Answers the connect-time password prompt (`null` cancels). The secret
  /// is handed to the core and wiped; it is never stored.
  Future<void> answerPassword(TerminalTab tab, SecretText? password) async {
    tab.passwordPrompt = null;
    _touch();
    await ref.read(terminalServiceProvider).answerPassword(tab.sessionId, password);
  }

  Future<void> reconnect(TerminalTab tab) => ref.read(terminalServiceProvider).reconnect(tab.sessionId);

  Future<void> close(TerminalTab tab) async {
    _open.remove(tab);
    await tab._output?.cancel();
    await tab._events?.cancel();
    final tabs = [...state.tabs]..remove(tab);
    state = state.copyWith(tabs: tabs, activeIndex: state.activeIndex.clamp(0, tabs.isEmpty ? 0 : tabs.length - 1));
    await ref.read(terminalServiceProvider).close(tab.sessionId);
  }

  Future<void> closeActive() async {
    final tab = state.active;
    if (tab != null) await close(tab);
  }

  void closeAll() {
    _closeSessions();
    if (state.tabs.isNotEmpty) state = const TerminalTabsState();
  }

  void _closeSessions() {
    for (final tab in List.of(_open)) {
      unawaited(tab._output?.cancel());
      unawaited(tab._events?.cancel());
      unawaited(_service.close(tab.sessionId));
    }
    _open.clear();
  }

  /// "Insert": types [text] at the prompt of the active tab without
  /// pressing Enter. Returns false if there is no tab.
  bool insertIntoActive(String text) {
    final tab = state.active;
    if (tab == null) return false;
    if (text.contains('\x1b') ||
        (RegExp(r'[\r\n]').hasMatch(text) && (!tab.isConnected || !tab.terminal.bracketedPasteMode))) {
      return false;
    }
    if (tab.isConnected) {
      tab.terminal.paste(text);
    } else {
      _sendOrQueue(tab, text);
    }
    return true;
  }

  /// "Run": callers must have shown the run confirmation first (§16).
  void runInTab(TerminalTab tab, String command) => _sendOrQueue(tab, '$command\r');

  void _disposeAll() => _closeSessions();
}

final terminalTabsProvider = NotifierProvider<TerminalTabsController, TerminalTabsState>(TerminalTabsController.new);
