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
  const TerminalPane({required this.tab, super.key});
  final TerminalTab tab;

  @override
  ConsumerState<TerminalPane> createState() => _TerminalPaneState();
}

class _TerminalPaneState extends ConsumerState<TerminalPane> {
  final _focus = FocusNode();
  bool? _previousPointerSuspension;

  void _restorePointerInput() {
    final previous = _previousPointerSuspension;
    _previousPointerSuspension = null;
    if (previous != null) widget.tab.controller.setSuspendPointerInput(previous);
  }

  @override
  void dispose() {
    _restorePointerInput();
    _focus.dispose();
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
      child: Listener(
        onPointerDown: (event) {
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
          controller: tab.controller,
          focusNode: _focus,
          autofocus: true,
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
            return dispatcher.handleKeyEvent(event) ? KeyEventResult.handled : KeyEventResult.ignored;
          },
        ),
      ),
    );
  }
}
