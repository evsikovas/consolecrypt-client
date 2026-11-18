import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/automation/ansible_code_editor.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

Future<void> key(WidgetTester tester, LogicalKeyboardKey code, {bool meta = false, bool shift = false}) async {
  final modifier = meta ? LogicalKeyboardKey.metaLeft : LogicalKeyboardKey.controlLeft;
  await tester.sendKeyDownEvent(modifier);
  if (shift) await tester.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
  await tester.sendKeyEvent(code, physicalKey: code == LogicalKeyboardKey.add ? PhysicalKeyboardKey.equal : null);
  if (shift) await tester.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
  await tester.sendKeyUpEvent(modifier);
  await tester.pump();
}

void main() {
  for (final meta in [false, true]) {
    testWidgets('${meta ? "Cmd" : "Ctrl"} plus/equal/minus zoom changes display only and keeps gutters aligned', (
      tester,
    ) async {
      final source = TextEditingController(text: '- name: Test\n  ansible.builtin.debug:\n    msg: hello\n');
      addTearDown(source.dispose);
      await tester.pumpWidget(
        MaterialApp(
          theme: AppTheme.dark(platform: TargetPlatform.windows),
          supportedLocales: AppLocalizations.supportedLocales,
          localizationsDelegates: AppLocalizations.localizationsDelegates,
          home: Scaffold(body: AnsibleCodeEditor(controller: source)),
        ),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('automation-editor')));
      await tester.pump();
      final before = source.value;
      double size() => tester.widget<TextField>(find.byKey(const ValueKey('automation-editor'))).style!.fontSize!;
      final lineBefore = tester.getSize(find.byKey(const ValueKey('ansible-line-0'))).height;
      expect(size(), 13);
      await key(tester, LogicalKeyboardKey.equal, meta: meta);
      expect(size(), 14);
      await key(tester, LogicalKeyboardKey.equal, meta: meta, shift: true);
      expect(size(), 15);
      await key(tester, LogicalKeyboardKey.add, meta: meta);
      expect(size(), 16);
      await key(tester, LogicalKeyboardKey.minus, meta: meta);
      expect(size(), 15);
      expect(tester.getSize(find.byKey(const ValueKey('ansible-line-0'))).height, greaterThan(lineBefore));
      expect(source.value, before, reason: 'Zoom never changes source, cursor, undo or dirty state');
      expect(tester.takeException(), isNull);
    });
  }

  testWidgets('read-only editor zoom is bounded and does not edit text', (tester) async {
    final source = TextEditingController(text: 'read only');
    addTearDown(source.dispose);
    await tester.pumpWidget(
      MaterialApp(
        theme: AppTheme.dark(platform: TargetPlatform.windows),
        supportedLocales: AppLocalizations.supportedLocales,
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        home: Scaffold(body: AnsibleCodeEditor(controller: source, readOnly: true)),
      ),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('automation-editor')));
    for (var i = 0; i < 30; i++) {
      await key(tester, LogicalKeyboardKey.numpadAdd);
    }
    expect(tester.widget<TextField>(find.byKey(const ValueKey('automation-editor'))).style!.fontSize, 32);
    for (var i = 0; i < 30; i++) {
      await key(tester, LogicalKeyboardKey.numpadSubtract);
    }
    expect(tester.widget<TextField>(find.byKey(const ValueKey('automation-editor'))).style!.fontSize, 8);
    expect(source.text, 'read only');
    expect(tester.takeException(), isNull);
  });
}
