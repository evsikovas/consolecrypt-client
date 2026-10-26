import 'dart:async';
import 'dart:collection';
import 'dart:convert';

import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/terminal/terminal_output.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/painting.dart' show Axis;
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:xterm/xterm.dart';

/// One terminal tab: an xterm [Terminal] bound to a core session.
/// Terminal buffers are local-only UI state (never synced, CLIENT_SPEC §12.3).
final class TerminalTab {
  TerminalTab({required this.sessionId, required this.target, required this.terminal, required this.controller})
    : title = target.label;

  final TerminalSessionId sessionId;
  final TerminalTarget target;
  Host? get host => target.sshHost;
  bool get isLocal => target is LocalTerminalTarget;
  Terminal terminal;
  final TerminalController controller;
  int renderGeneration = 0;

  SessionConnectionState state = SessionConnectionState.connecting;

  /// Route while connecting, or the reason for a disconnect.
  String? message;

  /// Pending unknown-host-key question, or a changed-key failure.
  HostKeyInfo? hostKey;

  /// Pending connect-time password question (password not stored).
  TerminalPasswordPrompt? passwordPrompt;
  String title;

  /// Name chosen by the user; wins over titles set by the remote shell.
  String? customTitle;
  String get displayTitle => customTitle ?? title;
  bool exited = false;

  /// Output finished (or the terminal bell rang) while nobody was looking at
  /// this tab. Cleared when the tab is shown.
  bool attention = false;
  DateTime? _connectedAt;
  DateTime? _resizedAt;
  Timer? _quiet;

  /// Input typed/inserted before the session is connected.
  final List<String> _pending = [];
  final Queue<Uint8List> _input = Queue();
  int _inputGeneration = 0;
  bool _sendingInput = false;
  // Cancelled by TerminalTabsController.close / closeAll / dispose.
  // ignore: cancel_subscriptions
  StreamSubscription<TerminalRenderFrame>? _output;
  // ignore: cancel_subscriptions
  StreamSubscription<TerminalEvent>? _events;

  bool get isConnected => state == SessionConnectionState.connected;

  /// Split panes (TODO-07): sessions shown beside this tab along
  /// [splitAxis]. Each pane is a tab of its own whose [owner] is this one;
  /// only owners are in the tab strip.
  final List<TerminalTab> panes = [];
  Axis splitAxis = Axis.horizontal;

  /// Share of the space of each pane, this tab first.
  List<double> paneShares = const [1];

  /// The pane that takes input: 0 is this tab, then [panes].
  int focusedPane = 0;

  /// The strip tab this pane is shown in; null for a strip tab.
  TerminalTab? owner;

  /// This tab and its panes, in order.
  List<TerminalTab> get allPanes => [this, ...panes];

  /// The pane the user works in.
  TerminalTab get focused => focusedPane <= 0 || focusedPane > panes.length ? this : panes[focusedPane - 1];

  /// Unseen activity in this tab or one of its panes.
  bool get anyAttention => attention || panes.any((pane) => pane.attention);

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

final _oscSequence = RegExp(r'\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)');
final _csiSequence = RegExp(r'\x1b\[[0-9;?<>=!]*[ -/]*[@-~]');
final _escSequence = RegExp(r'\x1b(?:[()#%][0-9A-Za-z]|[=>78DEHMNOZc\\])');

/// Terminal output without escape sequences: titles (OSC, which end in BEL),
/// cursor moves and modes. Prompt redraws and title updates are therefore
/// not mistaken for a command's output or a bell.
@visibleForTesting
String visibleTerminalText(String text) =>
    text.replaceAll(_oscSequence, '').replaceAll(_csiSequence, '').replaceAll(_escSequence, '');

final class TerminalTabsState {
  const TerminalTabsState({this.tabs = const [], this.activeIndex = 0, this.revision = 0});

  final List<TerminalTab> tabs;
  final int activeIndex;

  /// Bumped when a tab mutates in place (status, prompt, title).
  final int revision;

  /// The tab selected in the strip.
  TerminalTab? get activeTab => tabs.isEmpty ? null : tabs[activeIndex.clamp(0, tabs.length - 1)];

  /// The session the user works in: the focused pane of the active tab.
  TerminalTab? get active => activeTab?.focused;

  /// Tabs with unseen finished activity.
  int get attentionCount => tabs.where((tab) => tab.anyAttention).length;

  TerminalTabsState copyWith({List<TerminalTab>? tabs, int? activeIndex}) =>
      TerminalTabsState(tabs: tabs ?? this.tabs, activeIndex: activeIndex ?? this.activeIndex, revision: revision + 1);
}

class TerminalTabsController extends Notifier<TerminalTabsState> {
  /// Mirror of the open tabs for disposal (Riverpod forbids reading `state`
  /// or `ref` inside `onDispose`).
  final List<TerminalTab> _open = [];
  late TerminalService _service;
  int _generation = 0;
  bool _disposed = false;

  @override
  TerminalTabsState build() {
    _service = ref.read(terminalServiceProvider);
    ref.onDispose(_disposeAll);
    // Sessions belong to the profile that opened them.
    ref.listen(activeProfileProvider.select((p) => p?.id), (prev, next) {
      if (prev != next) closeAll();
    });
    ref.listen(vaultStatusProvider.select((s) => s.value?.isUnlocked), (_, unlocked) {
      if (unlocked == false) closeAll();
    });
    return const TerminalTabsState();
  }

  TerminalTargetPlatform get _platform => switch (defaultTargetPlatform) {
    TargetPlatform.macOS => TerminalTargetPlatform.macos,
    TargetPlatform.windows => TerminalTargetPlatform.windows,
    TargetPlatform.linux => TerminalTargetPlatform.linux,
    _ => TerminalTargetPlatform.unknown,
  };

  void _touch() {
    if (!_disposed) state = state.copyWith();
  }

  Future<TerminalTab> open(Host host) => openTarget(SshTerminalTarget(host));

  Future<TerminalTab> openTarget(TerminalTarget target) async {
    final generation = _generation;
    final service = ref.read(terminalServiceProvider);
    final handle = await service.openTarget(target: target, size: TerminalSize.initial);
    if (_disposed || generation != _generation) {
      await service.close(handle.id);
      throw const AppException(AppErrorCode.notFound, 'Session closed', reason: AppErrorReason.sessionClosed);
    }
    return openHandle(target: target, handle: handle);
  }

  Future<TerminalTab> openRuntime(String runtimeId) async {
    final generation = _generation;
    final service = ref.read(terminalServiceProvider);
    final attachment = await service.attachRuntime(runtimeId);
    if (_disposed || generation != _generation) {
      await service.close(attachment.handle.id);
      throw const AppException(AppErrorCode.notFound, 'Session closed', reason: AppErrorReason.sessionClosed);
    }
    return openHandle(target: attachment.target, handle: attachment.handle);
  }

  /// Adopt an already opened, profile-owned native handle without reconnecting
  /// or spawning another process. Used by approved CLI and runtime operations.
  TerminalTab openHandle({required TerminalTarget target, required TerminalSessionHandle handle}) {
    if (_disposed) {
      unawaited(_service.close(handle.id));
      throw const AppException(AppErrorCode.notFound, 'Session closed', reason: AppErrorReason.sessionClosed);
    }
    final existing = _open.where((tab) => tab.sessionId == handle.id).firstOrNull;
    if (existing != null) {
      activate(state.tabs.indexOf(existing.owner ?? existing));
      return existing;
    }
    final tab = _adopt(target, handle);
    final tabs = [...state.tabs, tab];
    state = state.copyWith(tabs: tabs, activeIndex: tabs.length - 1);
    return tab;
  }

  /// A tab for an opened [handle]: terminal, output and events bound.
  TerminalTab _adopt(TerminalTarget target, TerminalSessionHandle handle) {
    final service = ref.read(terminalServiceProvider);
    final scrollback = ref.read(settingsServiceProvider).currentLocal.terminalScrollback;
    final terminal = Terminal(maxLines: scrollback, platform: _platform);
    final tab = TerminalTab(sessionId: handle.id, target: target, terminal: terminal, controller: TerminalController());
    _bindTerminal(tab, service);
    // Reset markers share the byte queue: old buffered data drains first,
    // then the parser and view reset before the new snapshot is replayed.
    tab
      .._output = yieldingTerminalFrames(handle.framedOutput ?? handle.output.map(TerminalOutputFrame.new))
          .listen((frame) {
            if (!_open.contains(tab)) return;
            if (frame.reset) {
              final previous = tab.terminal;
              previous.onOutput = null;
              previous.onResize = null;
              previous.onTitleChange = null;
              tab.controller.clearSelection();
              tab.terminal = Terminal(maxLines: previous.maxLines, platform: _platform)
                ..resize(previous.viewWidth, previous.viewHeight);
              tab.renderGeneration++;
              _bindTerminal(tab, service);
              _touch();
            } else {
              tab.terminal.write(frame.text);
              final visible = visibleTerminalText(frame.text);
              _noteActivity(tab, bell: visible.contains('\x07'), text: visible.trim().isNotEmpty);
            }
          })
      .._events = handle.events.listen((e) => _onEvent(tab, e));
    _open.add(tab);
    return tab;
  }

  /// Most panes in one tab.
  static const maxPanes = 4;

  /// Opens another session to the focused pane's target beside it
  /// (TODO-07). The first split sets the [axis]; later panes join it.
  /// Returns null when the tab is full or went away meanwhile.
  Future<TerminalTab?> split(TerminalTab tab, Axis axis) async {
    final host = tab.owner ?? tab;
    if (host.allPanes.length >= maxPanes) return null;
    final generation = _generation;
    final service = ref.read(terminalServiceProvider);
    final target = host.focused.target;
    final handle = await service.openTarget(target: target, size: TerminalSize.initial);
    if (_disposed || generation != _generation || !state.tabs.contains(host) || host.allPanes.length >= maxPanes) {
      await service.close(handle.id);
      return null;
    }
    final pane = _adopt(target, handle)..owner = host;
    if (host.panes.isEmpty) host.splitAxis = axis;
    host.panes.add(pane);
    host
      ..paneShares = _equal(host.allPanes.length)
      ..focusedPane = host.panes.length;
    _touch();
    return pane;
  }

  static List<double> _equal(int count) => List.filled(count, 1 / count);

  /// Gives input to [pane] of its tab.
  void focusPane(TerminalTab pane) {
    final host = pane.owner ?? pane;
    final index = host.allPanes.indexOf(pane);
    if (index < 0 || index == host.focusedPane) return;
    host.focusedPane = index;
    if (_viewing && identical(state.activeTab, host)) _seen(pane);
    _touch();
  }

  /// Moves input to the next (or previous) pane of the active tab.
  void focusNextPane({bool backwards = false}) {
    final host = state.activeTab;
    if (host == null || host.panes.isEmpty) return;
    final count = host.allPanes.length;
    focusPane(host.allPanes[(host.focusedPane + (backwards ? count - 1 : 1)) % count]);
  }

  /// New shares of a split tab's panes (a divider was dragged).
  void resizePanes(TerminalTab host, List<double> shares) {
    if (shares.length != host.allPanes.length || shares.any((share) => share <= 0)) return;
    final total = shares.fold<double>(0, (sum, share) => sum + share);
    host.paneShares = [for (final share in shares) share / total];
    _touch();
  }

  void _bindTerminal(TerminalTab tab, TerminalService service) {
    final terminal = tab.terminal;
    terminal.onOutput = (data) => _write(tab, data);
    terminal.onResize = (w, h, _, _) {
      // The shell redraws its prompt after a resize; that is not activity.
      tab._resizedAt = DateTime.now();
      unawaited(service.resize(tab.sessionId, TerminalSize(w, h)));
    };
    terminal.onTitleChange = (title) {
      tab.title = title;
      _touch();
    };
  }

  void _onEvent(TerminalTab tab, TerminalEvent event) {
    if (!_open.contains(tab)) return;
    switch (event) {
      case TerminalStateChanged(:final state, :final message):
        if (!tab.isConnected || state != SessionConnectionState.connected) _invalidateInput(tab);
        tab
          ..state = state
          ..message = message;
        if (state != SessionConnectionState.awaitingPassword) tab.passwordPrompt = null;
        if (state == SessionConnectionState.connected) {
          tab._connectedAt ??= DateTime.now();
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
        if (!_watched(tab)) tab.attention = true;
    }
    _touch();
  }

  /// Keyboard input from the xterm widget.
  void _write(TerminalTab tab, String data) {
    if (tab.isConnected) _send(tab, data);
  }

  void _send(TerminalTab tab, String data) {
    if (!_open.contains(tab) || !tab.isConnected || data.isEmpty) return;
    tab._input.add(Uint8List.fromList(utf8.encode(data)));
    if (tab._sendingInput) return;
    tab._sendingInput = true;
    unawaited(_drainInput(tab, tab._inputGeneration));
  }

  // Await the native write before dispatching the next input event. Otherwise
  // async bridge calls can deliver a later key (including Enter) first.
  Future<void> _drainInput(TerminalTab tab, int generation) async {
    while (_open.contains(tab) && tab.isConnected && generation == tab._inputGeneration && tab._input.isNotEmpty) {
      final bytes = tab._input.removeFirst();
      try {
        await _service.write(tab.sessionId, bytes);
      } catch (_) {
        if (_open.contains(tab) && generation == tab._inputGeneration) {
          _invalidateInput(tab);
          tab
            ..state = SessionConnectionState.disconnected
            ..message = null;
          _touch();
        }
      } finally {
        bytes.fillRange(0, bytes.length, 0);
      }
    }
    if (generation == tab._inputGeneration) tab._sendingInput = false;
  }

  // Queued input belongs to one connection. An already-dispatched native
  // write cannot be recalled, but its completion must not affect a new one.
  void _invalidateInput(TerminalTab tab) {
    tab._inputGeneration++;
    for (final bytes in tab._input) {
      bytes.fillRange(0, bytes.length, 0);
    }
    tab._input.clear();
    tab._sendingInput = false;
  }

  void _sendOrQueue(TerminalTab tab, String data) {
    if (tab.isConnected) {
      _send(tab, data);
    } else {
      tab._pending.add(data);
    }
  }

  void activate(int index) {
    if (index < 0 || index >= state.tabs.length) return;
    if (_viewing) state.tabs[index].allPanes.forEach(_seen);
    state = state.copyWith(activeIndex: index);
  }

  /// Whether the terminal workspace is on screen (reported by the screen).
  bool _viewing = false;

  /// Output right after connecting (banner, prompt) is ignored for this long.
  @visibleForTesting
  static Duration activityGrace = const Duration(seconds: 3);

  /// Delay between the first unseen output and marking the tab.
  @visibleForTesting
  static Duration activityQuiet = const Duration(milliseconds: 1500);

  void setViewing(bool viewing) {
    if (_viewing == viewing) return;
    _viewing = viewing;
    final active = state.activeTab;
    if (viewing && active != null && active.anyAttention) {
      active.allPanes.forEach(_seen);
      _touch();
    }
  }

  void _seen(TerminalTab tab) {
    tab._quiet?.cancel();
    tab.attention = false;
  }

  bool _watched(TerminalTab tab) => _viewing && identical(state.activeTab, tab.owner ?? tab);

  /// Output in an unwatched tab marks it shortly after it starts, whether the
  /// command finished or keeps streaming (ping, tail -f); a bell marks it at
  /// once. The first seconds after connecting (banner, prompt) never count.
  void _noteActivity(TerminalTab tab, {required bool bell, required bool text}) {
    final connected = tab._connectedAt, resized = tab._resizedAt, now = DateTime.now();
    if (_watched(tab) ||
        tab.attention ||
        (!bell && !text) ||
        connected == null ||
        now.difference(connected) < activityGrace ||
        (resized != null && now.difference(resized) < activityGrace)) {
      return;
    }
    if (bell) {
      tab._quiet?.cancel();
      if (!tab.attention) {
        tab.attention = true;
        _touch();
      }
      return;
    }
    if (tab._quiet?.isActive ?? false) return;
    tab._quiet = Timer(activityQuiet, () {
      if (!_open.contains(tab) || _watched(tab) || tab.attention) return;
      tab.attention = true;
      _touch();
    });
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

  Future<void> reconnect(TerminalTab tab) async {
    if (!_open.contains(tab)) return;
    _invalidateInput(tab);
    tab.state = SessionConnectionState.reconnecting;
    _touch();
    await _service.reconnect(tab.sessionId);
  }

  /// Closes one session: a pane leaves its tab; a split tab hands its
  /// place to its first pane; any other tab leaves the strip.
  Future<void> close(TerminalTab tab) async {
    _open.remove(tab);
    tab._quiet?.cancel();
    _invalidateInput(tab);
    await tab._output?.cancel();
    await tab._events?.cancel();
    if (_disposed) return _service.close(tab.sessionId);
    final owner = tab.owner;
    if (owner != null) {
      final index = owner.allPanes.indexOf(tab);
      owner.panes.remove(tab);
      tab.owner = null;
      if (owner.focusedPane >= index && owner.focusedPane > 0) owner.focusedPane--;
      owner.paneShares = _equal(owner.allPanes.length);
      _touch();
    } else if (tab.panes.isNotEmpty && state.tabs.contains(tab)) {
      final heir = tab.panes.removeAt(0)..owner = null;
      heir.panes.addAll(tab.panes);
      for (final pane in heir.panes) {
        pane.owner = heir;
      }
      heir
        ..splitAxis = tab.splitAxis
        ..focusedPane = 0
        ..paneShares = _equal(heir.allPanes.length)
        ..customTitle ??= tab.customTitle;
      tab.panes.clear();
      final tabs = [...state.tabs];
      tabs[tabs.indexOf(tab)] = heir;
      state = state.copyWith(tabs: tabs);
    } else {
      final tabs = [...state.tabs]..remove(tab);
      state = state.copyWith(tabs: tabs, activeIndex: state.activeIndex.clamp(0, tabs.isEmpty ? 0 : tabs.length - 1));
    }
    await _service.close(tab.sessionId);
  }

  /// Closes a strip tab with all its panes.
  Future<void> closeTab(TerminalTab tab) async {
    for (final pane in [...tab.panes]) {
      await close(pane);
    }
    await close(tab);
  }

  /// Moves the tab at [from] to [to] (drag in the tab strip or column); the
  /// active tab stays active.
  void moveTab(int from, int to) {
    final tabs = [...state.tabs];
    if (from == to || from < 0 || to < 0 || from >= tabs.length || to >= tabs.length) return;
    final active = tabs[state.activeIndex.clamp(0, tabs.length - 1)];
    tabs.insert(to, tabs.removeAt(from));
    state = state.copyWith(tabs: tabs, activeIndex: tabs.indexOf(active));
  }

  void rename(TerminalTab tab, String? name) {
    final trimmed = name?.trim();
    tab.customTitle = trimmed == null || trimmed.isEmpty ? null : trimmed;
    _touch();
  }

  Future<TerminalTab> duplicate(TerminalTab tab) => openTarget(tab.target);

  Future<void> closeOthers(TerminalTab keep) async {
    for (final tab in [...state.tabs]) {
      if (!identical(tab, keep)) await closeTab(tab);
    }
    final index = state.tabs.indexOf(keep);
    if (index >= 0) activate(index);
  }

  Future<void> closeToRight(TerminalTab tab) async {
    final index = state.tabs.indexOf(tab);
    if (index < 0) return;
    for (final other in state.tabs.sublist(index + 1).toList()) {
      await closeTab(other);
    }
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
    _generation++;
    for (final tab in List.of(_open)) {
      _invalidateInput(tab);
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
  void runInTab(TerminalTab tab, String command) {
    // Local generated/snippet execution needs its own native approval target.
    // Typing and explicit Insert remain ordinary terminal input.
    if (tab.isLocal || !_open.contains(tab)) return;
    if (tab.isConnected && command.contains('\n') && tab.terminal.bracketedPasteMode && !command.contains('\x1b')) {
      // One bracketed paste, then Enter: an interactive client (psql, mysql,
      // clickhouse-client, a shell) receives the multi-line statement as a
      // whole instead of executing it line by line. Line breaks are sent as
      // CR, like a paste from a real terminal.
      tab.terminal.paste(command.replaceAll('\r\n', '\n').replaceAll('\n', '\r'));
      _send(tab, '\r');
    } else {
      _sendOrQueue(tab, '$command\r');
    }
  }

  void _disposeAll() {
    _disposed = true;
    _closeSessions();
  }
}

final terminalTabsProvider = NotifierProvider<TerminalTabsController, TerminalTabsState>(TerminalTabsController.new);
