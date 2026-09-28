import 'package:consolecrypt/sftp/widgets/numbered_text_preview.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

const _textKey = ValueKey('sftp-quicklook-text');
const _numbersKey = ValueKey('sftp-preview-line-numbers');

Future<void> _pump(WidgetTester tester, String text, {double scale = 1}) async {
  await tester.pumpWidget(
    MaterialApp(
      home: Scaffold(
        body: MediaQuery(
          data: MediaQueryData(textScaler: TextScaler.linear(scale)),
          child: SizedBox(width: 320, height: 240, child: NumberedTextPreview(text: text)),
        ),
      ),
    ),
  );
  await tester.pump();
}

void main() {
  for (final (text, numbers) in [('', '1'), ('one', '1'), ('one\ntwo\n', '1\n2\n3'), ('one\r\ntwo', '1\n2')]) {
    testWidgets('line count preserves document ${text.codeUnits}', (tester) async {
      await _pump(tester, text);
      expect(tester.widget<Text>(find.byKey(_numbersKey)).data, numbers);
      expect(tester.widget<SelectableText>(find.byKey(_textKey)).data, text);
      expect(tester.takeException(), isNull);
    });
  }

  testWidgets('select all and copy includes only original file contents', (tester) async {
    const document = 'first line\n  indented = true\nlast line\n';
    String? copied;
    tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(SystemChannels.platform, (call) async {
      if (call.method == 'Clipboard.setData') copied = (call.arguments as Map)['text'] as String;
      if (call.method == 'Clipboard.hasStrings') return {'value': false};
      return null;
    });
    addTearDown(() => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(SystemChannels.platform, null));
    await _pump(tester, document);
    await tester.tap(find.byKey(_textKey));
    await tester.pump();
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyA);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyC);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    await tester.pump();
    expect(copied, document);
    expect(tester.widgetList<SelectableText>(find.byType(SelectableText)).length, 1);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('long lines scroll horizontally, gutter stays pinned and scrolls vertically with text', (tester) async {
    final document = List.generate(40, (i) => 'line $i ${'content ' * 30}').join('\n');
    await _pump(tester, document, scale: 1.4);
    final gutter = find.byKey(_numbersKey);
    final text = find.byKey(_textKey);
    final before = tester.getTopLeft(gutter);
    final textBefore = tester.getTopLeft(text);
    await tester.sendEventToBinding(const PointerScrollEvent(position: Offset(240, 100), scrollDelta: Offset(180, 0)));
    await tester.pumpAndSettle();
    expect(tester.getTopLeft(gutter), before);
    expect(tester.getTopLeft(text).dx, lessThan(textBefore.dx));
    await tester.sendEventToBinding(const PointerScrollEvent(position: Offset(240, 100), scrollDelta: Offset(0, 100)));
    await tester.pumpAndSettle();
    expect(tester.getTopLeft(gutter).dy, lessThan(before.dy));
    expect(tester.getTopLeft(text).dy, closeTo(tester.getTopLeft(gutter).dy, .01));
    expect(tester.getSize(text).height, closeTo(tester.getSize(gutter).height, 1));
    expect(tester.takeException(), isNull);
  });
}
