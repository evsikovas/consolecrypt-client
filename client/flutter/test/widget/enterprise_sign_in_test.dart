import 'package:consolecrypt/core/mock/mock_cloud.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

/// Enterprise sign-in (protocol 1.8, ТР-10…ТР-13): `/v1/meta` decides the
/// sign-in form, the corporate authorization server is named wherever the
/// account is shown, sign-ins are remembered per server, and a disabled
/// account locks the profile.
void main() {
  final en = lookupAppLocalizations(const Locale('en'));
  final ru = lookupAppLocalizations(const Locale('ru'));
  final windows = TargetPlatformVariant.only(TargetPlatform.windows);

  Finder key(String k) => find.byKey(ValueKey(k));

  Future<void> openLogin(WidgetTester tester, AppLocalizations l) async {
    await tester.tap(find.text(l.welcomeServerAction));
    await settle(tester);
  }

  Future<void> checkServer(WidgetTester tester, AppLocalizations l, String url) async {
    await enterKey(tester, 'server-url', url);
    await tester.tap(find.text(l.loginCheckServer));
    await settle(tester);
  }

  testWidgets('a standard server keeps e-mail, password and registration', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await pumpApp(tester, backend);
    await openLogin(tester, en);
    await checkServer(tester, en, 'https://sync.example.org');
    expect(key('email'), findsOneWidget);
    expect(key('password'), findsOneWidget);
    expect(key('login-mode'), findsOneWidget);
    expect(find.text(en.loginForgotPassword), findsOneWidget);
    expect(key('corporate-auth-notice'), findsNothing);
    expect(key('oidc-form'), findsNothing);
    expect(key('ldap-form'), findsNothing);
  }, variant: windows);

  testWidgets('an OIDC server shows the SSO button and the corporate notice, then onboarding', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await pumpApp(tester, backend);
    await openLogin(tester, en);
    await checkServer(tester, en, 'https://sso.example.org');

    expect(key('corporate-auth-notice'), findsOneWidget);
    expect(find.text(en.corporateAuthNotice(MockCloud.ssoDisplayName)), findsOneWidget);
    expect(find.text(en.loginSsoButton(MockCloud.ssoDisplayName)), findsOneWidget);
    expect(key('oidc-login-hint'), findsOneWidget);
    // No account password, registration or "forgot password".
    expect(key('email'), findsNothing);
    expect(key('password'), findsNothing);
    expect(key('login-mode'), findsNothing);
    expect(find.text(en.loginForgotPassword), findsNothing);

    await enterKey(tester, 'oidc-login-hint', 'ann@acme.test');
    await tapKey(tester, 'oidc-submit');
    // A new account continues with the normal vault onboarding.
    expect(key('new-passphrase'), findsOneWidget);
    expect(backend.profiles.currentProfiles.active?.accountEmail, 'ann@acme.test');
    final hint = backend.auth.currentSignInHints.forServer(Uri.parse('https://sso.example.org'));
    expect(hint?.method, SignInMethod.oidc);
    expect(hint?.login, 'ann@acme.test');
    expect(hint?.displayName, MockCloud.ssoDisplayName);
  }, variant: windows);

  testWidgets('an LDAP server asks for the directory login and wipes the password field', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await pumpApp(tester, backend, locale: AppLocale.ru);
    await openLogin(tester, ru);
    await checkServer(tester, ru, 'https://ldap.example.org');

    expect(find.text(ru.loginLdapCaption), findsOneWidget);
    expect(find.text(ru.loginLdapLoginLabel), findsOneWidget);
    expect(key('corporate-auth-notice'), findsOneWidget);
    expect(key('login-mode'), findsNothing);
    expect(find.text(ru.loginForgotPassword), findsNothing);

    await enterKey(tester, 'ldap-login', r'ACME\jdoe');
    await tester.enterText(find.descendant(of: key('ldap-password'), matching: find.byType(TextField)), 'wrong');
    await tapKey(tester, 'ldap-submit');
    expect(key('enterprise-error'), findsOneWidget);
    expect(find.text(ru.errorInvalidCredentials), findsOneWidget);
    final field = tester.widget<TextField>(find.descendant(of: key('ldap-password'), matching: find.byType(TextField)));
    expect(field.controller!.text, isEmpty, reason: 'the directory password is never kept');

    await tester.enterText(
      find.descendant(of: key('ldap-password'), matching: find.byType(TextField)),
      MockCloud.demoPassword,
    );
    await tapKey(tester, 'ldap-submit');
    expect(key('new-passphrase'), findsOneWidget);
    expect(backend.profiles.currentProfiles.active?.accountEmail, 'jdoe@acme.test');
  }, variant: windows);

  testWidgets('a remembered sign-in is preselected, shown and can be forgotten', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    final server = Uri.parse('https://ldap.example.org');
    await backend.auth.rememberSignIn(
      serverUrl: server,
      method: SignInMethod.ldap,
      displayName: 'ACME',
      login: r'ACME\jdoe',
    );
    await pumpApp(tester, backend);
    await openLogin(tester, en);

    // Recent servers carry the method badge.
    expect(key('recent-servers'), findsOneWidget);
    expect(find.text(en.authBadgeAd), findsOneWidget);
    await tapKey(tester, 'recent-server-ldap.example.org');

    expect(key('sign-in-hint'), findsOneWidget);
    expect(find.text(en.signInLastTime(en.signInMethodLdap('ACME'), r'ACME\jdoe')), findsOneWidget);
    final login = tester.widget<TextField>(key('ldap-login'));
    expect(login.controller!.text, r'ACME\jdoe');
    expect(key('auth-mode-changed'), findsNothing);

    await tapKey(tester, 'forget-sign-in-hint');
    expect(key('sign-in-hint'), findsNothing);
    expect(backend.auth.currentSignInHints.forServer(server), isNull);
  }, variant: windows);

  testWidgets('a server that switched to corporate sign-in says so', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.auth.rememberSignIn(
      serverUrl: Uri.parse('https://sso.example.org'),
      method: SignInMethod.email,
      login: 'old@acme.test',
    );
    await pumpApp(tester, backend, locale: AppLocale.ru);
    await openLogin(tester, ru);
    await checkServer(tester, ru, 'https://sso.example.org');
    expect(key('auth-mode-changed'), findsOneWidget);
    expect(find.text(ru.authModeChangedToEnterprise(MockCloud.ssoDisplayName)), findsOneWidget);
    expect(find.text(ru.signInLastTime(ru.signInMethodEmail, 'old@acme.test')), findsOneWidget);
  }, variant: windows);

  testWidgets('settings, sync and devices name the corporate authorization server', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);

    // Standard servers: e-mail and account password.
    await tapKey(tester, 'nav-settings');
    expect(key('settings-auth-row'), findsOneWidget);
    expect(find.text(en.settingsAuthStandard), findsOneWidget);
    expect(find.text(en.settingsChangeAccountPassword), findsOneWidget);

    final profile = backend.profiles.currentProfiles.active!;
    await backend.auth.rememberSignIn(
      serverUrl: profile.serverUrl!,
      method: SignInMethod.oidc,
      displayName: MockCloud.ssoDisplayName,
      login: MockCloud.demoEmail,
      profileId: profile.id,
    );
    await settle(tester);
    expect(find.text(en.settingsAuthCorporate(MockCloud.ssoDisplayName, en.authProviderOidc)), findsOneWidget);
    expect(find.text(en.settingsAuthCorporateHelp), findsOneWidget);
    expect(key('authorization-signed-in'), findsOneWidget);
    // Corporate accounts have no account password to change.
    expect(find.text(en.settingsChangeAccountPassword), findsNothing);

    await tapKey(tester, 'nav-sync');
    expect(key('sync-auth-row'), findsOneWidget);
    expect(find.text(en.settingsAuthCorporate(MockCloud.ssoDisplayName, en.authProviderOidc)), findsOneWidget);

    await tapKey(tester, 'nav-devices');
    expect(key('devices-auth-notice'), findsOneWidget);
    expect(find.text(en.devicesAuthCorporate(MockCloud.ssoDisplayName)), findsOneWidget);
  }, variant: windows);

  testWidgets('a disabled account locks the profile; it can be deleted', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend, locale: AppLocale.ru);
    backend.cloud.disableAccount(MockCloud.demoEmail);
    await settle(tester);

    expect(key('account-disabled-screen'), findsOneWidget);
    expect(find.text(ru.accountDisabledTitle), findsOneWidget);
    expect(find.text(ru.accountDisabledBody('sync.example.org')), findsOneWidget);
    expect(backend.vault.currentStatus.phase, isNot(VaultPhase.unlocked));

    final id = backend.profiles.currentProfiles.activeId;
    await tapKey(tester, 'account-disabled-delete');
    await tapKey(tester, 'confirm-ok');
    expect(backend.profiles.currentProfiles.profiles.where((p) => p.id == id), isEmpty);
    expect(key('account-disabled-screen'), findsNothing);
  }, variant: windows);

  testWidgets('"Sign in again" after a disabled account shows the sign-in again', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);
    backend.cloud.disableAccount(MockCloud.demoEmail);
    await settle(tester);
    await tapKey(tester, 'account-disabled-sign-in');
    expect(key('account-disabled-screen'), findsNothing);
    expect(key('server-url'), findsOneWidget);
  }, variant: windows);
}
