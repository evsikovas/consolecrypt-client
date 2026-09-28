import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

const _branches = [
  'hosts',
  'groups',
  'credentials',
  'knownHosts',
  'terminal',
  'sftp',
  'tunnels',
  'snippets',
  'ai',
  'sync',
  'backups',
  'settings',
];

Future<void> _pumpSized(WidgetTester tester, MockBackend backend, Size size, {AppLocale locale = AppLocale.en}) =>
    pumpApp(tester, backend, size: size, locale: locale);

/// Visits every shell screen; any overflow / build error fails the test.
/// The test font (Ahem) is wider than real fonts, so this is conservative.
/// Russian runs at 1024 px catch strings that are too long for the layout.
void main() {
  for (final (size, locale) in const [
    (Size(1600, 1000), AppLocale.en),
    (Size(1024, 720), AppLocale.en),
    (Size(1024, 720), AppLocale.ru),
  ]) {
    testWidgets(
      'every screen renders without layout errors at ${size.width.toInt()}px in ${locale.wireName} (synced demo)',
      (tester) async {
        final backend = testBackend();
        addTearDown(backend.dispose);
        await backend.debugSignInDemoAndUnlock();
        await _pumpSized(tester, backend, size, locale: locale);
        for (final branch in [..._branches, 'devices']) {
          await tapKey(tester, 'nav-$branch');
          expect(tester.takeException(), isNull, reason: branch);
        }
        await tapKey(tester, 'nav-hosts');
        await tapKey(tester, 'add-host');
        expect(tester.takeException(), isNull, reason: 'host editor');
        // Existing hosts: one with a shared key credential, one inheriting from its group.
        await tapKey(tester, 'nav-hosts');
        await enterKey(tester, 'hosts-search', 'bastion-a');
        await tapKey(tester, 'host-menu-bastion-a');
        await tapKey(tester, 'host-edit-menu-bastion-a');
        if (locale == AppLocale.en) {
          expect(find.text('SSH key · prod-deploy'), findsOneWidget, reason: 'shared key shown as linked');
        }
        expect(tester.takeException(), isNull, reason: 'editor with shared key credential');
        await tapKey(tester, 'nav-hosts');
        await tapKey(tester, 'nav-hosts');
        await enterKey(tester, 'hosts-search', 'prod-db-1');
        await tapKey(tester, 'host-menu-prod-db-1');
        await tapKey(tester, 'host-edit-menu-prod-db-1');
        expect(tester.takeException(), isNull, reason: 'editor inheriting from group');
      },
      variant: TargetPlatformVariant.only(TargetPlatform.windows),
    );
  }

  for (final locale in const [AppLocale.en, AppLocale.ru]) {
    testWidgets('local profile screens render at compact width in ${locale.wireName}; Devices hidden', (tester) async {
      final backend = testBackend();
      addTearDown(backend.dispose);
      await backend.debugCreateUnlockedLocalProfile();
      await _pumpSized(tester, backend, const Size(1024, 720), locale: locale);
      for (final branch in _branches) {
        await tapKey(tester, 'nav-$branch');
        expect(tester.takeException(), isNull, reason: branch);
      }
      expect(find.byKey(const ValueKey('nav-devices')), findsNothing);
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
  }

  for (final (size, locale) in const [(Size(1280, 800), AppLocale.en), (Size(1024, 720), AppLocale.ru)]) {
    testWidgets('dialogs render without layout errors (${locale.wireName}, ${size.width.toInt()}px)', (tester) async {
      final backend = testBackend();
      addTearDown(backend.dispose);
      await backend.debugSignInDemoAndUnlock();
      await _pumpSized(tester, backend, size, locale: locale);
      final l10n = lookupAppLocalizations(Locale(locale.wireName));

      Future<void> openAndClose(String label, Future<void> Function() open) async {
        await open();
        expect(tester.takeException(), isNull, reason: label);
        expect(findDialog(), findsOneWidget, reason: label);
        await tester.tap(find.text(l10n.commonCancel).last);
        await settle(tester);
      }

      await tapKey(tester, 'nav-credentials');
      for (final item in ['new-password', 'new-generate', 'new-import']) {
        await openAndClose(item, () async {
          await tapKey(tester, 'add-credential');
          await tapKey(tester, item);
        });
      }

      await tapKey(tester, 'nav-tunnels');
      await openAndClose('tunnel editor', () async {
        await tapKey(tester, 'add-tunnel');
        await enterKey(tester, 'tunnel-bind-host', '0.0.0.0');
        await settle(tester);
        expect(find.byKey(const ValueKey('public-bind-warning')), findsOneWidget);
        expect(isEnabled(tester, 'save-tunnel'), isFalse, reason: 'public bind needs acknowledgment');
      });

      await tapKey(tester, 'nav-snippets');
      await openAndClose('snippet editor', () => tapKey(tester, 'add-snippet'));

      await tapKey(tester, 'nav-groups');
      await openAndClose('group dialog', () => tapKey(tester, 'add-group'));

      await tapKey(tester, 'nav-settings');
      await openAndClose('provider dialog', () => tapKey(tester, 'add-provider'));
      await openAndClose('change passphrase', () async {
        final change = find.text(locale == AppLocale.en ? 'Change passphrase…' : l10n.settingsChangePassphrase);
        await tester.ensureVisible(change);
        await tester.tap(change);
        await settle(tester);
      });
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
  }
}
