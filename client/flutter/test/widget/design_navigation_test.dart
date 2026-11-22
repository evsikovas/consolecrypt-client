import 'package:consolecrypt/core/models/models.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_ui_proof.dart';
import '../helpers/test_app.dart';

void main() {
  testWidgets('host details open on demand and keep actions aligned', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    tester.binding.platformDispatcher.platformBrightnessTestValue = Brightness.dark;
    addTearDown(tester.binding.platformDispatcher.clearPlatformBrightnessTestValue);
    await loadAutomationProofFonts(tester);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend, locale: AppLocale.ru, captureBoundary: true);
    final inspector = find.byKey(const ValueKey('host-inspector'));
    final select = find.byKey(const ValueKey('inspect-prod-web-1'));
    expect(inspector, findsNothing);
    await captureAutomationWorkspace(tester, 'design-hosts-windows');
    await enterKey(tester, 'hosts-search', 'prod-web');
    expect(select, findsOneWidget);
    await tester.ensureVisible(select);
    await tapKey(tester, 'inspect-prod-web-1');
    expect(find.descendant(of: inspector, matching: find.text('prod-web-1')), findsOneWidget);
    expect(find.byKey(const ValueKey('connect-prod-web-1')), findsOneWidget);
    expect(tester.getRect(inspector).left, greaterThan(tester.getRect(select).right));
    final connect = tester.getRect(find.byKey(const ValueKey('host-inspector-connect')));
    final sftp = tester.getRect(find.byKey(const ValueKey('host-inspector-sftp')));
    final edit = tester.getRect(find.byKey(const ValueKey('host-inspector-edit')));
    expect(connect.left, sftp.left);
    expect(connect.right, edit.right);
    expect(sftp.top, edit.top);
    expect(sftp.width, closeTo(edit.width, 0.01));
    await captureAutomationWorkspace(tester, 'design-host-inspector-windows');
    await tapKey(tester, 'host-inspector-close');
    expect(inspector, findsNothing);
    await tapKey(tester, 'nav-passwords');
    expect(find.byKey(const ValueKey('keepass-open')), findsOneWidget);
    await captureAutomationWorkspace(tester, 'design-passwords-windows');
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('settings index brings the selected section into view', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    tester.binding.platformDispatcher.platformBrightnessTestValue = Brightness.dark;
    addTearDown(tester.binding.platformDispatcher.clearPlatformBrightnessTestValue);
    await loadAutomationProofFonts(tester);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend, locale: AppLocale.ru, captureBoundary: true);
    await tapKey(tester, 'nav-settings');
    await tapKey(tester, 'settings-section-2');
    expect(find.text('Оформление').hitTestable(), findsNWidgets(2));
    await captureAutomationWorkspace(tester, 'design-settings-windows');
    await tapKey(tester, 'settings-section-6');
    expect(find.text('О программе').hitTestable(), findsNWidgets(2));
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
}
