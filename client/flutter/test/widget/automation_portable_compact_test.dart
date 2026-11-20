import 'package:consolecrypt/automation/automation_portable_dialog.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_ui_proof.dart';
import '../helpers/test_app.dart';
import 'automation_portable_menu_test.dart' as m;

void main() {
  for (final width in [1400.0, 500.0]) {
    testWidgets('compact encrypted package export keeps8px password gap at $width', (tester) async {
      if (width == 500) tester.platformDispatcher.textScaleFactorTestValue = 1.5;
      addTearDown(tester.platformDispatcher.clearTextScaleFactorTestValue);
      final (_, _, gateway, files) = await m.open(tester, locale: AppLocale.ru, width: width);
      await m.exportMenu(tester);
      final l = tester.element(find.byType(AutomationPortableDialog)).l10n;
      expect(find.text(l.automationPortableExportTitle), findsOneWidget);
      expect(find.text(l.automationPortableExport), findsOneWidget);
      final password = tester.getRect(find.byKey(const ValueKey('portable-password'))),
          confirm = tester.getRect(find.byKey(const ValueKey('portable-confirm-password')));
      expect(confirm.top - password.bottom, greaterThanOrEqualTo(8));
      expect(gateway.calls.map((c) => c.kind), ['context']);
      expect(files.saves, 0);
      expect(tester.takeException(), isNull);
      await captureAutomationDialog(tester, 'portable-compact-export-ru-${width.toInt()}');
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
  }
  testWidgets(
    'compact inspect remains clearly encrypted and readonly with no confirmation password or file auto-pick',
    (tester) async {
      final (_, _, gateway, files) = await m.open(tester, locale: AppLocale.ru, width: 500);
      await tapKey(tester, 'automation-project-actions');
      await tapKey(tester, 'automation-portable-inspect-open');
      final l = tester.element(find.byType(AutomationPortableDialog)).l10n;
      expect(find.text(l.automationPortableInspect), findsOneWidget);
      expect(find.text(l.automationPortableInspectTitle), findsOneWidget);
      expect(find.byKey(const ValueKey('portable-confirm-password')), findsNothing);
      expect(gateway.calls, isEmpty);
      expect(files.opens, 0);
      expect(files.saves, 0);
      expect(tester.takeException(), isNull);
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
  );
}
