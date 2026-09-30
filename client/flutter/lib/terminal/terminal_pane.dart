import 'dart:async';

import 'package:consolecrypt/app/commands.dart';
import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/terminal/terminal_context_menu.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';
import 'package:xterm/xterm.dart';

/// Keeps native terminal selection and mouse reporting; selected text or
/// Shift+right-click opts into the local menu even in a mouse-aware TUI.
class TerminalPane extends ConsumerStatefulWidget {
  const TerminalPane({required this.tab, this.active = true, super.key});
  final TerminalTab tab;
  final bool active;

  @override
  ConsumerState<TerminalPane> createState() => _TerminalPaneState();
}

class _TerminalPaneState extends ConsumerState<TerminalPane> {
  final _focus = FocusNode();
  final _view = GlobalKey<TerminalViewState>();
  final _scroll = ScrollController();
  bool? _previousPointerSuspension;

  void _activate() {
    if (!mounted || !widget.active) return;
    _focus.requestFocus();
    // Reattach native text input even when focus survived a window switch.
    // Committed text carries the current keyboard layout (including Cyrillic).
    _view.currentState?.requestKeyboard();
  }

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) => _activate());
  }

  @override
  void didUpdateWidget(TerminalPane oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!widget.active && _focus.hasFocus) _focus.unfocus();
    if (widget.active && !oldWidget.active) {
      WidgetsBinding.instance.addPostFrameCallback((_) => _activate());
    }
  }

  void _restorePointerInput() {
    final previous = _previousPointerSuspension;
    _previousPointerSuspension = null;
    if (previous != null) widget.tab.controller.setSuspendPointerInput(previous);
  }

  @override
  void dispose() {
    _restorePointerInput();
    _focus.dispose();
    _scroll.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final tab = widget.tab;
    final fontSize = ref.watch(localSettingsProvider.select((s) => s.value?.terminalFontSize ?? 13));
    final dispatcher = ref.read(appCommandDispatcherProvider);
    final scheme = ref.watch(
      localSettingsProvider.select((s) => s.value?.terminalColorScheme ?? TerminalColorScheme.system),
    );
    final custom = ref.watch(localSettingsProvider.select((s) => s.value?.customTerminalColors));
    return RepaintBoundary(
      child: ExcludeFocus(
        excluding: !widget.active,
        child: Listener(
          onPointerDown: (event) {
            if (event.buttons == kPrimaryMouseButton || event.kind == PointerDeviceKind.touch) _activate();
            if (event.buttons == kSecondaryMouseButton &&
                (tab.controller.selection != null || HardwareKeyboard.instance.isShiftPressed)) {
              _previousPointerSuspension ??= tab.controller.suspendedPointerInputs;
              tab.controller.setSuspendPointerInput(true);
            }
          },
          onPointerUp: (_) => scheduleMicrotask(_restorePointerInput),
          onPointerCancel: (_) => _restorePointerInput(),
          child: TerminalView(
            tab.terminal,
            key: _view,
            controller: tab.controller,
            focusNode: _focus,
            scrollController: _scroll,
            autofocus: widget.active,
            keyboardType: TextInputType.text,
            theme: AppTheme.terminalTheme(Theme.of(context).brightness, scheme: scheme, custom: custom),
            textStyle: TerminalStyle(
              fontSize: fontSize,
              fontFamily: AppPlatform.monospaceFamily,
              fontFamilyFallback: AppPlatform.monospaceFallback,
            ),
            onSecondaryTapUp: (details, _) {
              _focus.requestFocus();
              unawaited(showTerminalContextMenu(context, ref, tab, details.globalPosition & const Size(1, 1)));
            },
            onKeyEvent: (node, event) {
              if (event is KeyDownEvent || event is KeyRepeatEvent) {
                final keys = HardwareKeyboard.instance;
                if (event.logicalKey == LogicalKeyboardKey.contextMenu ||
                    (event.logicalKey == LogicalKeyboardKey.f10 && keys.isShiftPressed)) {
                  unawaited(
                    showTerminalContextMenu(
                      context,
                      ref,
                      tab,
                      Rect.fromCenter(center: glassAnchorRect(context).center, width: 1, height: 1),
                    ),
                  );
                  return KeyEventResult.handled;
                }
                final primary = AppPlatform.usesMeta
                    ? keys.isMetaPressed && !keys.isControlPressed
                    : keys.isControlPressed && !keys.isMetaPressed;
                if (primary &&
                    !keys.isAltPressed &&
                    !keys.isShiftPressed &&
                    event.logicalKey == LogicalKeyboardKey.keyV) {
                  unawaited(pasteIntoTerminal(context, ref, tab));
                  return KeyEventResult.handled;
                }
                if (primary &&
                    !keys.isAltPressed &&
                    !keys.isShiftPressed &&
                    event.logicalKey == LogicalKeyboardKey.keyA) {
                  selectTerminalBuffer(tab);
                  return KeyEventResult.handled;
                }
              }
              if (dispatcher.handleKeyEvent(event)) return KeyEventResult.handled;
              // Windows can deliver the Unicode WM_CHAR value on KeyEvent
              // without forwarding it to xterm's custom TextInputClient.
              // Consume printable hardware input here: returning handled also
              // prevents a second WM_CHAR insertion by Flutter's text plugin.
              // xterm skips this callback during native IME composition, so
              // composed/dead-key input still commits through TextInput.
              if (widget.active && AppPlatform.isWindows && (event is KeyDownEvent || event is KeyRepeatEvent)) {
                final keys = HardwareKeyboard.instance;
                final altGr = keys.isControlPressed && keys.isLogicalKeyPressed(LogicalKeyboardKey.altRight);
                final text = event.character;
                if (!keys.isMetaPressed &&
                    ((!keys.isControlPressed && !keys.isAltPressed) || altGr) &&
                    text != null &&
                    text.isNotEmpty &&
                    text.runes.every((rune) => rune >= 0x20 && rune != 0x7f)) {
                  tab.terminal.textInput(text);
                  if (_scroll.hasClients) _scroll.jumpTo(_scroll.position.maxScrollExtent);
                  return KeyEventResult.handled;
                }
              }
              return KeyEventResult.ignored;
            },
          ),
        ),
      ),
    );
  }
}
