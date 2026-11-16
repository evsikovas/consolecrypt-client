import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/automation/automation_wsl_preparation.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

void main() {
  for (final language in ['en', 'ru']) {
    testWidgets('WSL guide $language stays usable in a small window and only copies on explicit action', (tester) async {
      tester.view.physicalSize = const Size(470, 560);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);
      final copied = <String>[];
      tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(SystemChannels.platform, (call) async {
        if (call.method == 'Clipboard.setData') copied.add((call.arguments as Map)['text'] as String);
        return null;
      });
      addTearDown(() => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(SystemChannels.platform, null));
      await tester.pumpWidget(ProviderScope(child: MaterialApp(
        locale: Locale(language),
        theme: AppTheme.dark(platform: TargetPlatform.windows),
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        supportedLocales: AppLocalizations.supportedLocales,
        home: Builder(builder: (context) => Scaffold(body: TextButton(
          onPressed: () => showDialog<void>(context: context, builder: (_) => const AutomationWslPreparation()),
          child: const Text('OPEN'),
        ))),
      )));
      await tester.tap(find.text('OPEN'));
      await tester.pumpAndSettle();
      expect(copied, isEmpty);
      expect(tester.takeException(), isNull);
      final install = find.byKey(const ValueKey('wsl-install-copy'));
      await tester.ensureVisible(install);
      await tester.pumpAndSettle();
      await tester.tap(install);
      await tester.pumpAndSettle();
      expect(copied, ['wsl.exe --install --distribution Ubuntu-24.04']);
      expect(copied.single, isNot(contains('--set-default')));
      expect(tester.takeException(), isNull);
      await tester.ensureVisible(find.text(language == 'ru' ? '4. Вернуться и проверить окружение' : '4. Return and check the environment'));
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
      await tester.tap(find.text(language == 'ru' ? 'Закрыть' : 'Close'));
      await tester.pumpAndSettle();
      expect(find.byType(AutomationWslPreparation), findsNothing);
      expect(copied, hasLength(1));
    });
  }
}
