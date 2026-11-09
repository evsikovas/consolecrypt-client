import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

void main() {
  testWidgets('settings headings collapse independently and preserve values when reopened', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend, locale: AppLocale.ru);
    await tapKey(tester, 'nav-settings');
    final heading = find.text('Безопасность');
    await tester.ensureVisible(heading);
    await settle(tester);
    final before = backend.services.settings.currentLocal.clipboardClearSeconds;
    expect(find.byKey(const ValueKey('clipboard-clear')), findsOneWidget);
    await tester.tap(heading);
    await settle(tester);
    expect(find.byKey(const ValueKey('clipboard-clear')), findsNothing);
    expect(backend.services.settings.currentLocal.clipboardClearSeconds, before);
    await tester.tap(heading);
    await settle(tester);
    expect(find.byKey(const ValueKey('clipboard-clear')), findsOneWidget);
    final sftp = find.text('Редактор по умолчанию для SFTP');
    await tester.ensureVisible(sftp);
    await settle(tester);
    await tester.tap(sftp);
    await settle(tester);
    expect(find.byKey(const ValueKey('sftp-default-editor-choose')), findsNothing);
    await tester.tap(sftp);
    await settle(tester);
    expect(find.byKey(const ValueKey('sftp-default-editor-choose')), findsOneWidget);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('collapsed section keeps an unsaved text field mounted', (tester) async {
    final controller = TextEditingController(text: 'draft');
    addTearDown(controller.dispose);
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: SectionCardCollapseScope(
            child: SectionCard(
              title: 'Heading',
              child: TextField(key: const ValueKey('draft'), controller: controller),
            ),
          ),
        ),
      ),
    );
    await tester.enterText(find.byKey(const ValueKey('draft')), 'unsaved draft');
    await tester.tap(find.text('Heading'));
    await tester.pump();
    expect(find.byKey(const ValueKey('draft')), findsNothing);
    await tester.tap(find.text('Heading'));
    await tester.pump();
    expect(find.text('unsaved draft'), findsOneWidget);
  });
}
