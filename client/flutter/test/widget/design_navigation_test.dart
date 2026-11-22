import 'package:consolecrypt/core/glass/glass.dart';
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
    expect(tester.getRect(inspector).top, greaterThan(tester.getRect(select).bottom));
    final listSurface = find
        .ancestor(of: find.byKey(const ValueKey('hosts-search')), matching: find.byType(ContentSurface))
        .first;
    expect(tester.getRect(inspector).top - tester.getRect(listSurface).bottom, greaterThanOrEqualTo(GlassSpacing.s12));
    final connect = tester.getRect(find.byKey(const ValueKey('host-inspector-connect')));
    final sftp = tester.getRect(find.byKey(const ValueKey('host-inspector-sftp')));
    final edit = tester.getRect(find.byKey(const ValueKey('host-inspector-edit')));
    expect(connect.right, lessThanOrEqualTo(sftp.left));
    expect(sftp.right, lessThanOrEqualTo(edit.left));
    expect(sftp.top, edit.top);
    expect(connect.top, sftp.top);
    await captureAutomationWorkspace(tester, 'design-host-inspector-windows');
    await tapKey(tester, 'host-inspector-close');
    expect(inspector, findsNothing);
    await tapKey(tester, 'nav-passwords');
    expect(find.byKey(const ValueKey('keepass-open')), findsOneWidget);
    await captureAutomationWorkspace(tester, 'design-passwords-windows');
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  for (final (width, scale) in [(1420.0, 1.0), (1350.0, 1.4)]) {
    testWidgets('host inspector fields and actions stay separate at ${width}x$scale', (tester) async {
      final backend = testBackend();
      addTearDown(backend.dispose);
      await backend.debugSignInDemoAndUnlock();
      tester.platformDispatcher.textScaleFactorTestValue = scale;
      addTearDown(tester.platformDispatcher.clearTextScaleFactorTestValue);
      await pumpApp(tester, backend, locale: AppLocale.ru, size: Size(width, 820));
      await enterKey(tester, 'hosts-search', 'prod-web');
      await tapKey(tester, 'inspect-prod-web-1');
      final inspector = find.byKey(const ValueKey('host-inspector'));
      final bounds = tester.getRect(inspector);
      final title = find.descendant(of: inspector, matching: find.text('prod-web-1'));
      final rects = [tester.getRect(title)];
      for (final action in ['connect', 'sftp', 'edit', 'close']) {
        final button = find.byKey(ValueKey('host-inspector-$action'));
        expect(button.hitTestable(), findsOneWidget);
        final rect = tester.getRect(button);
        expect(rect.left, greaterThanOrEqualTo(bounds.left));
        expect(rect.right, lessThanOrEqualTo(bounds.right));
        expect(rects.any((previous) => previous.overlaps(rect)), isFalse, reason: action);
        rects.add(rect);
      }
      final values = find.descendant(of: inspector, matching: find.byType(SelectableText));
      expect(values, findsAtLeastNWidgets(3));
      final lastButtonBottom = rects.map((rect) => rect.bottom).reduce((a, b) => a > b ? a : b);
      for (var i = 0; i < values.evaluate().length; i++) {
        expect(tester.getRect(values.at(i)).top, greaterThan(lastButtonBottom));
      }
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
  }

  testWidgets('operational sections fit in desktop and narrow windows', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    tester.binding.platformDispatcher.platformBrightnessTestValue = Brightness.dark;
    addTearDown(tester.binding.platformDispatcher.clearPlatformBrightnessTestValue);
    await loadAutomationProofFonts(tester);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend, locale: AppLocale.ru, captureBoundary: true);
    for (final page in ['tunnels', 'devices', 'sync']) {
      await tapKey(tester, 'nav-$page');
      if (page == 'tunnels') {
        final item = find.byWidgetPredicate(
          (w) => w.key is ValueKey<String> && (w.key! as ValueKey<String>).value.startsWith('tunnel-') && w is InkWell,
        );
        await tester.tap(item.first);
        await settle(tester);
        expect(find.byType(SelectableText), findsWidgets);
      }
      await captureAutomationWorkspace(tester, 'design-$page-windows');
      tester.view.physicalSize = const Size(950, 710);
      await settle(tester);
      expect(tester.takeException(), isNull, reason: page);
      tester.view.physicalSize = const Size(1600, 1000);
      await settle(tester);
    }
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
