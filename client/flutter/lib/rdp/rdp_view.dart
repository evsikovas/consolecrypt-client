import 'dart:async';
import 'dart:ui' as ui;

import 'package:consolecrypt/app/auto_lock.dart';
import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/rdp_mobile.dart';
import 'package:consolecrypt/rdp/rdp_guard.dart';
import 'package:consolecrypt/rdp/rdp_mobile_overlay.dart';
import 'package:consolecrypt/rdp/rdp_providers.dart';
import 'package:consolecrypt/rdp/rdp_service.dart';
import 'package:consolecrypt/rdp/rdp_touchpad.dart';
import 'package:consolecrypt/rdp/rdp_viewport.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

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
    this.mobileSettings = const RdpMobileSettings(),
    this.onMobileSettingsChanged,
    this.mobileActions = const [],
    this.hostKey,
    this.scope,
    this.mobileStatus,
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
  final RdpMobileSettings mobileSettings;
  final ValueChanged<RdpMobileSettings>? onMobileSettingsChanged;
  final List<RdpMobileAction> mobileActions;
  final String? hostKey;
  final RdpScope? scope;
  final String? mobileStatus;
  @override
  RdpViewState createState() => RdpViewState();
}

class RdpViewState extends State<RdpView> with WidgetsBindingObserver, TextInputClient {
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
  bool get _acceptsInput => mounted && widget.enabled && _appActive;
  final Set<PhysicalKeyboardKey> _pasteKeys = {};
  final Set<PhysicalKeyboardKey> _localKeys = {};
  late final RdpTouchpad _touchpad;
  bool _touchpadEnabled = false;
  bool _keyboardShown = false;
  bool _imeWasVisible = false;
  bool _mobileInitialized = false;
  bool _panelOpen = false;
  bool _touching = false;
  final Set<int> _ignoredPointers = {};
  final Set<int> _touchPointers = {};
  final _mobileViewport = RdpViewport();
  late final RdpMobileKeys _mobileKeys;
  Timer? _panTimer;
  bool get menuOpen => _panelOpen;
  bool get keyboardShown => _keyboardShown;

  void showControls([bool show = true]) {
    if (show) _releaseHeld(cancelInteraction: true);
    setState(() => _panelOpen = show);
  }

  @override
  void initState() {
    super.initState();
    _appActive =
        WidgetsBinding.instance.lifecycleState == null ||
        WidgetsBinding.instance.lifecycleState == AppLifecycleState.resumed;
    WidgetsBinding.instance.addObserver(this);
    _focus.addListener(_focusChanged);
    _touchpad = RdpTouchpad(
      _send,
      onScale: (factor, center) => setState(() => _mobileViewport.magnify(factor, center)),
      onFit: () => setState(_mobileViewport.reset),
      onKeyboard: _toggleKeyboard,
    )..addListener(_cursorChanged);
    _mobileKeys = RdpMobileKeys(_send)..addListener(_keysChanged);
    _offer(widget.frame);
  }

  void _cursorChanged() {
    if (!mounted || !AppPlatform.isMobile || !_touchpadEnabled) return;
    _panTimer?.cancel();
    void step() {
      if (!mounted) return;
      final more = _mobileViewport.follow(
        _touchpad.position,
        fraction: MediaQuery.disableAnimationsOf(context) ? 1 : .35,
      );
      setState(() {});
      if (more) _panTimer = Timer(const Duration(milliseconds: 16), step);
    }

    step();
  }

  void _keysChanged() {
    if (mounted) setState(() {});
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    if (!AppPlatform.isMobile) return;
    if (!_mobileInitialized || MediaQuery.accessibleNavigationOf(context)) {
      _touchpadEnabled = widget.mobileSettings.touchpad(
        phone: MediaQuery.sizeOf(context).shortestSide < 600,
        accessible: MediaQuery.accessibleNavigationOf(context),
        host: widget.hostKey,
      );
      _mobileInitialized = true;
    }
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
        _mobileKeys.cancel();
        _touchPointers.clear();
        _ignoredPointers.clear();
        _touching = false;
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
    _mobileKeys.cancel();
    _touchPointers.clear();
    _ignoredPointers.clear();
    _touching = false;
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
    if (AppPlatform.isMobile) {
      final point = _mobileViewport.toRemote(local);
      return point == null ? null : (point.dx.floor(), point.dy.floor());
    }
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
  Widget build(BuildContext context) {
    if (!AppPlatform.isMobile) return _surface();
    final keyboard = MediaQuery.viewInsetsOf(context).bottom;
    final landscape = MediaQuery.orientationOf(context) == Orientation.landscape;
    return Stack(
      children: [
        Positioned.fill(child: _surface()),
        if (widget.mobileStatus != null)
          Positioned.fill(
            child: IgnorePointer(
              child: Center(
                child: Padding(
                  padding: const EdgeInsets.all(24),
                  child: Text(
                    widget.mobileStatus!,
                    textAlign: TextAlign.center,
                    style: const TextStyle(color: Colors.white, fontSize: 16),
                  ),
                ),
              ),
            ),
          ),
        Positioned.fill(
          child: RdpMobileOverlay(
            actions: _mobileActions(),
            open: _panelOpen,
            onOpenChanged: showControls,
            touching: _touching,
            autoHide: widget.mobileSettings.autoHide,
            bottomInset: keyboard > 0 ? keyboard + 40 : 0,
            position: landscape ? widget.mobileSettings.landscapeButton : widget.mobileSettings.portraitButton,
            onPositionChanged: (value) => widget.onMobileSettingsChanged?.call(
              widget.mobileSettings.copyWith(
                landscapeButton: landscape ? value : null,
                portraitButton: landscape ? null : value,
              ),
            ),
          ),
        ),
        if (_keyboardShown && keyboard > 0)
          Positioned(
            left: 0,
            right: 0,
            bottom: keyboard,
            child: Material(
              key: const ValueKey('rdp-keyboard-strip'),
              color: const Color(0xff17243b),
              child: SizedBox(
                height: 40,
                child: SingleChildScrollView(scrollDirection: Axis.horizontal, child: _specialKeys()),
              ),
            ),
          ),
      ],
    );
  }

  bool _usesTouchpad(PointerEvent event) =>
      AppPlatform.isMobile && _touchpadEnabled && event.kind == PointerDeviceKind.touch;

  Widget _surface() => LayoutBuilder(
    builder: (context, constraints) {
      _viewport = constraints.biggest;
      final width = widget.frame?.width ?? widget.width;
      final height = widget.frame?.height ?? widget.height;
      _mobileViewport.configure(_viewport, width, height);
      final keyboard = MediaQuery.viewInsetsOf(context).bottom;
      _mobileViewport.keyboard(keyboard > 0 ? keyboard + 40 : 0, _touchpad.position);
      _touchpad
        ..speed = widget.mobileSettings.speed
        ..acceleration = widget.mobileSettings.acceleration
        ..configure(
          width,
          height,
          AppPlatform.isMobile ? _mobileViewport.rect.width : rdpFitRect(_viewport, width, height).width,
        );
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
              if (AppPlatform.isMobile && event.kind == PointerDeviceKind.touch) {
                final edge = MediaQuery.systemGestureInsetsOf(context);
                final point = event.localPosition;
                if (point.dx < edge.left ||
                    point.dx > _viewport.width - edge.right ||
                    point.dy < edge.top ||
                    point.dy > _viewport.height - edge.bottom) {
                  _ignoredPointers.add(event.pointer);
                  return;
                }
                _touchPointers.add(event.pointer);
                setState(() => _touching = true);
              }
              if (_usesTouchpad(event)) {
                if (!_acceptsInput) return;
                _activate();
                _touchpad.down(event);
              } else {
                _pointer(event, down: true);
              }
            },
            onPointerUp: (event) {
              if (_ignoredPointers.remove(event.pointer)) return;
              if (_touchPointers.remove(event.pointer) && _touchPointers.isEmpty) setState(() => _touching = false);
              if (_usesTouchpad(event)) {
                if (_acceptsInput) _touchpad.up(event);
              } else {
                _pointer(event, down: false);
              }
            },
            onPointerCancel: (_) => _releaseHeld(cancelInteraction: true),
            onPointerMove: (event) {
              if (_ignoredPointers.contains(event.pointer)) return;
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
                  desktopRect: AppPlatform.isMobile ? _mobileViewport.rect : null,
                  cursorSize: widget.mobileSettings.cursorSize,
                ),
                size: Size.infinite,
              ),
            ),
          ),
        ),
      );
    },
  );

  void hideKeyboard() {
    if (_keyboardShown) _toggleKeyboard();
  }

  void _toggleKeyboard() {
    if (!_acceptsInput) return;
    if (_keyboardShown) {
      _connection?.close();
      _connection = null;
      _editing = _empty;
      setState(() {
        _keyboardShown = false;
        _panelOpen = false;
      });
    } else {
      setState(() {
        _keyboardShown = true;
        _panelOpen = false;
      });
      _activate();
    }
  }

  void _mobileKey(int code, {bool extended = false}) {
    if (!_acceptsInput) return;
    AutoLockScope.recordActivity(context);
    // Do not execute remote Enter while the user is composing a command.
    if (_editing.composing.isValid && !_editing.composing.isCollapsed) return;
    _activate();
    _mobileKeys.key(code, extended: extended);
  }

  bool get _composing => _editing.composing.isValid && !_editing.composing.isCollapsed;

  Widget _specialKeys({bool expanded = false}) {
    Widget key(String title, int code, {bool extended = false, bool modifier = false}) => TextButton(
      key: ValueKey('rdp-key-$title'),
      style: TextButton.styleFrom(
        minimumSize: Size(44, expanded ? 44 : 40),
        padding: const EdgeInsets.symmetric(horizontal: 10),
        foregroundColor: _mobileKeys.active(code) ? const Color(0xff82b4ff) : Colors.white,
        backgroundColor: _mobileKeys.locked(code) ? const Color(0xff334d73) : null,
      ),
      onPressed: !_acceptsInput
          ? null
          : () {
              if (_composing) return;
              if (modifier) {
                _mobileKeys.modifier(code);
              } else {
                _mobileKey(code, extended: extended);
              }
            },
      child: Text(title),
    );
    final keys = <Widget>[
      key('Esc', 0x01),
      key('Tab', 0x0f),
      key('Ctrl', 0x1d, modifier: true),
      key('Alt', 0x38, modifier: true),
      key('Win', 0x5b, modifier: true),
      key('Shift', 0x2a, modifier: true),
      key('←', 0x4b, extended: true),
      key('↑', 0x48, extended: true),
      key('↓', 0x50, extended: true),
      key('→', 0x4d, extended: true),
      key('Enter', 0x1c),
      key('⌫', 0x0e),
      key('Del', 0x53, extended: true),
      if (expanded) ...[
        key('Home', 0x47, extended: true),
        key('End', 0x4f, extended: true),
        key('PgUp', 0x49, extended: true),
        key('PgDn', 0x51, extended: true),
        for (var i = 1; i <= 12; i++) key('F$i', i <= 10 ? 0x3a + i : 0x57 + i - 11),
        key('PrtSc', 0x37, extended: true),
        for (final combo in [
          ('Alt+Tab', 0x38, 0x0f),
          ('Ctrl+C', 0x1d, 0x2e),
          ('Ctrl+V', 0x1d, 0x2f),
          ('Ctrl+Z', 0x1d, 0x2c),
          ('Win+R', 0x5b, 0x13),
          ('Win+E', 0x5b, 0x12),
        ])
          TextButton(
            onPressed: !_acceptsInput
                ? null
                : () {
                    if (_composing) return;
                    _releaseHeld(cancelInteraction: true);
                    _send([
                      RdpScancodeInput(combo.$2, extended: combo.$2 == 0x5b, down: true),
                      RdpScancodeInput(combo.$3, down: true),
                      RdpScancodeInput(combo.$3, down: false),
                      RdpScancodeInput(combo.$2, extended: combo.$2 == 0x5b, down: false),
                    ]);
                  },
            child: Text(combo.$1),
          ),
      ] else
        TextButton(onPressed: _showKeys, child: Text(context.l10n.rdpMobileKeys)),
    ];
    return ExcludeFocus(
      child: expanded ? Wrap(children: keys) : Row(children: keys),
    );
  }

  void _showKeys() {
    showControls(false);
    _releaseHeld(cancelInteraction: true);
    showModalBottomSheet<void>(
      context: context,
      backgroundColor: const Color(0xff17243b),
      builder: (context) => _guardOverlay(
        SafeArea(
          child: ListenableBuilder(
            listenable: _mobileKeys,
            builder: (context, _) => Padding(
              padding: const EdgeInsets.all(8),
              child: SingleChildScrollView(child: _specialKeys(expanded: true)),
            ),
          ),
        ),
      ),
    ).whenComplete(() {
      if (mounted) _releaseHeld(cancelInteraction: true);
    });
  }

  Widget _guardOverlay(Widget child) =>
      widget.scope == null ? child : RdpScopeGuard(scope: widget.scope!, child: child);

  List<RdpMobileAction> _mobileActions() {
    final l = context.l10n;
    return [
      RdpMobileAction(
        _keyboardShown ? l.rdpMobileHideKeyboard : l.rdpMobileShowKeyboard,
        Icons.keyboard_outlined,
        _acceptsInput ? _toggleKeyboard : null,
        key: const ValueKey('rdp-mobile-keyboard'),
      ),
      RdpMobileAction(
        _touchpadEnabled ? l.rdpMobileTouchpad : l.rdpMobileDirectTouch,
        Icons.mouse_outlined,
        !_acceptsInput
            ? null
            : () {
                _releaseHeld(cancelInteraction: true);
                setState(() => _touchpadEnabled = !_touchpadEnabled);
                final host = widget.hostKey;
                if (host != null) {
                  widget.onMobileSettingsChanged?.call(
                    widget.mobileSettings.copyWith(
                      hostModes: {
                        host: _touchpadEnabled,
                        for (final entry in widget.mobileSettings.hostModes.entries)
                          if (entry.key != host) entry.key: entry.value,
                      },
                    ),
                  );
                }
              },
        selected: _touchpadEnabled,
        key: const ValueKey('rdp-touchpad-toggle'),
      ),
      RdpMobileAction(l.rdpMobileKeys, Icons.keyboard_command_key, _showKeys, key: const ValueKey('rdp-mobile-keys')),
      RdpMobileAction(l.rdpMobileFit, Icons.fit_screen, () => setState(_mobileViewport.reset)),
      RdpMobileAction(
        l.rdpMobileLeftClick,
        Icons.ads_click,
        _acceptsInput ? () => _touchpad.click(RdpPointerButton.left) : null,
        key: const ValueKey('rdp-mobile-left-click'),
      ),
      RdpMobileAction(
        l.rdpMobileRightClick,
        Icons.more_horiz,
        _acceptsInput ? () => _touchpad.click(RdpPointerButton.right) : null,
        key: const ValueKey('rdp-mobile-right-click'),
      ),
      ...widget.mobileActions,
      RdpMobileAction(l.rdpMobileHelpTitle, Icons.help_outline, () {
        showControls(false);
        _releaseHeld(cancelInteraction: true);
        showDialog<void>(
          context: context,
          builder: (context) => _guardOverlay(
            AlertDialog(
              title: Text(l.rdpMobileHelpTitle),
              content: SingleChildScrollView(child: Text(l.rdpMobileHelp)),
              actions: [TextButton(onPressed: () => Navigator.pop(context), child: const Text('OK'))],
            ),
          ),
        );
      }, key: const ValueKey('rdp-mobile-help')),
    ];
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
      final shortcut = _mobileKeys.hasShortcutModifier && text.length == 1 ? _textScancode(text) : null;
      if (shortcut != null) {
        _mobileKeys.key(shortcut);
      } else {
        _send([RdpUnicodeInput(text)]);
        _mobileKeys.consume();
      }
    } else if (value.text.isEmpty) {
      _mobileKeys.key(0x0e);
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
      _mobileKeys.key(0x1c);
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
    _panTimer?.cancel();
    _mobileKeys.removeListener(_keysChanged);
    _mobileKeys.dispose();
    _connection?.close();
    _focus.dispose();
    _editing = _empty;
    _dropImages();
    super.dispose();
  }
}

class _DesktopPainter extends CustomPainter {
  const _DesktopPainter(
    this.image, {
    this.pointer,
    required this.remoteWidth,
    required this.remoteHeight,
    this.desktopRect,
    this.cursorSize = 24,
  });
  final ui.Image? image;
  final Offset? pointer;
  final int remoteWidth, remoteHeight;
  final Rect? desktopRect;
  final double cursorSize;
  @override
  void paint(Canvas canvas, Size size) {
    canvas.drawRect(Offset.zero & size, Paint()..color = Colors.black);
    canvas.save();
    canvas.clipRect(Offset.zero & size);
    final frame = image;
    if (frame != null) {
      canvas.drawImageRect(
        frame,
        Rect.fromLTWH(0, 0, frame.width.toDouble(), frame.height.toDouble()),
        desktopRect ?? rdpFitRect(size, frame.width, frame.height),
        Paint()..filterQuality = FilterQuality.low,
      );
    }
    final point = pointer;
    if (point != null) {
      final rect = desktopRect ?? rdpFitRect(size, remoteWidth, remoteHeight);
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
      canvas.scale(cursorSize / 25);
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
    canvas.restore();
  }

  @override
  bool shouldRepaint(_DesktopPainter oldDelegate) =>
      !identical(image, oldDelegate.image) ||
      pointer != oldDelegate.pointer ||
      desktopRect != oldDelegate.desktopRect ||
      cursorSize != oldDelegate.cursorSize ||
      remoteWidth != oldDelegate.remoteWidth ||
      remoteHeight != oldDelegate.remoteHeight;
}

int? _textScancode(String text) {
  for (final row in [('1234567890', 0x02), ('qwertyuiop', 0x10), ('asdfghjkl', 0x1e), ('zxcvbnm', 0x2c)]) {
    final index = row.$1.indexOf(text.toLowerCase());
    if (index >= 0) return row.$2 + index;
  }
  return text == ' ' ? 0x39 : null;
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
