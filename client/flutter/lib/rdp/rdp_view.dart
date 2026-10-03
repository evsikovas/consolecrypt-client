import 'dart:async';
import 'dart:ui' as ui;

import 'package:consolecrypt/rdp/rdp_service.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

/// Fits the remote desktop without cropping. Pointer coordinates use the same
/// rectangle as the painter, including letterboxing and display scale.
Rect rdpFitRect(Size viewport, int width, int height) {
  final fitted = applyBoxFit(BoxFit.contain, Size(width.toDouble(), height.toDouble()), viewport).destination;
  return Alignment.center.inscribe(fitted, Offset.zero & viewport);
}

class RdpView extends StatefulWidget {
  const RdpView({
    super.key,
    required this.frame,
    required this.width,
    required this.height,
    required this.enabled,
    required this.onInput,
    this.localClipboardEnabled = false,
    this.onPaste,
    this.onInteractionCancelled,
  });
  final RdpFrame? frame;
  final int width, height;
  final bool enabled;
  final ValueChanged<List<RdpInput>> onInput;

  /// Immediate native cancellation, independent of the ordinary input FIFO.
  final VoidCallback? onInteractionCancelled;
  final bool localClipboardEnabled;
  final Future<void> Function(bool Function() isInputCurrent)? onPaste;
  @override
  State<RdpView> createState() => _RdpViewState();
}

class _RdpViewState extends State<RdpView> with TextInputClient {
  final _focus = FocusNode(debugLabel: 'rdp-input');
  TextInputConnection? _connection;
  static const _empty = TextEditingValue(text: '\u200b', selection: TextSelection.collapsed(offset: 1));
  TextEditingValue _editing = _empty;
  final Map<PhysicalKeyboardKey, (int, bool)> _held = {};
  ui.Image? _image;
  RdpFrame? _pending;
  bool _decoding = false;
  int _decodeGeneration = 0;
  Size _viewport = Size.zero;
  int _buttons = 0;
  int _inputGeneration = 0;
  bool _pastePending = false;
  final Set<PhysicalKeyboardKey> _pasteKeys = {};

  @override
  void initState() {
    super.initState();
    _focus.addListener(_focusChanged);
    _offer(widget.frame);
  }

  @override
  void didUpdateWidget(RdpView oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.localClipboardEnabled != widget.localClipboardEnabled || oldWidget.enabled != widget.enabled) {
      _inputGeneration++;
    }
    if (!widget.enabled) {
      if (oldWidget.enabled) {
        _held.clear();
        _buttons = 0;
        if (oldWidget.onInteractionCancelled != null) {
          oldWidget.onInteractionCancelled!();
        } else {
          oldWidget.onInput(const [RdpReleaseAllInput()]);
        }
      }
      _focus.unfocus();
      _dropImages();
    } else if (!oldWidget.enabled || !identical(widget.frame, oldWidget.frame)) {
      _offer(widget.frame);
    }
  }

  void _offer(RdpFrame? frame) {
    if (!widget.enabled || frame == null) return;
    _pending = frame; // Replace, never enqueue a backlog of frames.
    _decodeNext();
  }

  void _decodeNext() {
    if (_decoding || _pending == null || !mounted) return;
    final frame = _pending!;
    _pending = null;
    _decoding = true;
    final generation = _decodeGeneration;
    ui.decodeImageFromPixels(frame.rgba, frame.width, frame.height, ui.PixelFormat.rgba8888, (image) {
      _decoding = false;
      if (!mounted || !widget.enabled || generation != _decodeGeneration) {
        image.dispose();
      } else {
        final previous = _image;
        setState(() => _image = image);
        previous?.dispose();
      }
      if (mounted) _decodeNext();
    }, rowBytes: frame.width * 4);
  }

  void _dropImages() {
    _decodeGeneration++;
    _pending = null;
    _image?.dispose();
    _image = null;
  }

  void _activate() {
    if (!widget.enabled) return;
    _focus.requestFocus();
    _openInput();
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted && widget.enabled && _focus.hasFocus) _openInput();
    });
  }

  void _focusChanged() {
    if (_focus.hasFocus && widget.enabled) {
      _openInput();
    } else {
      _inputGeneration++;
      _pasteKeys.clear();
      _releaseHeld(cancelInteraction: true);
      _connection?.close();
      _connection = null;
      _editing = _empty;
    }
  }

  void _openInput() {
    if (!widget.enabled || !_focus.hasFocus) return;
    if (_connection?.attached != true) {
      _connection = TextInput.attach(
        this,
        const TextInputConfiguration(
          inputType: TextInputType.multiline,
          inputAction: TextInputAction.newline,
          autocorrect: false,
          enableSuggestions: false,
          enableIMEPersonalizedLearning: false,
        ),
      );
      _editing = _empty;
      _connection!.setEditingState(_editing);
    }
    _connection!.show();
  }

  void _send(List<RdpInput> events) {
    if (widget.enabled) widget.onInput(events);
  }

  void _releaseHeld({bool cancelInteraction = false}) {
    if (cancelInteraction && widget.onInteractionCancelled != null) {
      _held.clear();
      _buttons = 0;
      // didUpdateWidget already cancelled the previously enabled view.
      if (widget.enabled) widget.onInteractionCancelled!();
      return;
    }
    if (_held.isNotEmpty) {
      _send([for (final code in _held.values) RdpScancodeInput(code.$1, extended: code.$2, down: false)]);
      _held.clear();
    }
    if (_buttons != 0) {
      _send([for (final button in RdpPointerButton.values) RdpPointerInput(0, 0, button: button, down: false)]);
      _buttons = 0;
    }
    _send([const RdpReleaseAllInput()]);
  }

  KeyEventResult _key(FocusNode node, KeyEvent event) {
    if (!widget.enabled) return KeyEventResult.ignored;
    if (event is KeyUpEvent) {
      if (_pasteKeys.remove(event.physicalKey)) return KeyEventResult.handled;
      final code = _held.remove(event.physicalKey);
      if (code == null) return KeyEventResult.ignored;
      _send([RdpScancodeInput(code.$1, extended: code.$2, down: false)]);
      return KeyEventResult.handled;
    }
    if (event is KeyRepeatEvent && _pasteKeys.contains(event.physicalKey)) return KeyEventResult.handled;
    // IME owns printable keys until composition is committed. Navigation and
    // editing keys must also stay local while the candidate window is open.
    if (_editing.composing.isValid && !_editing.composing.isCollapsed) return KeyEventResult.ignored;
    final keys = HardwareKeyboard.instance;
    final apple = defaultTargetPlatform == TargetPlatform.macOS || defaultTargetPlatform == TargetPlatform.iOS;
    final primary = apple ? keys.isMetaPressed && !keys.isControlPressed : keys.isControlPressed && !keys.isMetaPressed;
    if (widget.localClipboardEnabled &&
        event.logicalKey == LogicalKeyboardKey.keyV &&
        primary &&
        !keys.isShiftPressed &&
        !keys.isAltPressed &&
        (event is KeyDownEvent || event is KeyRepeatEvent)) {
      _pasteKeys.add(event.physicalKey);
      // A second distinct shortcut is also consumed while the first is pending.
      // Releasing here would cancel the acknowledged offer we are awaiting.
      if (_pastePending) return KeyEventResult.handled;
      // The modifier down may already be remote. Release it before awaiting the
      // clipboard; the eventual paste is one guarded native Ctrl+V transaction.
      _releaseHeld();
      if (event is KeyDownEvent && !_pastePending && widget.onPaste != null) {
        _pastePending = true;
        final generation = _inputGeneration;
        unawaited(
          widget.onPaste!(() => mounted && widget.enabled && _focus.hasFocus && generation == _inputGeneration)
              .whenComplete(() => _pastePending = false)
              .catchError((Object _) {}),
        );
      }
      return KeyEventResult.handled;
    }
    // Windows may deliver layout Unicode on WM_CHAR-backed KeyEvent without
    // forwarding it to a custom TextInputClient. Consume it once here, as the
    // terminal does; handled suppresses the engine's duplicate text insertion.
    if (defaultTargetPlatform == TargetPlatform.windows && (event is KeyDownEvent || event is KeyRepeatEvent)) {
      final keys = HardwareKeyboard.instance;
      final altGr = keys.isControlPressed && keys.isLogicalKeyPressed(LogicalKeyboardKey.altRight);
      final text = event.character;
      if (!keys.isMetaPressed &&
          ((!keys.isControlPressed && !keys.isAltPressed) || altGr) &&
          text != null &&
          text.isNotEmpty &&
          text.runes.every((rune) => rune >= 0x20 && rune != 0x7f)) {
        _send([RdpUnicodeInput(text)]);
        return KeyEventResult.handled;
      }
    }
    final code =
        _specialScancode(event.physicalKey) ??
        ((HardwareKeyboard.instance.isControlPressed ||
                HardwareKeyboard.instance.isAltPressed ||
                HardwareKeyboard.instance.isMetaPressed)
            ? _shortcutScancode(event.physicalKey)
            : null);
    if (code == null) return KeyEventResult.ignored;
    _held[event.physicalKey] = code;
    _send([RdpScancodeInput(code.$1, extended: code.$2, down: true)]);
    return KeyEventResult.handled;
  }

  (int, int)? _point(Offset local) {
    final frame = widget.frame;
    final width = frame?.width ?? widget.width, height = frame?.height ?? widget.height;
    final rect = rdpFitRect(_viewport, width, height);
    if (rect.isEmpty || !rect.contains(local)) return null;
    return (
      ((local.dx - rect.left) * width / rect.width).floor().clamp(0, width - 1),
      ((local.dy - rect.top) * height / rect.height).floor().clamp(0, height - 1),
    );
  }

  void _pointer(PointerEvent event, {required bool down, bool moving = false}) {
    if (!widget.enabled) return;
    if (down) _activate();
    final point = _point(event.localPosition);
    if (point == null) {
      if (!down && !moving) _releaseHeld();
      return;
    }
    if (moving) {
      _send([RdpPointerInput(point.$1, point.$2)]);
      return;
    }
    final changed = down ? event.buttons & ~_buttons : _buttons & ~event.buttons;
    _buttons = event.buttons;
    _send([
      for (final entry in [
        (kPrimaryButton, RdpPointerButton.left),
        (kSecondaryButton, RdpPointerButton.right),
        (kMiddleMouseButton, RdpPointerButton.middle),
      ])
        if (changed & entry.$1 != 0) RdpPointerInput(point.$1, point.$2, button: entry.$2, down: down),
    ]);
  }

  @override
  Widget build(BuildContext context) => LayoutBuilder(
    builder: (context, constraints) {
      _viewport = constraints.biggest;
      return Focus(
        focusNode: _focus,
        canRequestFocus: widget.enabled,
        onKeyEvent: _key,
        child: MouseRegion(
          cursor: SystemMouseCursors.basic,
          child: Listener(
            key: const ValueKey('rdp-input-surface'),
            onPointerDown: (event) => _pointer(event, down: true),
            onPointerUp: (event) => _pointer(event, down: false),
            onPointerCancel: (_) => _releaseHeld(cancelInteraction: true),
            onPointerMove: (event) => _pointer(event, down: false, moving: true),
            onPointerHover: (event) => _pointer(event, down: false, moving: true),
            onPointerSignal: (event) {
              if (!widget.enabled || event is! PointerScrollEvent) return;
              final point = _point(event.localPosition);
              if (point == null) return;
              GestureBinding.instance.pointerSignalResolver.register(
                event,
                (_) => _send([
                  RdpWheelInput(
                    point.$1,
                    point.$2,
                    -event.scrollDelta.dy.round().clamp(-32767, 32767),
                    horizontal: -event.scrollDelta.dx.round().clamp(-32767, 32767),
                  ),
                ]),
              );
            },
            child: RepaintBoundary(
              child: CustomPaint(painter: _DesktopPainter(_image), size: Size.infinite),
            ),
          ),
        ),
      );
    },
  );

  @override
  TextEditingValue get currentTextEditingValue => _editing;
  @override
  AutofillScope? get currentAutofillScope => null;
  @override
  void updateEditingValue(TextEditingValue value) {
    if (!widget.enabled || !_focus.hasFocus) return;
    _editing = value;
    if (value.composing.isValid && !value.composing.isCollapsed) return;
    final text = value.text.startsWith('\u200b') ? value.text.substring(1) : value.text;
    if (text.isNotEmpty) {
      _send([RdpUnicodeInput(text)]);
    } else if (value.text.isEmpty) {
      _send([const RdpScancodeInput(0x0e, down: true), const RdpScancodeInput(0x0e, down: false)]);
    }
    _editing = _empty;
    _connection?.setEditingState(_empty);
  }

  @override
  void performAction(TextInputAction action) {
    if (action == TextInputAction.newline || action == TextInputAction.done) {
      _send([const RdpScancodeInput(0x1c, down: true), const RdpScancodeInput(0x1c, down: false)]);
    }
  }

  @override
  void connectionClosed() {
    _connection = null;
    _editing = _empty;
  }

  @override
  void performPrivateCommand(String action, Map<String, dynamic> data) {}
  @override
  void updateFloatingCursor(RawFloatingCursorPoint point) {}
  @override
  void showAutocorrectionPromptRect(int start, int end) {}

  @override
  void dispose() {
    _inputGeneration++;
    _releaseHeld(cancelInteraction: true);
    _focus.removeListener(_focusChanged);
    _connection?.close();
    _focus.dispose();
    _editing = _empty;
    _dropImages();
    super.dispose();
  }
}

class _DesktopPainter extends CustomPainter {
  const _DesktopPainter(this.image);
  final ui.Image? image;
  @override
  void paint(Canvas canvas, Size size) {
    canvas.drawRect(Offset.zero & size, Paint()..color = Colors.black);
    final frame = image;
    if (frame == null) return;
    canvas.drawImageRect(
      frame,
      Rect.fromLTWH(0, 0, frame.width.toDouble(), frame.height.toDouble()),
      rdpFitRect(size, frame.width, frame.height),
      Paint()..filterQuality = FilterQuality.low,
    );
  }

  @override
  bool shouldRepaint(_DesktopPainter oldDelegate) => !identical(image, oldDelegate.image);
}

(int, bool)? _specialScancode(PhysicalKeyboardKey key) => switch (key) {
  PhysicalKeyboardKey.escape => (0x01, false),
  PhysicalKeyboardKey.backspace => (0x0e, false),
  PhysicalKeyboardKey.tab => (0x0f, false),
  PhysicalKeyboardKey.enter => (0x1c, false),
  PhysicalKeyboardKey.numpadEnter => (0x1c, true),
  PhysicalKeyboardKey.controlLeft => (0x1d, false),
  PhysicalKeyboardKey.controlRight => (0x1d, true),
  PhysicalKeyboardKey.shiftLeft => (0x2a, false),
  PhysicalKeyboardKey.shiftRight => (0x36, false),
  PhysicalKeyboardKey.altLeft => (0x38, false),
  PhysicalKeyboardKey.altRight => (0x38, true),
  PhysicalKeyboardKey.metaLeft => (0x5b, true),
  PhysicalKeyboardKey.metaRight => (0x5c, true),
  PhysicalKeyboardKey.arrowUp => (0x48, true),
  PhysicalKeyboardKey.arrowDown => (0x50, true),
  PhysicalKeyboardKey.arrowLeft => (0x4b, true),
  PhysicalKeyboardKey.arrowRight => (0x4d, true),
  PhysicalKeyboardKey.home => (0x47, true),
  PhysicalKeyboardKey.end => (0x4f, true),
  PhysicalKeyboardKey.pageUp => (0x49, true),
  PhysicalKeyboardKey.pageDown => (0x51, true),
  PhysicalKeyboardKey.insert => (0x52, true),
  PhysicalKeyboardKey.delete => (0x53, true),
  PhysicalKeyboardKey.f1 => (0x3b, false),
  PhysicalKeyboardKey.f2 => (0x3c, false),
  PhysicalKeyboardKey.f3 => (0x3d, false),
  PhysicalKeyboardKey.f4 => (0x3e, false),
  PhysicalKeyboardKey.f5 => (0x3f, false),
  PhysicalKeyboardKey.f6 => (0x40, false),
  PhysicalKeyboardKey.f7 => (0x41, false),
  PhysicalKeyboardKey.f8 => (0x42, false),
  PhysicalKeyboardKey.f9 => (0x43, false),
  PhysicalKeyboardKey.f10 => (0x44, false),
  PhysicalKeyboardKey.f11 => (0x57, false),
  PhysicalKeyboardKey.f12 => (0x58, false),
  _ => null,
};

(int, bool)? _shortcutScancode(PhysicalKeyboardKey key) {
  const keys = [
    PhysicalKeyboardKey.keyQ,
    PhysicalKeyboardKey.keyW,
    PhysicalKeyboardKey.keyE,
    PhysicalKeyboardKey.keyR,
    PhysicalKeyboardKey.keyT,
    PhysicalKeyboardKey.keyY,
    PhysicalKeyboardKey.keyU,
    PhysicalKeyboardKey.keyI,
    PhysicalKeyboardKey.keyO,
    PhysicalKeyboardKey.keyP,
  ];
  const home = [
    PhysicalKeyboardKey.keyA,
    PhysicalKeyboardKey.keyS,
    PhysicalKeyboardKey.keyD,
    PhysicalKeyboardKey.keyF,
    PhysicalKeyboardKey.keyG,
    PhysicalKeyboardKey.keyH,
    PhysicalKeyboardKey.keyJ,
    PhysicalKeyboardKey.keyK,
    PhysicalKeyboardKey.keyL,
  ];
  const bottom = [
    PhysicalKeyboardKey.keyZ,
    PhysicalKeyboardKey.keyX,
    PhysicalKeyboardKey.keyC,
    PhysicalKeyboardKey.keyV,
    PhysicalKeyboardKey.keyB,
    PhysicalKeyboardKey.keyN,
    PhysicalKeyboardKey.keyM,
  ];
  final topIndex = keys.indexOf(key), homeIndex = home.indexOf(key), bottomIndex = bottom.indexOf(key);
  if (topIndex >= 0) return (0x10 + topIndex, false);
  if (homeIndex >= 0) return (0x1e + homeIndex, false);
  if (bottomIndex >= 0) return (0x2c + bottomIndex, false);
  return switch (key) {
    PhysicalKeyboardKey.digit1 => (0x02, false),
    PhysicalKeyboardKey.digit2 => (0x03, false),
    PhysicalKeyboardKey.digit3 => (0x04, false),
    PhysicalKeyboardKey.digit4 => (0x05, false),
    PhysicalKeyboardKey.digit5 => (0x06, false),
    PhysicalKeyboardKey.digit6 => (0x07, false),
    PhysicalKeyboardKey.digit7 => (0x08, false),
    PhysicalKeyboardKey.digit8 => (0x09, false),
    PhysicalKeyboardKey.digit9 => (0x0a, false),
    PhysicalKeyboardKey.digit0 => (0x0b, false),
    PhysicalKeyboardKey.space => (0x39, false),
    PhysicalKeyboardKey.minus => (0x0c, false),
    PhysicalKeyboardKey.equal => (0x0d, false),
    PhysicalKeyboardKey.bracketLeft => (0x1a, false),
    PhysicalKeyboardKey.bracketRight => (0x1b, false),
    PhysicalKeyboardKey.semicolon => (0x27, false),
    PhysicalKeyboardKey.quote => (0x28, false),
    PhysicalKeyboardKey.backquote => (0x29, false),
    PhysicalKeyboardKey.backslash => (0x2b, false),
    PhysicalKeyboardKey.comma => (0x33, false),
    PhysicalKeyboardKey.period => (0x34, false),
    PhysicalKeyboardKey.slash => (0x35, false),
    _ => null,
  };
}
