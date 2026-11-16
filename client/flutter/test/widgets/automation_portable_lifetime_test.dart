import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/automation/automation_portable_dialog.dart';
import 'package:consolecrypt/automation/automation_portable_lifetime.dart';
import 'package:consolecrypt/automation/automation_portable_native_adapter.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/services/runtime_notice_adapter.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../unit/automation_portable_controller_test.dart' as f;
import '../unit/automation_portable_native_adapter_test.dart' show Gateway;
import 'automation_portable_dialog_test.dart' show labels;

void main() {
  testWidgets(
    'same-profile boundary closes only original dialog and clears typed password while coarse AutomationScope stays identical',
    (tester) async {
      final events = RuntimeNoticeEventAdapter(), gateway = Gateway();
      addTearDown(events.dispose);
      final lifetime = PortableUiLifetime(profileId: f.profile, events: events, isOriginalUiCurrent: () => true);
      final adapter = await AutomationPortableNativeAdapter.capture(lifetime: lifetime, gateway: gateway);
      addTearDown(adapter.dispose);
      final files = f.Files(), controller = adapter.createController(files: files);
      const original = AutomationScope(f.profile, true);
      await tester.pumpWidget(
        ProviderScope(
          overrides: [automationScopeProvider.overrideWithValue(original)],
          child: MaterialApp(
            theme: AppTheme.dark(platform: TargetPlatform.windows),
            supportedLocales: AppLocalizations.supportedLocales,
            localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
            home: Scaffold(
              body: Builder(
                builder: (context) => TextButton(
                  onPressed: () => showDialog<void>(
                    context: context,
                    builder: (_) => AutomationPortableDialog(
                      controller: controller,
                      originalScope: original,
                      labels: labels,
                      importMode: true,
                    ),
                  ),
                  child: const Text('open'),
                ),
              ),
            ),
          ),
        ),
      );
      await tester.tap(find.text('open'));
      await tester.pumpAndSettle();
      final field = tester.widget<EditableText>(
        find.descendant(of: find.byKey(const ValueKey('portable-password')), matching: find.byType(EditableText)),
      );
      await tester.enterText(
        find.descendant(of: find.byKey(const ValueKey('portable-password')), matching: find.byType(EditableText)),
        'synthetic typed package password',
      );
      events.boundary();
      expect(lifetime.current, isFalse);
      await tester.pump();
      await tester.pumpAndSettle();
      expect(field.controller.text, isEmpty);
      expect(find.byType(AutomationPortableDialog), findsNothing);
      expect(find.text('open'), findsOneWidget);
      expect(gateway.calls, isEmpty);
      expect(files.opens, 0);
      expect(files.saves, 0);
      expect(tester.takeException(), isNull);
    },
  );
}
