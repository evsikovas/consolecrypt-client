import 'dart:io';

import 'package:consolecrypt/core/widgets/secret_selectable_text.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

/// S8/S9, ТР-14/15: every clipboard write goes through `SecureClipboard`
/// (`copySecret` / `copyPrivate` / `copyPlain`), and revealed secrets shown
/// as selectable text have no standard (plain, unprotected) Copy.
void main() {
  List<(String, int, String)> lines(String root) => [
    for (final file in Directory(root).listSync(recursive: true).whereType<File>())
      if (file.path.endsWith('.dart') && !file.path.contains('/l10n/') && !file.path.contains('/src/rust/'))
        for (final (index, line) in file.readAsLinesSync().indexed)
          if (!line.trimLeft().startsWith('//')) (file.path, index + 1, line),
  ];

  test('no direct clipboard writes outside SecureClipboard', () {
    // The single allowed place: copyPlain's own write.
    const allowed = {'lib/core/security/secure_clipboard.dart'};
    final write = RegExp(r'\bClipboard\.setData\b');
    final offenders = [
      for (final (path, line, text) in [...lines('lib'), ...lines('vendor/xterm/lib')])
        if (write.hasMatch(text) && !allowed.contains(path)) '$path:$line: ${text.trim()}',
    ];
    expect(
      offenders,
      isEmpty,
      reason: 'Use copySecretWithNotice / copyPrivateWithNotice / copyPlainWithNotice:\n${offenders.join('\n')}',
    );
  });

  test('the terminal copy shortcut is routed through the app', () {
    final pane = File('lib/terminal/terminal_pane.dart').readAsStringSync();
    expect(pane, contains('onCopy: (text) => copyTerminalSelection(context, ref, text)'));
    final actions = File('vendor/xterm/lib/src/ui/shortcut/actions.dart').readAsStringSync();
    expect(actions, contains('await onCopy?.call(text);'));
  });

  test('dialogs that reveal secrets show no plain SelectableText', () {
    // Revealed values here must use SecretSelectableText (protected Copy).
    const secretViews = [
      'lib/kubernetes/kube_secret_dialog.dart',
      'lib/db/db_value_dialog.dart',
      'lib/db/db_tree.dart',
    ];
    final plain = RegExp(r'(?<![A-Za-z])SelectableText(\.rich)?\(');
    final offenders = [
      for (final path in secretViews)
        for (final (index, line) in File(path).readAsLinesSync().indexed)
          if (plain.hasMatch(line)) '$path:${index + 1}: ${line.trim()}',
    ];
    expect(offenders, isEmpty);
    // Revealed Docker environment and inspect JSON switch to the protected
    // widget while shown unmasked.
    final docker = File('lib/docker/docker_container_details.dart').readAsStringSync();
    expect(
      docker,
      contains('''_revealed != null
                          ? SecretSelectableText('''),
    );
    expect(docker, contains('onCopySecret: full == null'));
  });

  testWidgets('SecretSelectableText copies through its callback, never the plain clipboard', (tester) async {
    var platformWrites = 0;
    tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(SystemChannels.platform, (call) async {
      if (call.method == 'Clipboard.setData') platformWrites++;
      return null;
    });
    addTearDown(() => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(SystemChannels.platform, null));
    final copied = <String>[];
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(body: SecretSelectableText('token=s3cr3t-value', onCopy: (text) async => copied.add(text))),
      ),
    );
    // Select all, then the platform copy shortcut.
    await tester.tap(find.byType(SelectableText));
    await tester.pump();
    final control = defaultTargetPlatform == TargetPlatform.macOS
        ? LogicalKeyboardKey.metaLeft
        : LogicalKeyboardKey.controlLeft;
    await tester.sendKeyDownEvent(control);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyA);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyC);
    await tester.sendKeyUpEvent(control);
    await tester.pump();
    expect(copied, ['token=s3cr3t-value']);

    // The context menu's Copy takes the same path; no Cut/Share.
    if (defaultTargetPlatform == TargetPlatform.android) {
      await tester.longPress(find.byType(SelectableText));
    } else {
      await tester.tap(find.byType(SelectableText), buttons: kSecondaryMouseButton, kind: PointerDeviceKind.mouse);
    }
    await tester.pumpAndSettle();
    expect(find.text('Cut'), findsNothing);
    expect(find.text('Share'), findsNothing);
    await tester.tap(find.text('Copy').first);
    await tester.pump();
    expect(copied, hasLength(2));
    expect(copied.last, isNotEmpty);
    expect(platformWrites, 0);
  }, variant: const TargetPlatformVariant({TargetPlatform.android, TargetPlatform.macOS, TargetPlatform.windows}));
}
