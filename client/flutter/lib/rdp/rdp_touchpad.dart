import 'dart:async';

import 'package:consolecrypt/rdp/rdp_service.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/gestures.dart';

/// Converts phone gestures into ordinary RDP mouse input. The remote position
/// survives lifting/repositioning a finger; touching never warps it to a finger.
/// All gesture state belongs to one view/session, not to the shared workspace.
class RdpTouchpad extends ChangeNotifier {
  RdpTouchpad(this.onInput, {this.onScale, this.onFit, this.onKeyboard});
  final ValueChanged<List<RdpInput>> onInput;
  final void Function(double factor, Offset center)? onScale;
  final VoidCallback? onFit;
  final VoidCallback? onKeyboard;
  double speed = 1;
  bool acceleration = true;
  final Map<int, Offset> _pointers = {};
  final Map<int, Offset> _origins = {};
  Offset _position = Offset.zero;
  Offset get position => _position;
  int _width = 1, _height = 1;
  double _scale = 1;
  bool _initialized = false, _moved = false, _dragging = false;
  int _peak = 0;
  Duration _started = Duration.zero;
  Duration? _lastTap;
  Offset? _lastTapLocation;
  Timer? _hold;
  Timer? _rightTap;
  Duration? _lastTwoTap;
  Offset? _lastTwoLocation;
  Duration? _lastMove;
  Offset _twoCenter = Offset.zero;
  double _twoSpan = 0;
  double _previousSpan = 0;
  _TwoFingerGesture? _twoGesture;
  Offset _wheel = Offset.zero;

  /// Use the rendered desktop's scale, preserving sub-pixel motion. Coordinates
  /// are clamped before rounding so even the last row/column remains in bounds.
  void configure(int width, int height, double displayWidth) {
    final previous = (_width, _height);
    _width = width;
    _height = height;
    _scale = displayWidth > 0 ? width / displayWidth : 1;
    if (!_initialized) {
      _position = Offset(width / 2, height / 2);
      _initialized = true;
    } else if (previous != (width, height) && previous.$1 > 0 && previous.$2 > 0) {
      // A resized desktop (rotation, fit) keeps the cursor at the same
      // relative spot instead of pinning it to the nearest edge.
      _position = Offset(_position.dx * width / previous.$1, _position.dy * height / previous.$2);
    }
    _position = _clamp(_position);
  }

  Offset _clamp(Offset point) =>
      Offset(point.dx.clamp(0, _width - 1).toDouble(), point.dy.clamp(0, _height - 1).toDouble());

  RdpPointerInput _pointer({RdpPointerButton? button, bool down = false}) =>
      RdpPointerInput(_position.dx.round(), _position.dy.round(), button: button, down: down);

  void syncPosition(int x, int y) {
    final next = _clamp(Offset(x.toDouble(), y.toDouble()));
    if (next == _position) return;
    _position = next;
    notifyListeners();
  }

  void click(RdpPointerButton button) {
    onInput([_pointer(button: button, down: true), _pointer(button: button)]);
  }

  void down(PointerDownEvent event) {
    if (_pointers.isEmpty) {
      _started = event.timeStamp;
      _peak = 1;
      _moved = false;
      _wheel = Offset.zero;
      _lastMove = event.timeStamp;
      _twoGesture = null;
      final lastTap = _lastTap;
      final doubleTap =
          lastTap != null &&
          event.timeStamp >= lastTap &&
          event.timeStamp - lastTap <= const Duration(milliseconds: 300) &&
          (event.localPosition - _lastTapLocation!).distance <= 32;
      _lastTap = null;
      if (doubleTap) {
        // The second down completes a double-click, or starts a held drag.
        _startDrag();
      } else {
        _hold = Timer(const Duration(milliseconds: 500), () {
          if (_pointers.length == 1 && !_moved && _peak == 1) _startDrag();
        });
      }
    } else {
      _hold?.cancel();
      _endDrag();
    }
    _pointers[event.pointer] = event.localPosition;
    _origins[event.pointer] = event.localPosition;
    if (_pointers.length > _peak) _peak = _pointers.length;
    if (_pointers.length == 2) {
      _twoCenter = _centroid;
      _twoSpan = _span;
      _previousSpan = _twoSpan;
    }
    if (_peak >= 3) {
      _rightTap?.cancel();
      _lastTwoTap = null;
    }
  }

  Offset get _centroid => _pointers.values.reduce((a, b) => a + b) / _pointers.length.toDouble();
  double get _span => (_pointers.values.first - _pointers.values.last).distance;

  /// Continuous bounded gain, in logical pixels per millisecond. Slow motion
  /// remains precise; fast swipes cover a large desktop without finger travel.
  static double gain(double velocity) => 1 + ((velocity - .15) / 1.35).clamp(0, 1) * 1.5;

  void move(PointerMoveEvent event) {
    final previous = _pointers[event.pointer];
    if (previous == null) return;
    final before = _centroid;
    _pointers[event.pointer] = event.localPosition;
    if ((event.localPosition - _origins[event.pointer]!).distance > 8) {
      _moved = true;
      _hold?.cancel();
    }
    if (_peak == 1) {
      final delta = event.localPosition - previous;
      final elapsed = (event.timeStamp - (_lastMove ?? event.timeStamp)).inMicroseconds / 1000;
      _lastMove = event.timeStamp;
      final multiplier = acceleration && elapsed > 0 ? gain(delta.distance / elapsed) : 1.0;
      _position = _clamp(_position + delta * _scale * speed.clamp(.5, 3) * multiplier);
      onInput([_pointer()]);
      notifyListeners();
    } else if (_peak == 2 && _pointers.length == 2) {
      final span = _span;
      final translation = (_centroid - _twoCenter).distance;
      final separation = (span - _twoSpan).abs();
      var firstDecision = false;
      if (_twoGesture == null && (translation >= 12 || separation >= 12)) {
        _twoGesture = separation > translation ? _TwoFingerGesture.zoom : _TwoFingerGesture.scroll;
        _moved = true;
        firstDecision = true;
        _rightTap?.cancel();
        _lastTwoTap = null;
      }
      if (_twoGesture == _TwoFingerGesture.zoom) {
        final previousSpan = firstDecision ? _twoSpan : _previousSpan;
        if (previousSpan > 0) onScale?.call(span / previousSpan, _centroid);
      }
      _previousSpan = span;
      if (_twoGesture != _TwoFingerGesture.scroll) return;
      // Two fingers scroll at the cursor, never click or relocate it. Using the
      // centroid avoids doubling the wheel when both fingers deliver events.
      _wheel += (_centroid - (firstDecision ? _twoCenter : before)) * -3;
      final vertical = _wheel.dy.truncate(), horizontal = _wheel.dx.truncate();
      _wheel -= Offset(horizontal.toDouble(), vertical.toDouble());
      if (vertical != 0 || horizontal != 0) {
        onInput([RdpWheelInput(_position.dx.round(), _position.dy.round(), vertical, horizontal: horizontal)]);
      }
    }
  }

  void up(PointerUpEvent event) {
    if (!_pointers.containsKey(event.pointer)) return;
    final origin = _origins[event.pointer]!;
    _pointers.remove(event.pointer);
    _origins.remove(event.pointer);
    _hold?.cancel();
    if (_pointers.isNotEmpty) return;
    if (_dragging) {
      _endDrag();
    } else if (!_moved && event.timeStamp - _started <= const Duration(milliseconds: 500)) {
      if (_peak == 1) {
        click(RdpPointerButton.left);
        _lastTap = event.timeStamp;
        _lastTapLocation = origin;
      } else if (_peak == 2) {
        final last = _lastTwoTap;
        if (last != null &&
            event.timeStamp - last <= const Duration(milliseconds: 300) &&
            (_twoCenter - _lastTwoLocation!).distance <= 32) {
          _rightTap?.cancel();
          _lastTwoTap = null;
          onFit?.call();
        } else {
          _rightTap?.cancel();
          _lastTwoTap = event.timeStamp;
          _lastTwoLocation = _twoCenter;
          // Delay the right click so the fit gesture never opens a context menu.
          final down = _pointer(button: RdpPointerButton.right, down: true);
          final up = _pointer(button: RdpPointerButton.right);
          _rightTap = Timer(const Duration(milliseconds: 300), () => onInput([down, up]));
        }
      } else if (_peak == 3) {
        onKeyboard?.call();
      }
    }
    _peak = 0;
  }

  void _startDrag() {
    _dragging = true;
    onInput([_pointer(button: RdpPointerButton.left, down: true)]);
  }

  void _endDrag() {
    if (!_dragging) return;
    _dragging = false;
    onInput([_pointer(button: RdpPointerButton.left)]);
  }

  /// The owner sends native ReleaseAll when cancelling. Suppressing an extra
  /// asynchronous button-up here prevents it leaking into a replacement tab.
  void cancel() {
    _hold?.cancel();
    _hold = null;
    _rightTap?.cancel();
    _rightTap = null;
    _pointers.clear();
    _origins.clear();
    _dragging = false;
    _peak = 0;
    _lastTap = null;
    _lastTapLocation = null;
    _lastTwoTap = null;
    _lastTwoLocation = null;
    _lastMove = null;
    _twoGesture = null;
    _wheel = Offset.zero;
  }

  @override
  void dispose() {
    cancel();
    super.dispose();
  }
}

enum _TwoFingerGesture { scroll, zoom }
