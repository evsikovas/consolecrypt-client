import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/automation/automation_windows_prerequisites.dart';
import 'package:consolecrypt/automation/automation_wsl_prerequisite_wizard.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:crypto/crypto.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

final original = RuntimeNoticeScope.fromProfile({
  'id': '01a1d609-8ca4-7782-bbae-cb76828e040f',
  'active': true,
  'vault_state': 'unlocked',
  'notification_scope_id': '607f4b96-0ee4-406a-b576-0e30545c10ba',
})!;

class NativeCapture
    implements
        AutomationWindowsPrerequisiteService,
        AutomationRunnerSetupService {
  NativeCapture(this.value);
  final AutomationJson value;
  int reads = 0;
  @override
  Future<AutomationJson> inspectWindowsPrerequisites({
    required String operationId,
    required String profileId,
    required String notificationScopeId,
  }) async {
    expect(profileId, original.profileId);
    expect(notificationScopeId, original.nonce);
    reads++;
    return value;
  }

  @override
  Future<void> cancelRunnerSetup(String operationId) async {}
  @override
  Future<AutomationJson> setupRunner(
    AutomationJson target, {
    required String operationId,
    bool install = false,
  }) => throw StateError('No Linux request in Windows capture test');
}

void main() {
  testWidgets(
    'untouched actual Native Windows output parses and renders original unknown features and observed reboot indicator',
    (tester) async {
      final bytes = File('test/fixtures/windows_prerequisites_native.json')
          .readAsBytesSync();
      expect(
        sha256.convert(bytes).toString(),
        '5edabff5992fcc1abf48c8c5cdf50328654f5f65ecbb83c862f9ce6e3a88c559',
      );
      final value = jsonDecode(utf8.decode(bytes)) as AutomationJson,
          port = NativeCapture(
            jsonDecode(utf8.decode(bytes)) as AutomationJson,
          );
      final snapshot = WindowsPrerequisiteSnapshot.parse(value, original);
      expect(snapshot.elevation, WindowsElevationObservation.notElevated);
      expect(snapshot.subsystem.state, WindowsFeatureObservation.unknown);
      expect(
        snapshot.virtualMachine.reason,
        WindowsFeatureUnknown.requiresElevation,
      );
      expect(snapshot.reboot, WindowsRebootObservation.indicatorsPresent);
      expect(snapshot.indicators, [WindowsRebootIndicator.pendingFileRename]);
      final foreign = RuntimeNoticeScope.fromProfile({
        'id': original.profileId,
        'active': true,
        'vault_state': 'unlocked',
        'notification_scope_id': 'foreign-original-owner',
      })!;
      expect(
        () => WindowsPrerequisiteSnapshot.parse(value, foreign),
        throwsFormatException,
      );
      await tester.pumpWidget(
        MaterialApp(
          theme: AppTheme.dark(platform: TargetPlatform.windows),
          supportedLocales: AppLocalizations.supportedLocales,
          localizationsDelegates: const [
            AppLocalizations.delegate,
            ...GlobalMaterialLocalizations.delegates,
          ],
          home: Scaffold(
            body: SingleChildScrollView(
              child: AutomationWslPrerequisiteWizard(
                original: () => true,
                windows: port,
                setup: port,
                nativeScope: original,
                discover: () async => const LocalShellProfiles(),
                openTerminal: (_) async {},
              ),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(port.reads, 0);
      final inspect = find.byKey(const ValueKey('wsl-wizard-inspect-windows'));
      await tester.ensureVisible(inspect);
      await tester.pumpAndSettle();
      await tester.tap(inspect);
      await tester.pumpAndSettle();
      expect(port.reads, 1);
      expect(
        find.textContaining('reading this feature requires'),
        findsNWidgets(2),
      );
      expect(
        find.textContaining('They do not identify every restart requirement'),
        findsOneWidget,
      );
      expect(find.textContaining('2026-11-13T17:51:53.274Z'), findsOneWidget);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox.shrink());
    },
  );
}
