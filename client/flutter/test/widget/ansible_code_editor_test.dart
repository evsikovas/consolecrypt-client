import 'dart:async';
import 'dart:ui' as ui;

import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/automation/ansible_code_editor.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/security/secure_clipboard.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

const source = '''- name: hidden
  ansible.builtin.debug:
    msg: hidden-source-value
- name: visible
  ansible.builtin.debug:
    msg: visible
''';

SecureClipboard? protectedClipboard;
Future<bool> protectedCopy(String text) async {
  try {
    await protectedClipboard!.copySecret(text);
    return true;
  } catch (_) {
    return false;
  }
}

Widget harness(
  TextEditingController controller, {
  bool readOnly = false,
  bool softWrap = false,
  AnsibleEditorViewController? commands,
  ValueChanged<String>? onChanged,
  bool copyEnabled = true,
  Future<bool> Function(String)? onCopySelection,
}) {
  protectedClipboard ??= SecureClipboard(clearAfter: const Duration(seconds: 30));
  return MaterialApp(
    theme: AppTheme.dark(platform: TargetPlatform.windows),
    locale: const Locale('en'),
    supportedLocales: AppLocalizations.supportedLocales,
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    home: Scaffold(
      body: AnsibleCodeEditor(
        controller: controller,
        readOnly: readOnly,
        softWrap: softWrap,
        viewController: commands,
        onChanged: onChanged,
        onCopySelection: copyEnabled ? (onCopySelection ?? protectedCopy) : null,
      ),
    ),
  );
}

Future<void> ctrl(WidgetTester tester, LogicalKeyboardKey key) async {
  await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
  await tester.sendKeyEvent(key);
  await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
  await tester.pump();
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  var clipboard = '';
  var plainWrites = 0, protectedWrites = 0;
  var protectedUnavailable = false;
  int? promisedClearMilliseconds;
  setUp(() {
    clipboard = '';
    plainWrites = 0;
    protectedWrites = 0;
    protectedUnavailable = false;
    promisedClearMilliseconds = null;
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(
      SecureClipboard.channel,
      (call) async {
        if (call.method == 'copySecret') {
          if (protectedUnavailable) throw PlatformException(code: 'unavailable');
          protectedWrites++;
          clipboard = (call.arguments as Map)['text'] as String;
          promisedClearMilliseconds = (call.arguments as Map)['clearAfterMilliseconds'] as int;
          return 'owned-fixture-token';
        }
        if (call.method == 'clearIfOwned') {
          clipboard = '';
          return 'cleared';
        }
        return null;
      },
    );
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(
      SystemChannels.platform,
      (call) async {
        switch (call.method) {
          case 'Clipboard.setData':
            plainWrites++;
            clipboard = (call.arguments as Map)['text'] as String;
            return null;
          case 'Clipboard.getData':
            return {'text': clipboard};
          case 'Clipboard.hasStrings':
            return {'value': clipboard.isNotEmpty};
          default:
            return null;
        }
      },
    );
  });
  tearDown(() {
    protectedClipboard?.dispose();
    protectedClipboard = null;
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(
      SecureClipboard.channel,
      null,
    );
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(
      SystemChannels.platform,
      null,
    );
  });
  testWidgets('soft wrap keeps the next logical line aligned and preserves the selected source', (tester) async {
    final line = 'some code ' * 18;
    final controller = TextEditingController(text: '$line\nnext')
      ..selection = TextSelection(baseOffset: 2, extentOffset: line.length + 1);
    final selection = controller.selection;
    addTearDown(controller.dispose);
    await tester.pumpWidget(harness(controller));
    final label = find.byKey(const ValueKey('ansible-line-1'));
    final firstTop = tester.getTopLeft(label).dy;
    await tester.pumpWidget(harness(controller, softWrap: true));
    await tester.pump();
    expect(tester.getTopLeft(label).dy, greaterThan(firstTop));
    expect(tester.widget<TextField>(find.byType(TextField)).selectionWidthStyle, ui.BoxWidthStyle.tight);
    final editable = tester.state<EditableTextState>(find.byType(EditableText)).renderEditable;
    final caret = editable.getLocalRectForCaret(TextPosition(offset: line.length + 1));
    expect((editable.localToGlobal(caret.topLeft).dy - tester.getTopLeft(label).dy).abs(), lessThan(5));
    expect(controller.text, '$line\nnext');
    expect(controller.selection, selection);
  });

  testWidgets('source copy honors protected secret clearing and never writes ordinary clipboard', (tester) async {
    final raw = TextEditingController(text: 'ansible_password: revealed-fixture-secret\n');
    addTearDown(raw.dispose);
    await tester.pumpWidget(harness(raw));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('automation-editor')));
    await ctrl(tester, LogicalKeyboardKey.keyA);
    await ctrl(tester, LogicalKeyboardKey.keyC);
    expect(clipboard, raw.text);
    expect(protectedWrites, 1);
    expect(plainWrites, 0);
    expect(promisedClearMilliseconds, 30000);
    await tester.pump(const Duration(seconds: 31));
    await tester.pump();
    expect(clipboard, isEmpty);
    expect(raw.text, 'ansible_password: revealed-fixture-secret\n');
    protectedClipboard?.dispose();
    await tester.pumpWidget(const SizedBox());
  });
  testWidgets('failed protected copy never cuts data or falls back to plain clipboard', (tester) async {
    final raw = TextEditingController(text: source);
    addTearDown(raw.dispose);
    protectedUnavailable = true;
    await tester.pumpWidget(harness(raw));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('automation-editor')));
    await ctrl(tester, LogicalKeyboardKey.keyA);
    await ctrl(tester, LogicalKeyboardKey.keyX);
    expect(raw.text, source);
    expect(plainWrites, 0);
    expect(protectedWrites, 0);
    expect(clipboard, isEmpty);
    protectedClipboard?.dispose();
    await tester.pumpWidget(const SizedBox());
  });
  testWidgets('missing copy callback disables copy and cut without leaking the selected source', (tester) async {
    final raw = TextEditingController(text: source);
    addTearDown(raw.dispose);
    await tester.pumpWidget(harness(raw, copyEnabled: false));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('automation-editor')));
    await ctrl(tester, LogicalKeyboardKey.keyA);
    await ctrl(tester, LogicalKeyboardKey.keyC);
    await ctrl(tester, LogicalKeyboardKey.keyX);
    expect(raw.text, source);
    expect(plainWrites, 0);
    expect(protectedWrites, 0);
    final state = tester.state<EditableTextState>(find.byType(EditableText));
    state.showToolbar();
    await tester.pumpAndSettle();
    expect(find.text('Copy'), findsNothing);
    expect(find.text('Cut'), findsNothing);
    protectedClipboard?.dispose();
    await tester.pumpWidget(const SizedBox());
  });
  testWidgets('late successful copy cannot cut text edited while clipboard ownership was pending', (tester) async {
    final raw = TextEditingController(text: source);
    addTearDown(raw.dispose);
    final pending = Completer<bool>();
    await tester.pumpWidget(harness(raw, onCopySelection: (_) => pending.future));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('automation-editor')));
    await ctrl(tester, LogicalKeyboardKey.keyA);
    await ctrl(tester, LogicalKeyboardKey.keyX);
    raw.text = 'new unsaved edit';
    pending.complete(true);
    await tester.pump();
    expect(raw.text, 'new unsaved edit');
    expect(plainWrites, 0);
    protectedClipboard?.dispose();
    await tester.pumpWidget(const SizedBox());
  });
  testWidgets('late copied selection cannot cut a replacement file controller', (tester) async {
    final first = TextEditingController(text: source);
    final second = TextEditingController(text: 'replacement file');
    addTearDown(() {
      first.dispose();
      second.dispose();
    });
    final pending = Completer<bool>();
    Future<bool> copy(String _) => pending.future;
    await tester.pumpWidget(harness(first, onCopySelection: copy));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('automation-editor')));
    await ctrl(tester, LogicalKeyboardKey.keyA);
    await ctrl(tester, LogicalKeyboardKey.keyX);
    await tester.pumpWidget(harness(second, onCopySelection: copy));
    pending.complete(true);
    await tester.pump();
    expect(first.text, source);
    expect(second.text, 'replacement file');
    expect(plainWrites, 0);
    protectedClipboard?.dispose();
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('actual shortcuts copy cut paste undo preserve folded canonical document', (tester) async {
    final raw = TextEditingController(text: source);
    addTearDown(raw.dispose);
    final saves = <String>[];
    await tester.pumpWidget(harness(raw, onChanged: saves.add));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('ansible-fold-1')));
    await tester.pump();
    final field = tester.widget<TextField>(find.byKey(const ValueKey('automation-editor')));
    expect(field.decoration!.filled, isFalse);
    expect(field.decoration!.enabledBorder, InputBorder.none);
    expect(field.decoration!.focusedBorder, InputBorder.none);
    final frame = tester.widget<Container>(find.byKey(const ValueKey('ansible-editor-frame')));
    expect((frame.decoration! as BoxDecoration).border, isNull);
    expect(field.controller!.text, contains('# … 2'));
    expect(raw.text, source);
    expect(saves, isEmpty);
    expect(tester.widget<Text>(find.byKey(const ValueKey('ansible-line-2'))).data, '4');
    await tester.tap(find.byKey(const ValueKey('automation-editor')));
    await ctrl(tester, LogicalKeyboardKey.keyA);
    await ctrl(tester, LogicalKeyboardKey.keyC);
    expect((await Clipboard.getData(Clipboard.kTextPlain))!.text, source);
    await ctrl(tester, LogicalKeyboardKey.keyX);
    await tester.pump();
    expect(raw.text, isEmpty);
    expect((await Clipboard.getData(Clipboard.kTextPlain))!.text, source);
    await ctrl(tester, LogicalKeyboardKey.keyZ);
    expect(raw.text, source);
    await ctrl(tester, LogicalKeyboardKey.keyA);
    await Clipboard.setData(const ClipboardData(text: '- name: pasted\n'));
    await ctrl(tester, LogicalKeyboardKey.keyV);
    await tester.pump();
    expect(raw.text, '- name: pasted\n');
    await ctrl(tester, LogicalKeyboardKey.keyZ);
    expect(raw.text, source);
    expect(saves.every((value) => !value.contains('# …')), isTrue);
    protectedClipboard?.dispose();
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('real context menu copy exports hidden source rather than fold marker', (tester) async {
    final raw = TextEditingController(text: source);
    addTearDown(raw.dispose);
    await tester.pumpWidget(harness(raw));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('ansible-fold-1')));
    await tester.tap(find.byKey(const ValueKey('automation-editor')));
    await ctrl(tester, LogicalKeyboardKey.keyA);
    final state = tester.state<EditableTextState>(find.byType(EditableText));
    state.showToolbar();
    await tester.pumpAndSettle();
    expect(find.text('Copy'), findsOneWidget);
    await tester.tap(find.text('Copy'));
    await tester.pump();
    expect((await Clipboard.getData(Clipboard.kTextPlain))!.text, source);
    expect(raw.text, source);
    protectedClipboard?.dispose();
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('controller swap clears undo and masked semantics clipboard stay masked', (tester) async {
    const secret = 'private-secret-value';
    final raw = TextEditingController(text: 'ansible_password: $secret\n');
    final masked = TextEditingController(text: 'ansible_password: ${'•' * secret.length}\n');
    final commands = AnsibleEditorViewController();
    addTearDown(() {
      raw.dispose();
      masked.dispose();
      commands.dispose();
    });
    await tester.pumpWidget(harness(raw, commands: commands));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('automation-editor')));
    await ctrl(tester, LogicalKeyboardKey.keyA);
    await Clipboard.setData(const ClipboardData(text: 'edited'));
    await ctrl(tester, LogicalKeyboardKey.keyV);
    await tester.pump();
    expect(commands.canUndo, isTrue);
    // Deliberately retain the same widget identity, exercising didUpdateWidget.
    await tester.pumpWidget(harness(masked, readOnly: true, commands: commands));
    await tester.pumpAndSettle();
    expect(commands.canUndo, isFalse);
    commands.undo();
    final semantics = tester.ensureSemantics();
    final field = find.byKey(const ValueKey('automation-editor'));
    expect(tester.widget<TextField>(field).readOnly, isTrue);
    expect(tester.widget<TextField>(field).controller!.text, masked.text);
    expect(tester.getSemantics(find.byType(EditableText)).getSemanticsData().value, isNot(contains(secret)));
    await tester.tap(field);
    await ctrl(tester, LogicalKeyboardKey.keyA);
    await ctrl(tester, LogicalKeyboardKey.keyC);
    expect((await Clipboard.getData(Clipboard.kTextPlain))!.text, masked.text);
    await ctrl(tester, LogicalKeyboardKey.keyX);
    await Clipboard.setData(const ClipboardData(text: 'must not replace'));
    await ctrl(tester, LogicalKeyboardKey.keyV);
    await ctrl(tester, LogicalKeyboardKey.keyZ);
    expect(masked.text, contains('•'));
    expect(raw.text, 'edited');
    semantics.dispose();
    protectedClipboard?.dispose();
    await tester.pumpWidget(const SizedBox());
  });
}
