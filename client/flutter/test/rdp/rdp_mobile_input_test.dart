import 'package:consolecrypt/core/models/rdp_mobile.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/rdp/rdp_service.dart';
import 'package:consolecrypt/rdp/rdp_view.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

const surfaceKey = ValueKey('rdp-input-surface');
const mouseKey = ValueKey('rdp-touchpad-toggle');
const keyboardKey = ValueKey('rdp-mobile-keyboard');
final mobile = TargetPlatformVariant.only(TargetPlatform.android);

Future<void> open(
  WidgetTester tester,
  List<RdpInput> inputs, {
  bool enabled = true,
  String session = 'one',
  Size size = const Size(400, 400),
}) async {
  await tester.pumpWidget(
    MaterialApp(
      localizationsDelegates: AppLocalizations.localizationsDelegates,
      supportedLocales: AppLocalizations.supportedLocales,
      home: Scaffold(
        resizeToAvoidBottomInset: false,
        body: Center(
          child: SizedBox(
            width: size.width,
            height: size.height,
            child: RdpView(
              key: ValueKey(session),
              frame: null,
              width: 800,
              height: 600,
              enabled: enabled,
              onInput: inputs.addAll,
              mobileSettings: const RdpMobileSettings(acceleration: false),
            ),
          ),
        ),
      ),
    ),
  );
  await tester.pump();
}

Future<void> menu(WidgetTester tester) async {
  if (find.byKey(const ValueKey('rdp-mobile-panel')).evaluate().isEmpty) {
    await tester.tap(find.byKey(const ValueKey('rdp-mobile-menu')));
    await tester.pump();
  }
}

Future<void> keyboard(WidgetTester tester) async {
  await menu(tester);
  await tester.ensureVisible(find.byKey(keyboardKey));
  await tester.tap(find.byKey(keyboardKey));
  await tester.pump();
  tester.view.viewInsets = tester.testTextInput.isVisible ? const FakeViewPadding(bottom: 100) : FakeViewPadding.zero;
  addTearDown(tester.view.resetViewInsets);
  await tester.pump();
}

Future<Rect> touchpad(WidgetTester tester, List<RdpInput> inputs) async {
  await open(tester, inputs);
  await menu(tester);
  await tester.ensureVisible(find.byKey(mouseKey));
  await tester.tap(find.byKey(mouseKey));
  await tester.pump();
  await tester.tap(find.byKey(const ValueKey('rdp-mobile-menu')));
  await tester.pump();
  inputs.clear();
  return tester.getRect(find.byKey(surfaceKey));
}

List<(int, int, RdpPointerButton?, bool)> pointers(List<RdpInput> inputs) =>
    inputs.whereType<RdpPointerInput>().map((e) => (e.x, e.y, e.button, e.down)).toList();

void main() {
  testWidgets('relative cursor survives lifting and repositioning; swipe is not a click', (tester) async {
    final inputs = <RdpInput>[];
    final rect = await touchpad(tester, inputs);
    final first = await tester.startGesture(rect.topLeft + const Offset(20, 20));
    expect(inputs, isEmpty);
    await first.moveBy(const Offset(40, 15));
    await first.up();
    expect(pointers(inputs), [(480, 330, null, false)]);
    inputs.clear();
    final second = await tester.startGesture(rect.bottomRight - const Offset(40, 40));
    await second.moveBy(const Offset(10, -10));
    await second.up();
    expect(pointers(inputs), [(500, 310, null, false)]);
    expect(tester.testTextInput.isVisible, isFalse);
  }, variant: mobile);

  testWidgets('single and double taps click at cursor, independent of finger coordinates', (tester) async {
    final inputs = <RdpInput>[];
    final rect = await touchpad(tester, inputs);
    final point = rect.topLeft + const Offset(25, 25);
    await tester.tapAt(point);
    await tester.pump(const Duration(milliseconds: 80));
    await tester.tapAt(point);
    expect(pointers(inputs), [
      (400, 300, RdpPointerButton.left, true),
      (400, 300, RdpPointerButton.left, false),
      (400, 300, RdpPointerButton.left, true),
      (400, 300, RdpPointerButton.left, false),
    ]);
  }, variant: mobile);

  testWidgets('double tap and hold drags then releases at the new cursor position', (tester) async {
    final inputs = <RdpInput>[];
    final rect = await touchpad(tester, inputs);
    await tester.tapAt(rect.center);
    await tester.pump(const Duration(milliseconds: 80));
    inputs.clear();
    final drag = await tester.startGesture(rect.center);
    await drag.moveBy(const Offset(20, -10));
    await drag.up();
    expect(pointers(inputs), [
      (400, 300, RdpPointerButton.left, true),
      (440, 280, null, false),
      (440, 280, RdpPointerButton.left, false),
    ]);
  }, variant: mobile);

  testWidgets('two fingers tap right click and scroll without moving or clicking', (tester) async {
    final inputs = <RdpInput>[];
    final rect = await touchpad(tester, inputs);
    var one = await tester.startGesture(rect.center - const Offset(20, 0), pointer: 1);
    var two = await tester.startGesture(rect.center + const Offset(20, 0), pointer: 2);
    await one.up();
    await two.up();
    await tester.pump(const Duration(milliseconds: 310));
    expect(pointers(inputs), [(400, 300, RdpPointerButton.right, true), (400, 300, RdpPointerButton.right, false)]);
    inputs.clear();
    one = await tester.startGesture(rect.center - const Offset(20, 0), pointer: 3);
    two = await tester.startGesture(rect.center + const Offset(20, 0), pointer: 4);
    await one.moveBy(const Offset(0, -30));
    await two.moveBy(const Offset(0, -30));
    await one.up();
    await two.moveBy(const Offset(25, 25));
    await two.up();
    expect(pointers(inputs), isEmpty);
    expect(inputs.whereType<RdpWheelInput>().fold(0, (sum, e) => sum + e.vertical), 90);
  }, variant: mobile);

  testWidgets('long hold releases on pointer cancellation and has no delayed click', (tester) async {
    final inputs = <RdpInput>[];
    final rect = await touchpad(tester, inputs);
    final drag = await tester.startGesture(rect.center);
    await tester.pump(const Duration(milliseconds: 550));
    expect(pointers(inputs).single, (400, 300, RdpPointerButton.left, true));
    await drag.cancel();
    expect(inputs.whereType<RdpReleaseAllInput>(), isNotEmpty);
    inputs.clear();
    await tester.pump(const Duration(seconds: 1));
    expect(inputs, isEmpty);
  }, variant: mobile);

  testWidgets('cursor clamps at edges and moves back without accumulated overflow', (tester) async {
    final inputs = <RdpInput>[];
    final rect = await touchpad(tester, inputs);
    final drag = await tester.startGesture(rect.center);
    await drag.moveBy(const Offset(1000, 1000));
    expect(pointers(inputs).last, (799, 599, null, false));
    await drag.moveBy(const Offset(-10, -10));
    expect(pointers(inputs).last, (779, 579, null, false));
    await drag.up();
  }, variant: mobile);

  testWidgets('keyboard is explicit, commits RU/IME once, and closes independently', (tester) async {
    final inputs = <RdpInput>[];
    final rect = await touchpad(tester, inputs);
    await tester.tapAt(rect.center);
    expect(tester.testTextInput.isVisible, isFalse);
    await keyboard(tester);
    await tester.pump();
    expect(tester.testTextInput.isVisible, isTrue);
    inputs.clear();
    tester.testTextInput.enterText('Привет');
    await tester.pump();
    tester.testTextInput.updateEditingValue(
      const TextEditingValue(
        text: '\u200bに',
        selection: TextSelection.collapsed(offset: 2),
        composing: TextRange(start: 1, end: 2),
      ),
    );
    expect(inputs.whereType<RdpUnicodeInput>().map((e) => e.text), ['Привет']);
    tester.testTextInput.updateEditingValue(
      const TextEditingValue(text: '\u200b日本', selection: TextSelection.collapsed(offset: 3)),
    );
    await tester.pump();
    expect(inputs.whereType<RdpUnicodeInput>().map((e) => e.text), ['Привет', '日本']);
    await tester.tap(find.text('Tab'));
    expect(inputs.whereType<RdpScancodeInput>().map((e) => (e.code, e.down)), [(0x0f, true), (0x0f, false)]);
    await keyboard(tester);
    await tester.pump();
    expect(tester.testTextInput.isVisible, isFalse);
    await tester.tapAt(tester.getRect(find.byKey(surfaceKey)).center);
    await tester.pump();
    expect(tester.testTextInput.isVisible, isFalse);
  }, variant: mobile);

  testWidgets('lock or session replacement cancels pending long press and keyboard', (tester) async {
    final inputs = <RdpInput>[];
    final rect = await touchpad(tester, inputs);
    final finger = await tester.startGesture(rect.center);
    await tester.pump(const Duration(milliseconds: 100));
    await open(tester, inputs, enabled: false);
    inputs.clear();
    await tester.pump(const Duration(seconds: 1));
    await finger.up();
    expect(inputs, isEmpty);
    expect(tester.testTextInput.isVisible, isFalse);
    await open(tester, inputs, session: 'two');
    await menu(tester);
    expect(find.text('Direct touch'), findsOneWidget);
  }, variant: mobile);

  testWidgets('background cancels held drag and pending gestures', (tester) async {
    final inputs = <RdpInput>[];
    final rect = await touchpad(tester, inputs);
    final drag = await tester.startGesture(rect.center);
    await tester.pump(const Duration(milliseconds: 550));
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.inactive);
    await tester.pump();
    expect(inputs.whereType<RdpReleaseAllInput>(), isNotEmpty);
    inputs.clear();
    await drag.moveBy(const Offset(20, 20));
    await drag.up();
    expect(inputs, isEmpty);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
    await tester.pump();
    expect(tester.testTextInput.isVisible, isFalse);
    await menu(tester);
    expect(tester.widget<TextButton>(find.byKey(keyboardKey)).onPressed, isNotNull);
    await keyboard(tester);
    await tester.pump();
    expect(tester.testTextInput.isVisible, isTrue);
  }, variant: mobile);

  testWidgets('IME composition blocks accessory keys until text is committed', (tester) async {
    final inputs = <RdpInput>[];
    await open(tester, inputs);
    await keyboard(tester);
    await tester.pump();
    inputs.clear();
    tester.testTextInput.updateEditingValue(
      const TextEditingValue(
        text: '\u200bに',
        selection: TextSelection.collapsed(offset: 2),
        composing: TextRange(start: 1, end: 2),
      ),
    );
    await tester.tap(find.text('Tab'));
    await tester.testTextInput.receiveAction(TextInputAction.newline);
    expect(inputs, isEmpty);
    tester.testTextInput.updateEditingValue(
      const TextEditingValue(text: '\u200b日本', selection: TextSelection.collapsed(offset: 3)),
    );
    await tester.testTextInput.receiveAction(TextInputAction.newline);
    expect(inputs.whereType<RdpUnicodeInput>().single.text, '日本');
    expect(inputs.whereType<RdpScancodeInput>().map((e) => (e.code, e.down)), [(0x1c, true), (0x1c, false)]);
  }, variant: mobile);

  testWidgets('Android IME dismissal updates toggle without closing text connection', (tester) async {
    final inputs = <RdpInput>[];
    await open(tester, inputs);
    addTearDown(tester.view.resetViewInsets);
    await keyboard(tester);
    await tester.pump();
    tester.view.viewInsets = const FakeViewPadding(bottom: 200);
    await tester.pump();
    await tester.pump();
    tester.view.viewInsets = FakeViewPadding.zero;
    await tester.pump();
    await tester.pump();
    await menu(tester);
    expect(find.text('Show keyboard'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('rdp-mobile-menu')));
    await tester.pump();
    await tester.tapAt(tester.getRect(find.byKey(surfaceKey)).topLeft + const Offset(50, 50));
    await tester.pump();
    expect(tester.testTextInput.isVisible, isFalse);
  }, variant: mobile);

  testWidgets('external mouse updates local cursor without a new framebuffer', (tester) async {
    final inputs = <RdpInput>[];
    final rect = await touchpad(tester, inputs);
    final paint = find.descendant(of: find.byKey(surfaceKey), matching: find.byType(CustomPaint));
    final before = tester.widget<CustomPaint>(paint).painter;
    final mouse = await tester.createGesture(kind: PointerDeviceKind.mouse);
    await mouse.addPointer(location: rect.center);
    await mouse.moveTo(rect.center + const Offset(20, 10));
    await tester.pump();
    final after = tester.widget<CustomPaint>(paint).painter;
    expect(after!.shouldRepaint(before!), isTrue);
    await mouse.removePointer();
  }, variant: mobile);

  testWidgets('attached mouse stays absolute in touchpad mode', (tester) async {
    final inputs = <RdpInput>[];
    final rect = await touchpad(tester, inputs);
    final mouse = await tester.startGesture(rect.center + const Offset(40, 15), kind: PointerDeviceKind.mouse);
    await mouse.up();
    expect(pointers(inputs), [(480, 330, RdpPointerButton.left, true), (480, 330, RdpPointerButton.left, false)]);
  }, variant: mobile);

  testWidgets('mobile controls fit narrow landscape with keyboard open', (tester) async {
    final inputs = <RdpInput>[];
    await open(tester, inputs, size: const Size(320, 170));
    await keyboard(tester);
    await tester.pump();
    expect(tester.takeException(), isNull);
    expect(tester.getSize(find.byKey(surfaceKey)).height, greaterThan(0));
  }, variant: mobile);

  testWidgets('desktop has no phone controls and keeps existing pointer/IME', (tester) async {
    final inputs = <RdpInput>[];
    await open(tester, inputs);
    expect(find.byKey(const ValueKey('rdp-mobile-menu')), findsNothing);
    await tester.tap(find.byKey(surfaceKey));
    await tester.pump();
    expect(tester.testTextInput.isVisible, isTrue);
    expect(pointers(inputs), [(400, 300, RdpPointerButton.left, true), (400, 300, RdpPointerButton.left, false)]);
  }, variant: TargetPlatformVariant.desktop());
}
