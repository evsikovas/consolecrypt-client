import 'dart:async';
import 'dart:ui' as ui;

import 'package:consolecrypt/app/auto_lock.dart';
import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/app/theme/tokens.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/rdp/rdp_service.dart';
import 'package:consolecrypt/rdp/rdp_touchpad.dart';
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
    this.onShowConnectionBar,
  });
  final RdpFrame? frame;
  final int width, height;
  final bool enabled;
  final ValueChanged<List<RdpInput>> onInput;

  /// Immediate native cancellation, independent of the ordinary input FIFO.
  final VoidCallback? onInteractionCancelled;

  /// Local escape hatch in fullscreen; plain Escape still reaches the server.
  final VoidCallback? onShowConnectionBar;
  final bool localClipboardEnabled;
  final Future<void> Function(bool Function() isInputCurrent)? onPaste;
  @override
  State<RdpView> createState() => _RdpViewState();
}

class _RdpViewState extends State<RdpView> with WidgetsBindingObserver, TextInputClient {
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
  bool _appActive = true;
  bool get _acceptsInput => widget.enabled && _appActive;
  final Set<PhysicalKeyboardKey> _pasteKeys = {};
  final Set<PhysicalKeyboardKey> _localKeys = {};
  late final RdpTouchpad _touchpad;
  bool _touchpadEnabled = false;
  bool _keyboardShown = false;
  bool _imeWasVisible = false;

  @override
  void initState() {
    super.initState();
    _appActive =
        WidgetsBinding.instance.lifecycleState == null ||
        WidgetsBinding.instance.lifecycleState == AppLifecycleState.resumed;
    WidgetsBinding.instance.addObserver(this);
    _focus.addListener(_focusChanged);
    _touchpad = RdpTouchpad(_send)..addListener(_cursorChanged);
    _offer(widget.frame);
  }

  void _cursorChanged() {
    if (mounted && AppPlatform.isMobile && _touchpadEnabled) setState(() {});
  }

  @override
  void didChangeMetrics() {
    if (!AppPlatform.isMobile) return;
    // Android Back can dismiss the IME without closing its text connection.
    // Do not mistake the initial opening animation for a dismissal.
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted) return;
      final visible = View.of(context).viewInsets.bottom > 0;
      if (_imeWasVisible && !visible && _keyboardShown) {
        _connection?.close();
        _connection = null;
        _editing = _empty;
        setState(() => _keyboardShown = false);
      }
      _imeWasVisible = visible;
    });
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    final active = state == AppLifecycleState.resumed;
    if (_appActive == active) return;
    _inputGeneration++;
    if (!active) {
      // OS Alt+Tab/minimize can retain Flutter's primary FocusNode. Cancel a
      // pending clipboard ACK and held input before deactivating dispatch.
      _pasteKeys.clear();
      _localKeys.clear();
      _releaseHeld(cancelInteraction: true);
      _connection?.close();
      _connection = null;
      _editing = _empty;
      if (AppPlatform.isMobile) setState(() => _keyboardShown = false);
    }
    setState(() => _appActive = active);
    if (active && _focus.hasFocus) _openInput();
  }

  @override
  void didUpdateWidget(RdpView oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.localClipboardEnabled != widget.localClipboardEnabled || oldWidget.enabled != widget.enabled) {
      _inputGeneration++;
    }
    if (!widget.enabled) {
      if (oldWidget.enabled) {
        _touchpad.cancel();
        _keyboardShown = false;
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
    if (!_acceptsInput) return;
    _focus.requestFocus();
    _openInput();
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted && _acceptsInput && _focus.hasFocus) _openInput();
    });
  }

  void _focusChanged() {
    if (_focus.hasFocus && _acceptsInput) {
      _openInput();
    } else {
      _inputGeneration++;
      _pasteKeys.clear();
      _localKeys.clear();
      _releaseHeld(cancelInteraction: true);
      _connection?.close();
      _connection = null;
      _editing = _empty;
      if (AppPlatform.isMobile && _keyboardShown && mounted) setState(() => _keyboardShown = false);
    }
  }

  void _openInput() {
    if (!_acceptsInput || !_focus.hasFocus) return;
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
    if (!AppPlatform.isMobile || _keyboardShown) _connection!.show();
  }

  void _send(List<RdpInput> events) {
    if (_acceptsInput) widget.onInput(events);
  }

  void _releaseHeld({bool cancelInteraction = false}) {
    _touchpad.cancel();
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
    if (!_acceptsInput) return KeyEventResult.ignored;
    if (event is KeyUpEvent) {
      if (_localKeys.remove(event.physicalKey)) return KeyEventResult.handled;
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
    if (widget.onShowConnectionBar != null &&
        event.logicalKey == LogicalKeyboardKey.home &&
        keys.isControlPressed &&
        keys.isAltPressed &&
        !keys.isMetaPressed &&
        !keys.isShiftPressed) {
      _localKeys.add(event.physicalKey);
      _releaseHeld(cancelInteraction: true);
      widget.onShowConnectionBar!();
      return KeyEventResult.handled;
    }
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
          widget.onPaste!(() => mounted && _acceptsInput && _focus.hasFocus && generation == _inputGeneration)
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
    if (!_acceptsInput) return;
    if (down) _activate();
    final point = _point(event.localPosition);
    if (point == null) {
      if (!down && !moving) _releaseHeld();
      return;
    }
    if (moving) {
      _touchpad.syncPosition(point.$1, point.$2);
      _send([RdpPointerInput(point.$1, point.$2)]);
      return;
    }
    final changed = down ? event.buttons & ~_buttons : _buttons & ~event.buttons;
    _buttons = event.buttons;
    _touchpad.syncPosition(point.$1, point.$2);
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
  Widget build(BuildContext context) => AppPlatform.isMobile
      ? LayoutBuilder(
          builder: (context, constraints) => Column(
            children: [
              Expanded(child: _surface()),
              _mobileControls(constraints.maxHeight < 180),
            ],
          ),
        )
      : _surface();

  bool _usesTouchpad(PointerEvent event) =>
      AppPlatform.isMobile && _touchpadEnabled && event.kind == PointerDeviceKind.touch;

  Widget _surface() => LayoutBuilder(
    builder: (context, constraints) {
      _viewport = constraints.biggest;
      final width = widget.frame?.width ?? widget.width;
      final height = widget.frame?.height ?? widget.height;
      _touchpad.configure(width, height, rdpFitRect(_viewport, width, height).width);
      return Focus(
        focusNode: _focus,
        canRequestFocus: widget.enabled,
        onKeyEvent: _key,
        child: MouseRegion(
          cursor: SystemMouseCursors.basic,
          child: Listener(
            key: const ValueKey('rdp-input-surface'),
            behavior: HitTestBehavior.opaque,
            onPointerDown: (event) {
              if (_usesTouchpad(event)) {
                if (!_acceptsInput) return;
                _activate();
                _touchpad.down(event);
              } else {
                _pointer(event, down: true);
              }
            },
            onPointerUp: (event) {
              if (_usesTouchpad(event)) {
                if (_acceptsInput) _touchpad.up(event);
              } else {
                _pointer(event, down: false);
              }
            },
            onPointerCancel: (_) => _releaseHeld(cancelInteraction: true),
            onPointerMove: (event) {
              if (_usesTouchpad(event)) {
                if (_acceptsInput) _touchpad.move(event);
              } else {
                _pointer(event, down: false, moving: true);
              }
            },
            onPointerHover: (event) => _pointer(event, down: false, moving: true),
            onPointerSignal: (event) {
              if (!_acceptsInput || event is! PointerScrollEvent) return;
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
              child: CustomPaint(
                painter: _DesktopPainter(
                  _image,
                  pointer: AppPlatform.isMobile && _touchpadEnabled && widget.enabled ? _touchpad.position : null,
                  remoteWidth: width,
                  remoteHeight: height,
                ),
                size: Size.infinite,
              ),
            ),
          ),
        ),
      );
    },
  );

  void _toggleKeyboard() {
    if (!_acceptsInput) return;
    if (_keyboardShown) {
      _connection?.close();
      _connection = null;
      _editing = _empty;
      setState(() => _keyboardShown = false);
    } else {
      setState(() => _keyboardShown = true);
      _activate();
    }
  }

  void _mobileKey(int code, {bool extended = false}) {
    if (!_acceptsInput) return;
    AutoLockScope.recordActivity(context);
    // Do not execute remote Enter while the user is composing a command.
    if (_editing.composing.isValid && !_editing.composing.isCollapsed) return;
    _activate();
    _send([
      RdpScancodeInput(code, extended: extended, down: true),
      RdpScancodeInput(code, extended: extended, down: false),
    ]);
  }

  Widget _specialKeys() => Row(
    children: [
      for (final entry in [
        ('Esc', 0x01, false),
        ('Tab', 0x0f, false),
        ('←', 0x4b, true),
        ('↑', 0x48, true),
        ('↓', 0x50, true),
        ('→', 0x4d, true),
        ('Enter', 0x1c, false),
        ('⌫', 0x0e, false),
      ])
        TextButton(
          onPressed: _acceptsInput ? () => _mobileKey(entry.$2, extended: entry.$3) : null,
          child: Text(entry.$1),
        ),
    ],
  );

  Widget _mobileControls(bool compact) {
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    final scheme = ColorScheme.fromSeed(
      seedColor: tokens.palette.accent,
      brightness: tokens.brightness,
    ).copyWith(surface: tokens.surfaces.contentSolid, onSurface: tokens.palette.label);
    final iconColor = WidgetStateProperty.resolveWith<Color>((states) {
      if (states.contains(WidgetState.disabled)) return tokens.palette.label.withValues(alpha: 0.38);
      if (states.contains(WidgetState.selected)) return tokens.palette.accent;
      return tokens.palette.label;
    });
    final controls = ExcludeFocus(
      child: Material(
        key: const ValueKey('rdp-mobile-controls'),
        color: tokens.surfaces.contentSolid,
        child: SafeArea(
          top: false,
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Row(
                children: [
                  IconButton(
                    key: const ValueKey('rdp-touchpad-toggle'),
                    tooltip: l.rdpMobileTouchpad,
                    isSelected: _touchpadEnabled,
                    icon: const Icon(Icons.mouse_outlined),
                    selectedIcon: const Icon(Icons.mouse),
                    onPressed: !_acceptsInput
                        ? null
                        : () {
                            _releaseHeld(cancelInteraction: true);
                            setState(() => _touchpadEnabled = !_touchpadEnabled);
                          },
                  ),
                  Expanded(
                    child: compact && _keyboardShown
                        ? SingleChildScrollView(scrollDirection: Axis.horizontal, child: _specialKeys())
                        : Text(
                            _touchpadEnabled ? l.rdpMobileTouchpad : l.rdpMobileDirectTouch,
                            key: const ValueKey('rdp-mobile-mode-label'),
                            style: TextStyle(color: tokens.palette.label),
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                          ),
                  ),
                  IconButton(
                    key: const ValueKey('rdp-mobile-keyboard'),
                    tooltip: _keyboardShown ? l.rdpMobileHideKeyboard : l.rdpMobileShowKeyboard,
                    isSelected: _keyboardShown,
                    icon: Icon(_keyboardShown ? Icons.keyboard_hide : Icons.keyboard_outlined),
                    onPressed: _acceptsInput ? _toggleKeyboard : null,
                  ),
                  if (_touchpadEnabled) ...[
                    IconButton(
                      key: const ValueKey('rdp-mobile-left-click'),
                      tooltip: l.rdpMobileLeftClick,
                      icon: const Icon(Icons.ads_click),
                      onPressed: _acceptsInput ? () => _touchpad.click(RdpPointerButton.left) : null,
                    ),
                    IconButton(
                      key: const ValueKey('rdp-mobile-right-click'),
                      tooltip: l.rdpMobileRightClick,
                      icon: const Icon(Icons.more_horiz),
                      onPressed: _acceptsInput ? () => _touchpad.click(RdpPointerButton.right) : null,
                    ),
                  ],
                  IconButton(
                    key: const ValueKey('rdp-mobile-help'),
                    tooltip: l.rdpMobileHelpTitle,
                    icon: const Icon(Icons.help_outline),
                    onPressed: () {
                      _releaseHeld(cancelInteraction: true);
                      showDialog<void>(
                        context: context,
                        builder: (context) => AlertDialog(
                          title: Text(l.rdpMobileHelpTitle),
                          content: SingleChildScrollView(child: Text(l.rdpMobileHelp)),
                          actions: [TextButton(onPressed: () => Navigator.pop(context), child: const Text('OK'))],
                        ),
                      );
                    },
                  ),
                ],
              ),
              if (_keyboardShown && !compact)
                SizedBox(
                  height: 44,
                  child: SingleChildScrollView(scrollDirection: Axis.horizontal, child: _specialKeys()),
                ),
            ],
          ),
        ),
      ),
    );
    return Theme(
      // The app uses material_ui; Flutter's ambient icon theme can still be
      // light even when GlassTokens are dark. Set the button states explicitly.
      data: Theme.of(context).copyWith(
        colorScheme: scheme,
        iconButtonTheme: IconButtonThemeData(
          style: ButtonStyle(foregroundColor: iconColor, iconColor: iconColor),
        ),
      ),
      child: DefaultTextStyle.merge(
        style: TextStyle(color: tokens.palette.label),
        child: controls,
      ),
    );
  }

  @override
  TextEditingValue get currentTextEditingValue => _editing;
  @override
  AutofillScope? get currentAutofillScope => null;
  @override
  void updateEditingValue(TextEditingValue value) {
    if (!_acceptsInput || !_focus.hasFocus) return;
    AutoLockScope.recordActivity(context);
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
    if (!_acceptsInput || !_focus.hasFocus) return;
    AutoLockScope.recordActivity(context);
    if (_editing.composing.isValid && !_editing.composing.isCollapsed) return;
    if (action == TextInputAction.newline || action == TextInputAction.done) {
      _send([const RdpScancodeInput(0x1c, down: true), const RdpScancodeInput(0x1c, down: false)]);
    }
  }

  @override
  void connectionClosed() {
    _inputGeneration++;
    _releaseHeld(cancelInteraction: true);
    _connection = null;
    _editing = _empty;
    if (AppPlatform.isMobile && mounted) setState(() => _keyboardShown = false);
  }

  @override
  void performPrivateCommand(String action, Map<String, dynamic> data) {}
  @override
  void updateFloatingCursor(RawFloatingCursorPoint point) {}
  @override
  void showAutocorrectionPromptRect(int start, int end) {}

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _inputGeneration++;
    _releaseHeld(cancelInteraction: true);
    _focus.removeListener(_focusChanged);
    _touchpad.removeListener(_cursorChanged);
    _touchpad.dispose();
    _connection?.close();
    _focus.dispose();
    _editing = _empty;
    _dropImages();
    super.dispose();
  }
}

class _DesktopPainter extends CustomPainter {
  const _DesktopPainter(this.image, {this.pointer, required this.remoteWidth, required this.remoteHeight});
  final ui.Image? image;
  final Offset? pointer;
  final int remoteWidth, remoteHeight;
  @override
  void paint(Canvas canvas, Size size) {
    canvas.drawRect(Offset.zero & size, Paint()..color = Colors.black);
    final frame = image;
    if (frame != null) {
      canvas.drawImageRect(
        frame,
        Rect.fromLTWH(0, 0, frame.width.toDouble(), frame.height.toDouble()),
        rdpFitRect(size, frame.width, frame.height),
        Paint()..filterQuality = FilterQuality.low,
      );
    }
    final point = pointer;
    if (point != null) {
      final rect = rdpFitRect(size, remoteWidth, remoteHeight);
      final tip = Offset(
        rect.left + point.dx * rect.width / remoteWidth,
        rect.top + point.dy * rect.height / remoteHeight,
      );
      // A local, constant-size cursor stays visible on a phone; the native core
      // deliberately disables unbounded server pointer-cache rendering.
      final arrow = Path()
        ..moveTo(0, 0)
        ..lineTo(0, 21)
        ..lineTo(5, 16)
        ..lineTo(9, 25)
        ..lineTo(13, 23)
        ..lineTo(9, 14)
        ..lineTo(16, 14)
        ..close();
      canvas.save();
      canvas.clipRect(rect);
      canvas.translate(tip.dx, tip.dy);
      canvas.drawPath(
        arrow,
        Paint()
          ..color = Colors.black
          ..style = PaintingStyle.stroke
          ..strokeWidth = 3
          ..strokeJoin = StrokeJoin.round,
      );
      canvas.drawPath(arrow, Paint()..color = Colors.white);
      canvas.restore();
    }
  }

  @override
  bool shouldRepaint(_DesktopPainter oldDelegate) =>
      !identical(image, oldDelegate.image) ||
      pointer != oldDelegate.pointer ||
      remoteWidth != oldDelegate.remoteWidth ||
      remoteHeight != oldDelegate.remoteHeight;
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
