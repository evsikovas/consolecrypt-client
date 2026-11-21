import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/app/commands.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/terminal/terminal_context_menu.dart';
import 'package:consolecrypt/wslc/app/theme.dart';
import 'package:consolecrypt/wslc/features/containers/container_actions.dart';
import 'package:consolecrypt/wslc/shared/error_text.dart';
import 'package:consolecrypt/wslc/shared/widgets.dart';
import 'package:consolecrypt/wslc/wslc_scope.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';
import 'package:xterm/xterm.dart';


/// Feeds decoded text into the terminal; multi-byte characters split across reads survive.
class _TerminalSink implements Sink<String> {
  _TerminalSink(this.terminal);
  final Terminal terminal;
  @override
  void add(String data) => terminal.write(data);
  @override
  void close() {}
}

TerminalTheme terminalTheme(DeskPalette p) => TerminalTheme(
  cursor: p.accentEnd,
  selection: p.accent.withValues(alpha: .35),
  foreground: p.terminalText,
  background: p.terminal,
  black: const Color(0xff1e2433),
  red: const Color(0xffff6b6b),
  green: const Color(0xff34d399),
  yellow: const Color(0xfffbbf24),
  blue: const Color(0xff60a5fa),
  magenta: const Color(0xffc084fc),
  cyan: const Color(0xff38bdf8),
  white: const Color(0xffcbd5e1),
  brightBlack: const Color(0xff64748b),
  brightRed: const Color(0xfff87171),
  brightGreen: const Color(0xff6ee7b7),
  brightYellow: const Color(0xfffde68a),
  brightBlue: const Color(0xff93c5fd),
  brightMagenta: const Color(0xffd8b4fe),
  brightCyan: const Color(0xff7dd3fc),
  brightWhite: const Color(0xffffffff),
  searchHitBackground: p.warning,
  searchHitBackgroundCurrent: p.accentEnd,
  searchHitForeground: Colors.black,
);

/// `wslc exec -it` inside the app: ConPTY in the core, xterm rendering here.
class TerminalPanel extends ConsumerStatefulWidget {
  const TerminalPanel({super.key, required this.containerId, required this.running});
  final String containerId;
  final bool running;
  @override
  ConsumerState<TerminalPanel> createState() => _TerminalPanelState();
}

class _TerminalPanelState extends ConsumerState<TerminalPanel> {
  late final api = WslcApiScope.of(context);

  late Terminal terminal;
  final controller = TerminalController();
  bool pasting = false;
  late ByteConversionSink decoder;
  BigInt? session;
  Timer? poll;
  bool polling = false;
  int generation = 0;
  bool reconnecting = false;
  bool ended = false;
  String? error;
  int cols = 100, rows = 30;

  @override
  void initState() {
    super.initState();
    _create();
    if (widget.running) unawaited(connect());
  }

  void _create() {
    controller.clearSelection();
    terminal = Terminal(maxLines: 10000);
    decoder = const Utf8Decoder(allowMalformed: true).startChunkedConversion(_TerminalSink(terminal));
    terminal.onOutput = (data) {
      final id = session;
      if (id == null) return;
      unawaited(api.writeTerminal(session: id, data: Uint8List.fromList(utf8.encode(data))).catchError((Object _) {}));
    };
    terminal.onResize = (width, height, _, _) {
      cols = width;
      rows = height;
      final id = session;
      if (id != null) {
        unawaited(api.resizeTerminal(session: id, cols: width, rows: height).catchError((Object _) {}));
      }
    };
  }

  bool _current(BigInt id) =>
      mounted &&
      session == id &&
      ref.read(activeProfileProvider)?.id.value == api.profileId &&
      ref.read(appStageProvider) == AppStage.unlocked;

  Future<void> _paste() async {
    final id = session;
    if (pasting || id == null || !_current(id)) return;
    pasting = true;
    try {
      final text = await readConfirmedTerminalPaste(context, isCurrent: () => _current(id));
      if (text != null && _current(id)) {
        terminal.paste(text);
        controller.clearSelection();
      }
    } finally {
      pasting = false;
    }
  }

  KeyEventResult _key(FocusNode node, KeyEvent event) {
    if (ref.read(appCommandDispatcherProvider).handleKeyEvent(event)) return KeyEventResult.handled;
    final keys = HardwareKeyboard.instance;
    if (keys.isAltPressed || keys.isMetaPressed) return KeyEventResult.ignored;
    final copy = keys.isControlPressed && keys.isShiftPressed && event.logicalKey == LogicalKeyboardKey.keyC;
    final paste =
        (keys.isControlPressed && event.logicalKey == LogicalKeyboardKey.keyV) ||
        (keys.isShiftPressed && !keys.isControlPressed && event.logicalKey == LogicalKeyboardKey.insert);
    if (!copy && !paste) return KeyEventResult.ignored;
    if (event is KeyDownEvent) {
      if (paste) {
        unawaited(_paste());
      } else {
        final selection = controller.selection;
        if (selection != null) {
          unawaited(WslcApiScope.copy(context, ClipboardData(text: terminal.buffer.getText(selection))));
        }
      }
    }
    return KeyEventResult.handled;
  }

  Future<void> connect() async {
    final opening = ++generation;
    setState(() {
      ended = false;
      error = null;
    });
    try {
      final id = await api.openTerminalSession(containerId: widget.containerId, cols: cols, rows: rows);
      if (!mounted || generation != opening) {
        await api.closeTerminalSession(session: id);
        return;
      }
      setState(() => session = id);
      poll = Timer.periodic(const Duration(milliseconds: 30), (_) => unawaited(pump()));
    } catch (e) {
      if (mounted && generation == opening) setState(() => error = errorText(e));
    }
  }

  Future<void> pump() async {
    final id = session;
    if (polling || id == null) return;
    polling = true;
    try {
      final out = await api.takeTerminalOutput(session: id);
      if (!mounted || session != id) return;
      if (out.data.isNotEmpty) decoder.add(out.data);
      if (!out.alive && out.data.isEmpty) await disconnect(ended: true);
    } catch (e) {
      if (mounted && session == id) await disconnect(ended: true);
    } finally {
      polling = false;
    }
  }

  Future<void> disconnect({bool ended = false}) async {
    generation++;
    poll?.cancel();
    poll = null;
    final id = session;
    session = null;
    if (id != null) {
      try {
        await api.closeTerminalSession(session: id);
      } catch (_) {
        // Native profile locking may already have closed this terminal lease.
      }
    }
    if (mounted && ended) setState(() => this.ended = true);
  }

  Future<void> reconnect() async {
    if (reconnecting) return;
    reconnecting = true;
    try {
      await disconnect();
      if (!mounted) return;
      setState(_create);
      await connect();
    } finally {
      reconnecting = false;
    }
  }

  @override
  void didUpdateWidget(covariant TerminalPanel old) {
    super.didUpdateWidget(old);
    if (widget.running && !old.running && session == null) unawaited(connect());
  }

  @override
  void dispose() {
    unawaited(disconnect());
    controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    if (!widget.running) {
      return const Panel(
        child: EmptyState(icon: Icons.terminal_rounded, title: 'Терминал доступен у запущенного контейнера'),
      );
    }
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(
          children: [
            StatusDot(color: session != null ? p.success : p.faint, glow: session != null),
            const SizedBox(width: 8),
            Expanded(
              child: Text(
                error ??
                    (ended
                        ? 'Сессия завершена'
                        : session == null
                        ? 'Подключение…'
                        : 'wslc exec -it · bash или sh · $cols×$rows'),
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(color: error == null ? p.muted : p.danger, fontSize: 12.5),
              ),
            ),
            TextButton.icon(
              onPressed: reconnect,
              icon: const Icon(Icons.refresh_rounded, size: 18),
              label: Text(ended || error != null ? 'Подключить снова' : 'Новая сессия'),
            ),
            IconButton(
              tooltip: 'Открыть в отдельном окне',
              onPressed: () => openContainerTerminal(context, widget.containerId),
              icon: const Icon(Icons.open_in_new_rounded, size: 19),
            ),
          ],
        ),
        const SizedBox(height: 8),
        Expanded(
          child: Container(
            padding: const EdgeInsets.all(8),
            decoration: BoxDecoration(
              color: p.terminal,
              borderRadius: BorderRadius.circular(12),
              border: Border.all(color: p.border),
            ),
            clipBehavior: Clip.antiAlias,
            child: TerminalView(
              terminal,
              controller: controller,
              onKeyEvent: _key,
              theme: terminalTheme(p),
              autofocus: true,
              hardwareKeyboardOnly: true,
              textStyle: const TerminalStyle(fontFamily: monoFont, fontFamilyFallback: monoFallback, fontSize: 13),
            ),
          ),
        ),
        const SizedBox(height: 6),
        Text(
          'Выделение копируется Ctrl+Shift+C, вставка — Ctrl+Shift+V. '
          'Закрытие вкладки завершает только эту сессию, контейнер продолжает работать.',
          style: TextStyle(color: p.faint, fontSize: 12),
        ),
      ],
    );
  }
}
