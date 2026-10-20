import 'package:consolecrypt/rdp/rdp_mobile_overlay.dart';
import 'package:consolecrypt/rdp/rdp_service.dart';
import 'package:consolecrypt/rdp/rdp_touchpad.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('a resized desktop keeps the cursor at the same relative spot', () {
    final pad = RdpTouchpad((_) {})..configure(1280, 720, 1280);
    expect(pad.position, const Offset(640, 360));
    pad.configure(480, 1040, 390);
    expect(pad.position, const Offset(240, 520));
  });

  testWidgets('pinch locks its decision and never sends a wheel or click', (tester) async {
    final inputs = <RdpInput>[], scales = <double>[];
    final pad = RdpTouchpad(inputs.addAll, onScale: (factor, _) => scales.add(factor));
    addTearDown(pad.dispose);
    pad.configure(1600, 1000, 800);
    pad.down(const PointerDownEvent(pointer: 1, position: Offset(200, 200)));
    pad.down(const PointerDownEvent(pointer: 2, position: Offset(240, 200)));
    pad.move(const PointerMoveEvent(pointer: 2, position: Offset(260, 200)));
    expect(scales.single, 1.5);
    pad.move(const PointerMoveEvent(pointer: 1, position: Offset(210, 250)));
    pad.move(const PointerMoveEvent(pointer: 2, position: Offset(270, 250)));
    pad.up(const PointerUpEvent(pointer: 1));
    pad.up(const PointerUpEvent(pointer: 2));
    await tester.pump(const Duration(seconds: 1));
    expect(inputs, isEmpty);
    expect(scales.length, greaterThan(1));
  });

  testWidgets('parallel motion locks scroll before a later separation', (tester) async {
    final inputs = <RdpInput>[], scales = <double>[];
    final pad = RdpTouchpad(inputs.addAll, onScale: (factor, _) => scales.add(factor));
    addTearDown(pad.dispose);
    pad.configure(800, 600, 400);
    pad.down(const PointerDownEvent(pointer: 1, position: Offset(100, 100)));
    pad.down(const PointerDownEvent(pointer: 2, position: Offset(180, 100)));
    pad.move(const PointerMoveEvent(pointer: 1, position: Offset(100, 130)));
    pad.move(const PointerMoveEvent(pointer: 2, position: Offset(180, 130)));
    pad.move(const PointerMoveEvent(pointer: 2, position: Offset(220, 130)));
    pad.up(const PointerUpEvent(pointer: 1));
    pad.up(const PointerUpEvent(pointer: 2));
    expect(inputs.whereType<RdpWheelInput>(), isNotEmpty);
    expect(inputs.whereType<RdpPointerInput>(), isEmpty);
    expect(scales, isEmpty);
  });

  testWidgets('two-finger double tap fits without opening a context menu', (tester) async {
    final inputs = <RdpInput>[];
    var fit = 0;
    final pad = RdpTouchpad(inputs.addAll, onFit: () => fit++);
    addTearDown(pad.dispose);
    pad.configure(800, 600, 400);
    for (final ms in [0, 150]) {
      pad.down(
        PointerDownEvent(
          pointer: 1,
          position: const Offset(100, 100),
          timeStamp: Duration(milliseconds: ms),
        ),
      );
      pad.down(
        PointerDownEvent(
          pointer: 2,
          position: const Offset(150, 100),
          timeStamp: Duration(milliseconds: ms),
        ),
      );
      pad.up(PointerUpEvent(pointer: 1, timeStamp: Duration(milliseconds: ms + 50)));
      pad.up(PointerUpEvent(pointer: 2, timeStamp: Duration(milliseconds: ms + 50)));
      await tester.pump(const Duration(milliseconds: 100));
    }
    await tester.pump(const Duration(seconds: 1));
    expect(fit, 1);
    expect(inputs, isEmpty);
  });

  testWidgets('three fingers toggle keyboard once and cancellation suppresses delayed right-click', (tester) async {
    final inputs = <RdpInput>[];
    var keyboard = 0;
    final pad = RdpTouchpad(inputs.addAll, onKeyboard: () => keyboard++);
    addTearDown(pad.dispose);
    pad.configure(800, 600, 400);
    for (var id = 1; id <= 3; id++) {
      pad.down(PointerDownEvent(pointer: id, position: Offset(id * 40, 100)));
    }
    for (var id = 1; id <= 3; id++) {
      pad.up(PointerUpEvent(pointer: id));
    }
    expect(keyboard, 1);
    expect(inputs, isEmpty);
    pad.down(const PointerDownEvent(pointer: 1, position: Offset(100, 100)));
    pad.down(const PointerDownEvent(pointer: 2, position: Offset(150, 100)));
    pad.up(const PointerUpEvent(pointer: 1));
    pad.up(const PointerUpEvent(pointer: 2));
    pad.cancel();
    await tester.pump(const Duration(seconds: 1));
    expect(inputs, isEmpty);
  });

  test('acceleration is bounded and slow gestures remain precise', () {
    expect(RdpTouchpad.gain(.01), 1);
    expect(RdpTouchpad.gain(.5), greaterThan(1));
    expect(RdpTouchpad.gain(100), 2.5);
    final inputs = <RdpInput>[];
    final pad = RdpTouchpad(inputs.addAll);
    addTearDown(pad.dispose);
    pad.configure(800, 600, 400);
    pad.down(const PointerDownEvent(pointer: 1, position: Offset(100, 100)));
    pad.move(const PointerMoveEvent(pointer: 1, position: Offset(110, 100), timeStamp: Duration(milliseconds: 100)));
    expect(pad.position.dx, 420);
    pad.move(const PointerMoveEvent(pointer: 1, position: Offset(120, 100), timeStamp: Duration(milliseconds: 105)));
    expect(pad.position.dx, 470);
    pad.cancel();
  });

  test('one-shot, locked and cancelled modifiers have balanced scancodes', () {
    final inputs = <RdpInput>[];
    final keys = RdpMobileKeys(inputs.addAll);
    addTearDown(keys.dispose);
    final now = DateTime(2026);
    keys.modifier(0x1d, at: now);
    keys.key(0x2e);
    expect(inputs.whereType<RdpScancodeInput>().map((e) => (e.code, e.down)), [
      (0x1d, true),
      (0x2e, true),
      (0x2e, false),
      (0x1d, false),
    ]);
    inputs.clear();
    keys.modifier(0x38, at: now);
    keys.modifier(0x38, at: now.add(const Duration(milliseconds: 100)));
    keys.key(0x0f);
    expect(keys.locked(0x38), isTrue);
    keys.modifier(0x38, at: now.add(const Duration(milliseconds: 200)));
    expect(keys.active(0x38), isFalse);
    expect(inputs.whereType<RdpScancodeInput>().map((e) => (e.code, e.down)), [
      (0x38, true),
      (0x0f, true),
      (0x0f, false),
      (0x38, false),
    ]);
    keys.modifier(0x5b);
    inputs.clear();
    keys.cancel(); // The owner separately issues native ReleaseAll.
    keys.key(0x13);
    expect(inputs.whereType<RdpScancodeInput>().map((e) => e.code), [0x13, 0x13]);
  });
}
