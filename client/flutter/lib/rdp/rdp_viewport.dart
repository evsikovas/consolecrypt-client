import 'dart:math' as math;

import 'package:flutter/painting.dart';

/// One transform for rendering, input and the local pointer. Keyboard insets
/// restrict visibility, never the size of the remote desktop surface.
class RdpViewport {
  Size size = Size.zero;
  Size remote = const Size(1, 1);
  double zoom = 1;
  Offset offset = Offset.zero;
  double keyboardHeight = 0;
  (double, Offset)? _beforeKeyboard;

  Rect get fit {
    final fitted = applyBoxFit(BoxFit.contain, remote, size).destination;
    return Alignment.center.inscribe(fitted, Offset.zero & size);
  }

  Rect get rect => Rect.fromLTWH(fit.left + offset.dx, fit.top + offset.dy, fit.width * zoom, fit.height * zoom);

  Rect get visible => Rect.fromLTWH(0, 0, size.width, math.max(1, size.height - keyboardHeight));
  double get scale => rect.width / remote.width;

  void configure(Size nextSize, int width, int height) {
    if (nextSize == size && remote == Size(width.toDouble(), height.toDouble())) return;
    final center = size.isEmpty ? const Offset(.5, .5) : _fraction(visible.center);
    size = nextSize;
    remote = Size(width.toDouble(), height.toDouble());
    _anchor(center, visible.center);
    _clamp();
  }

  Offset _fraction(Offset local) => rect.isEmpty
      ? const Offset(.5, .5)
      : Offset((local.dx - rect.left) / rect.width, (local.dy - rect.top) / rect.height);

  Offset toLocal(Offset point) => rect.topLeft + point * scale;

  Offset? toRemote(Offset local) {
    if (rect.isEmpty || !rect.contains(local)) return null;
    final point = (local - rect.topLeft) / scale;
    return Offset(point.dx.clamp(0, remote.width - 1), point.dy.clamp(0, remote.height - 1));
  }

  void _anchor(Offset fraction, Offset local) {
    offset = local - fit.topLeft - Offset(fraction.dx * fit.width * zoom, fraction.dy * fit.height * zoom);
  }

  void magnify(double factor, Offset focalPoint) {
    if (!factor.isFinite || factor <= 0 || size.isEmpty) return;
    final anchor = _fraction(focalPoint);
    zoom = (zoom * factor).clamp(1, 4);
    _anchor(anchor, focalPoint);
    _clamp();
  }

  void reset() {
    zoom = 1;
    offset = Offset.zero;
    _beforeKeyboard = null;
    _clamp();
  }

  void keyboard(double height, Offset cursor) {
    final next = height.clamp(0, math.max(0, size.height - 44)).toDouble();
    if (next == keyboardHeight) return;
    if (keyboardHeight == 0 && next > 0) {
      _beforeKeyboard = (zoom, offset);
      magnify(math.max(zoom, 1.5) / zoom, toLocal(cursor));
    }
    keyboardHeight = next;
    if (next == 0) {
      final previous = _beforeKeyboard;
      _beforeKeyboard = null;
      if (previous != null) {
        zoom = previous.$1;
        offset = previous.$2;
      }
      _clamp();
    } else {
      follow(cursor);
    }
  }

  /// Returns whether another animation frame is needed. Small deltas snap so
  /// the view settles; reduced-motion callers use the default immediate step.
  bool follow(Offset cursor, {double fraction = 1}) {
    if (zoom == 1 && keyboardHeight == 0) return false;
    final point = toLocal(cursor);
    final area = visible;
    final marginX = area.width * .15;
    final marginY = area.height * .15;
    final dx = point.dx.clamp(area.left + marginX, area.right - marginX) - point.dx;
    final dy = point.dy.clamp(area.top + marginY, area.bottom - marginY) - point.dy;
    final delta = Offset(dx, dy);
    if (delta.distance < .25) return false;
    final before = offset;
    offset += delta * fraction;
    _clamp();
    return (offset - before).distance > .1 && fraction < 1;
  }

  void _clamp() {
    if (size.isEmpty) return;
    final area = visible;
    final width = fit.width * zoom, height = fit.height * zoom;
    double axis(double value, double start, double extent, double available) =>
        extent <= available ? (available - extent) / 2 - start : value.clamp(available - extent - start, -start);
    offset = Offset(axis(offset.dx, fit.left, width, area.width), axis(offset.dy, fit.top, height, area.height));
  }
}

/// Keeps the phone aspect ratio within the native frame budget. Logical
/// pixels avoid requesting a huge framebuffer on a high-density display.
(int, int) rdpPhoneResolution(Size safeSize, {double clarity = 1.5}) {
  if (safeSize.isEmpty) return (1280, 720);
  final longest = math.max(safeSize.width, safeSize.height);
  var scale = math.max(1280 / longest, clarity.clamp(1, 2));
  scale = math.min(scale, math.min(4096 / safeSize.width, 2160 / safeSize.height));
  int even(double value, int max) => ((value / 2).round() * 2).clamp(200, max);
  return (even(safeSize.width * scale, 4096), even(safeSize.height * scale, 2160));
}
