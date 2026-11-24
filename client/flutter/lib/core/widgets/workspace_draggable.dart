import 'package:consolecrypt/app/platform.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/widgets.dart';

/// In a wide Android window dragging with a mouse moves a row or tab;
/// a finger can still scroll the surrounding list or hold to open its menu.
class WorkspaceDraggable<T extends Object> extends Draggable<T> {
  const WorkspaceDraggable({
    required super.data,
    required super.feedback,
    required super.child,
    super.axis,
    super.childWhenDragging,
    super.dragAnchorStrategy,
    super.key,
  });

  @override
  MultiDragGestureRecognizer createRecognizer(GestureMultiDragStartCallback onStart) {
    if (!AppPlatform.isAndroid) return super.createRecognizer(onStart);
    return ImmediateMultiDragGestureRecognizer(supportedDevices: const {PointerDeviceKind.mouse})..onStart = onStart;
  }
}
