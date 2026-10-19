import 'dart:async';

import 'package:consolecrypt/rdp/rdp_service.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/gestures.dart';

/// Converts phone gestures into ordinary RDP mouse input. The remote position
/// survives lifting/repositioning a finger; touching never warps it to a finger.
/// All gesture state belongs to one view/session, not to the shared workspace.
class RdpTouchpad extends ChangeNotifier {
  RdpTouchpad(this.onInput);
  final ValueChanged<List<RdpInput>> onInput;
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
  Offset _wheel = Offset.zero;

  /// Use the rendered desktop's scale, preserving sub-pixel motion. Coordinates
  /// are clamped before rounding so even the last row/column remains in bounds.
  void configure(int width, int height, double displayWidth) {
    _width = width;
    _height = height;
    _scale = displayWidth > 0 ? width / displayWidth : 1;
    if (!_initialized) {
      _position = Offset(width / 2, height / 2);
      _initialized = true;
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
  }

  Offset get _centroid => _pointers.values.reduce((a, b) => a + b) / _pointers.length.toDouble();

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
      _position = _clamp(_position + (event.localPosition - previous) * _scale);
      onInput([_pointer()]);
      notifyListeners();
    } else if (_peak == 2 && _pointers.length == 2 && _moved) {
      // Two fingers scroll at the cursor, never click or relocate it. Using the
      // centroid avoids doubling the wheel when both fingers deliver events.
      _wheel += (_centroid - before) * -3;
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
        click(RdpPointerButton.right);
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
    _pointers.clear();
    _origins.clear();
    _dragging = false;
    _peak = 0;
    _lastTap = null;
    _lastTapLocation = null;
    _wheel = Offset.zero;
  }

  @override
  void dispose() {
    cancel();
    super.dispose();
  }
}
