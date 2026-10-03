import 'dart:async';

import 'package:consolecrypt/rdp/rdp_service.dart';
import 'package:consolecrypt/rdp/rdp_view.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

Future<void> _open(
  WidgetTester tester,
  List<RdpInput> inputs, {
  bool enabled = true,
  RdpFrame? frame,
  bool localClipboardEnabled = false,
  Future<void> Function(bool Function())? onPaste,
}) async {
  await tester.pumpWidget(
    MaterialApp(
      home: Center(
        child: SizedBox(
          width: 400,
          height: 400,
          child: RdpView(
            frame: frame,
            width: 200,
            height: 100,
            enabled: enabled,
            onInput: inputs.addAll,
            localClipboardEnabled: localClipboardEnabled,
            onPaste: onPaste,
          ),
        ),
      ),
    ),
  );
  await tester.tap(find.byKey(const ValueKey('rdp-input-surface')));
  await tester.pump();
}

void main() {
  testWidgets('Windows VK V with Cyrillic character invokes local paste once', (tester) async {
    final inputs = <RdpInput>[];
    var pastes = 0;
    await _open(tester, inputs, localClipboardEnabled: true, onPaste: (_) async => pastes++);
    inputs.clear();
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    // Windows engine derives logicalV from VK V; the current layout's
    // Unicode character is independent of that shortcut identity.
    expect(
      await tester.sendKeyDownEvent(LogicalKeyboardKey.keyV, physicalKey: PhysicalKeyboardKey.keyV, character: 'м'),
      isTrue,
    );
    expect(
      await tester.sendKeyRepeatEvent(LogicalKeyboardKey.keyV, physicalKey: PhysicalKeyboardKey.keyV, character: 'м'),
      isTrue,
    );
    expect(await tester.sendKeyUpEvent(LogicalKeyboardKey.keyV, physicalKey: PhysicalKeyboardKey.keyV), isTrue);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    await tester.pump();
    expect(pastes, 1);
    expect(inputs.whereType<RdpScancodeInput>().map((k) => (k.code, k.down)), [(0x1d, true), (0x1d, false)]);
    expect(inputs.whereType<RdpUnicodeInput>(), isEmpty);
    await tester.pumpWidget(const SizedBox.shrink());
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('Windows AltGr V preserves layout Unicode rather than local paste', (tester) async {
    final inputs = <RdpInput>[];
    var pastes = 0;
    await _open(tester, inputs, localClipboardEnabled: true, onPaste: (_) async => pastes++);
    inputs.clear();
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.altRight);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.keyV, physicalKey: PhysicalKeyboardKey.keyV, character: 'м');
    await tester.sendKeyUpEvent(LogicalKeyboardKey.keyV, physicalKey: PhysicalKeyboardKey.keyV);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.altRight);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    expect(pastes, 0);
    expect(inputs.whereType<RdpUnicodeInput>().single.text, 'м');
    await tester.pumpWidget(const SizedBox.shrink());
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('local paste consumes repeat/up and releases Ctrl before its async action', (tester) async {
    final inputs = <RdpInput>[];
    final pending = Completer<void>();
    var count = 0;
    bool Function()? current;
    await _open(
      tester,
      inputs,
      localClipboardEnabled: true,
      onPaste: (isCurrent) {
        count++;
        current = isCurrent;
        return pending.future;
      },
    );
    inputs.clear();
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    expect(await tester.sendKeyDownEvent(LogicalKeyboardKey.keyV), isTrue);
    expect(await tester.sendKeyRepeatEvent(LogicalKeyboardKey.keyV), isTrue);
    expect(await tester.sendKeyUpEvent(LogicalKeyboardKey.keyV), isTrue);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    expect(count, 1);
    expect(current!(), isTrue);
    expect(inputs.whereType<RdpScancodeInput>().map((k) => (k.code, k.down)), [(0x1d, true), (0x1d, false)]);
    expect(inputs.whereType<RdpReleaseAllInput>(), hasLength(1));
    FocusManager.instance.primaryFocus!.unfocus();
    await tester.pump();
    await tester.tap(find.byKey(const ValueKey('rdp-input-surface')));
    await tester.pump();
    expect(current!(), isFalse);
    pending.complete();
    await tester.pump();
    await tester.pumpWidget(const SizedBox.shrink());
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('paste with extra modifiers stays a remote shortcut', (tester) async {
    final inputs = <RdpInput>[];
    var pastes = 0;
    await _open(tester, inputs, localClipboardEnabled: true, onPaste: (_) async => pastes++);
    inputs.clear();
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyV);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    expect(pastes, 0);
    expect(inputs.whereType<RdpScancodeInput>().map((k) => (k.code, k.down)), [
      (0x1d, true),
      (0x2a, true),
      (0x2f, true),
      (0x2f, false),
      (0x2a, false),
      (0x1d, false),
    ]);
    await tester.pumpWidget(const SizedBox.shrink());
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  test('fit rectangle preserves aspect ratio and letterboxing', () {
    expect(rdpFitRect(const Size(400, 400), 200, 100), const Rect.fromLTWH(0, 100, 400, 200));
  });

  testWidgets('single click attaches native text input; RU EN and IME commit once', (tester) async {
    final inputs = <RdpInput>[];
    await _open(tester, inputs);
    expect(tester.testTextInput.isRegistered, isTrue);
    tester.testTextInput.enterText('привет');
    await tester.pump();
    tester.testTextInput.enterText('hello');
    await tester.pump();
    tester.testTextInput.updateEditingValue(
      const TextEditingValue(
        text: '\u200bに',
        selection: TextSelection.collapsed(offset: 2),
        composing: TextRange(start: 1, end: 2),
      ),
    );
    await tester.pump();
    expect(inputs.whereType<RdpUnicodeInput>().map((input) => input.text).toList(), ['привет', 'hello']);
    tester.testTextInput.updateEditingValue(
      const TextEditingValue(text: '\u200b日本', selection: TextSelection.collapsed(offset: 3)),
    );
    await tester.pump();
    expect(inputs.whereType<RdpUnicodeInput>().map((input) => input.text).toList(), ['привет', 'hello', '日本']);
    await tester.pumpWidget(const SizedBox.shrink());
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('Ctrl shortcut uses physical scancodes and releases held key on blur', (tester) async {
    final inputs = <RdpInput>[];
    await _open(tester, inputs);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft, physicalKey: PhysicalKeyboardKey.controlLeft);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.keyC, physicalKey: PhysicalKeyboardKey.keyC);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.keyC, physicalKey: PhysicalKeyboardKey.keyC);
    FocusManager.instance.primaryFocus!.unfocus();
    await tester.pump();
    final keys = inputs.whereType<RdpScancodeInput>().map((input) => (input.code, input.down)).toList();
    expect(keys, [(0x1d, true), (0x2e, true), (0x2e, false), (0x1d, false)]);
    expect(inputs.whereType<RdpUnicodeInput>(), isEmpty);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft, physicalKey: PhysicalKeyboardKey.controlLeft);
    await tester.pumpWidget(const SizedBox.shrink());
  });

  testWidgets('Windows hardware Unicode consumes WM_CHAR once including key repeat', (tester) async {
    final inputs = <RdpInput>[];
    await _open(tester, inputs);
    expect(await tester.sendKeyEvent(LogicalKeyboardKey.keyA, character: 'ф'), isTrue);
    expect(await tester.sendKeyEvent(LogicalKeyboardKey.backquote, character: 'ё'), isTrue);
    expect(await tester.sendKeyEvent(LogicalKeyboardKey.space, character: ' '), isTrue);
    expect(await tester.sendKeyDownEvent(LogicalKeyboardKey.keyL, character: 'l'), isTrue);
    expect(await tester.sendKeyRepeatEvent(LogicalKeyboardKey.keyL, character: 'l'), isTrue);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.keyL);
    expect(inputs.whereType<RdpUnicodeInput>().map((input) => input.text).join(), 'фё ll');
    await tester.pumpWidget(const SizedBox.shrink());
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('pointer and wheel map through fitted remote image; border sends no click', (tester) async {
    final inputs = <RdpInput>[];
    await _open(tester, inputs);
    final rect = tester.getRect(find.byKey(const ValueKey('rdp-input-surface')));
    final clicks = inputs.whereType<RdpPointerInput>().where((input) => input.button != null).toList();
    expect(clicks.map((input) => (input.x, input.y, input.down)).toList(), [(100, 50, true), (100, 50, false)]);
    inputs.clear();
    await tester.tapAt(rect.topLeft + const Offset(30, 20));
    expect(inputs.whereType<RdpPointerInput>(), isEmpty);
    await tester.sendEventToBinding(PointerScrollEvent(position: rect.center, scrollDelta: const Offset(0, 120)));
    await tester.pump();
    final wheel = inputs.whereType<RdpWheelInput>().single;
    expect((wheel.x, wheel.y, wheel.vertical), (100, 50, -120));
    await tester.pumpWidget(const SizedBox.shrink());
  });

  testWidgets('disabled surface cannot attach text input or forward keyboard/pointer', (tester) async {
    final inputs = <RdpInput>[];
    await _open(tester, inputs, enabled: false);
    expect(tester.testTextInput.hasAnyClients, isFalse);
    expect(inputs, isEmpty);
    await tester.pumpWidget(const SizedBox.shrink());
  });

  testWidgets('real RGBA decode can be replaced and disabled without stale image callbacks', (tester) async {
    final inputs = <RdpInput>[];
    final frame = RdpFrame(
      sequence: 0,
      width: 2,
      height: 1,
      rgba: Uint8List.fromList([255, 0, 0, 255, 0, 255, 0, 255]),
    );
    await _open(tester, inputs, frame: frame);
    await _open(tester, inputs, enabled: false);
    await tester.pumpWidget(const SizedBox.shrink());
    await tester.pump();
    expect(tester.takeException(), isNull);
  });
}
