import 'dart:async';

import 'package:consolecrypt/app/commands.dart';
import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/app/workspace_tools_controller.dart';
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
  int? _selectionPointer;
  Offset? _selectionStart;
  Offset? _selectionPosition;
  CellAnchor? _selectionBase;
  Timer? _selectionScrollTimer;
  bool _selectionScrolled = false;
  bool _selectionUpdateScheduled = false;

  void _activate() {
    if (!mounted || !widget.active || !_focus.canRequestFocus) return;
    _focus.requestFocus();
    // Reattach native text input even when focus survived a window switch.
    // Committed text carries the current keyboard layout (including Cyrillic).
    _view.currentState?.requestKeyboard();
    // Focus changes settle after pointer dispatch. Attach after that change as
    // well, without taking focus back from a subsequently clicked input field.
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted && widget.active && _focus.hasFocus) _view.currentState?.requestKeyboard();
    });
  }

  @override
  void initState() {
    super.initState();
    _scroll.addListener(_onSelectionScroll);
    WidgetsBinding.instance.addPostFrameCallback((_) => _activate());
  }

  @override
  void didUpdateWidget(TerminalPane oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!widget.active || widget.tab != oldWidget.tab) _stopSelectionDrag();
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

  void _startSelectionDrag(PointerDownEvent event) {
    if (!widget.active || event.kind != PointerDeviceKind.mouse || event.buttons != kPrimaryMouseButton) return;
    final tab = widget.tab;
    final local = tab.controller.suspendedPointerInputs || HardwareKeyboard.instance.isShiftPressed;
    if (tab.terminal.mouseMode != MouseMode.none && !local) return;
    final render = _view.currentState?.renderTerminal;
    if (render == null) return;
    _stopSelectionDrag();
    _selectionPointer = event.pointer;
    _selectionStart = event.position;
    _selectionPosition = event.position;
    _selectionBase = tab.terminal.buffer.createAnchorFromOffset(
      render.getCellOffset(render.globalToLocal(event.position)),
    );
  }

  void _stopSelectionDrag() {
    _selectionScrollTimer?.cancel();
    _selectionScrollTimer = null;
    _selectionBase?.dispose();
    _selectionBase = null;
    _selectionPointer = null;
    _selectionStart = null;
    _selectionPosition = null;
    _selectionScrolled = false;
  }

  void _onSelectionScroll() {
    if (_selectionPointer != null) {
      _selectionScrolled = true;
      _scheduleSelectionUpdate();
    }
  }

  void _scheduleSelectionUpdate() {
    if (_selectionUpdateScheduled) return;
    _selectionUpdateScheduled = true;
    // xterm resolves its drag recognizer after raw pointer dispatch. Keep its
    // normal selection gestures, then extend from a stable buffer anchor.
    WidgetsBinding.instance.addPostFrameCallback((_) {
      _selectionUpdateScheduled = false;
      if (!mounted) return;
      _updateSelectionDrag();
    });
  }

  void _updateSelectionDrag() {
    final base = _selectionBase;
    final point = _selectionPosition;
    final start = _selectionStart;
    if (!widget.active || base == null || !base.attached || point == null || start == null) {
      _stopSelectionDrag();
      return;
    }
    if ((point - start).distance < kPanSlop || widget.tab.controller.selection == null) return;
    final render = _view.currentState?.renderTerminal;
    if (render == null || !render.attached) return;
    final local = render.globalToLocal(point);
    final outside = local.dy < 0 || local.dy >= render.size.height;
    if (outside && !widget.tab.terminal.isUsingAltBuffer && _scroll.hasClients) {
      _selectionScrollTimer ??= Timer.periodic(const Duration(milliseconds: 50), (_) => _scrollSelection());
    } else {
      _selectionScrollTimer?.cancel();
      _selectionScrollTimer = null;
    }
    if (_selectionScrolled) _extendSelection();
  }

  void _extendSelection() {
    final base = _selectionBase;
    final point = _selectionPosition;
    final render = _view.currentState?.renderTerminal;
    if (base == null || !base.attached || point == null || render == null || !render.attached) return;
    final local = render.globalToLocal(point);
    var extent = render.getCellOffset(Offset(local.dx, local.dy.clamp(0.0, render.size.height - 1)));
    if (extent.x >= base.x) extent = CellOffset(extent.x + 1, extent.y);
    final buffer = widget.tab.terminal.buffer;
    widget.tab.controller.setSelection(
      buffer.createAnchorFromOffset(base.offset),
      buffer.createAnchorFromOffset(extent),
    );
  }

  void _scrollSelection() {
    if (!mounted || !widget.active || widget.tab.controller.selection == null || !_scroll.hasClients) {
      _stopSelectionDrag();
      return;
    }
    final render = _view.currentState?.renderTerminal;
    final point = _selectionPosition;
    if (render == null || !render.attached || point == null || widget.tab.terminal.isUsingAltBuffer) {
      _stopSelectionDrag();
      return;
    }
    final y = render.globalToLocal(point).dy;
    final overrun = y < 0
        ? y
        : y >= render.size.height
        ? y - render.size.height
        : 0.0;
    if (overrun == 0) {
      _selectionScrollTimer?.cancel();
      _selectionScrollTimer = null;
      return;
    }
    final lines = (1 + overrun.abs() / render.lineHeight).clamp(1.0, 6.0);
    final position = _scroll.position;
    final next = (position.pixels + overrun.sign * lines * render.lineHeight).clamp(
      position.minScrollExtent,
      position.maxScrollExtent,
    );
    _selectionScrolled = true;
    if (next != position.pixels) _scroll.jumpTo(next);
    _extendSelection();
    if (next == position.minScrollExtent || next == position.maxScrollExtent) {
      _selectionScrollTimer?.cancel();
      _selectionScrollTimer = null;
    }
  }

  @override
  void dispose() {
    _stopSelectionDrag();
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
    ref.listen(workspaceToolsProvider.select((s) => s.selected), (previous, next) {
      if (previous == null || next != null) return;
      // In a compact window the dismissing click belongs to the tool barrier,
      // so the terminal never receives pointerDown. Wait for ExcludeFocus to
      // lift, then restore input only in the visible, active workspace.
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted && TickerMode.valuesOf(context).enabled && ref.read(workspaceToolsProvider).selected == null) {
          _activate();
        }
      });
    });
    return RepaintBoundary(
      child: ExcludeFocus(
        excluding: !widget.active,
        child: Listener(
          onPointerDown: (event) {
            // A tool's EditableText.onTapOutside also runs during this pointer
            // dispatch and can undo an immediate focus request. Activate once
            // that outside-tap handler has finished, still within this click.
            if (event.buttons == kPrimaryMouseButton || event.kind == PointerDeviceKind.touch) {
              scheduleMicrotask(_activate);
            }
            if ((event.buttons == kSecondaryMouseButton && tab.controller.selection != null) ||
                (HardwareKeyboard.instance.isShiftPressed &&
                    (event.buttons == kPrimaryMouseButton || event.buttons == kSecondaryMouseButton))) {
              _previousPointerSuspension ??= tab.controller.suspendedPointerInputs;
              tab.controller.setSuspendPointerInput(true);
            }
            _startSelectionDrag(event);
          },
          onPointerMove: (event) {
            if (event.pointer != _selectionPointer) return;
            _selectionPosition = event.position;
            _scheduleSelectionUpdate();
          },
          onPointerUp: (event) {
            if (event.pointer == _selectionPointer) _stopSelectionDrag();
            scheduleMicrotask(_restorePointerInput);
          },
          onPointerCancel: (event) {
            if (event.pointer == _selectionPointer) _stopSelectionDrag();
            _restorePointerInput();
          },
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
