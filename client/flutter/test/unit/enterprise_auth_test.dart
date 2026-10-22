import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/src/rust/api/error.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  group('capabilities.auth', () {
    test('absent or standard → standard', () {
      expect(serverAuthFromJson(null).isEnterprise, isFalse);
      final info = serverInfoFromJson({'server_version': '0.1.16', 'protocol_version': '1.7'});
      expect(info.auth.mode, ServerAuthMode.standard);
      expect(info.auth.method, SignInMethod.email);
    });

    test('enterprise OIDC / LDAP with display name', () {
      final info = serverInfoFromJson({
        'server_version': '0.2.0',
        'protocol_version': '1.8',
        'capabilities': {
          'device_protection': true,
          'auth': {'mode': 'enterprise', 'provider': 'oidc', 'display_name': ' Acme SSO '},
        },
      });
      expect(info.auth.isOidc, isTrue);
      expect(info.auth.displayName, 'Acme SSO');
      expect(info.auth.method, SignInMethod.oidc);
      final ldap = serverAuthFromJson({'mode': 'enterprise', 'provider': 'ldap'});
      expect(ldap.isLdap, isTrue);
      expect(ldap.displayName, isNull);
    });

    test('unknown modes and providers ask for a newer app', () {
      expect(serverAuthFromJson({'mode': 'passkeys'}).needsNewerApp, isTrue);
      expect(serverAuthFromJson({'mode': 'enterprise', 'provider': 'saml'}).needsNewerApp, isTrue);
      expect(serverAuthFromJson({'mode': 'enterprise', 'provider': 'oidc'}).needsNewerApp, isFalse);
    });
  });

  test('sign-in hints round-trip without secrets and ignore a trailing slash', () {
    final at = DateTime.utc(2026, 10, 22, 9, 30);
    final hints = SignInHints.empty.remember(
      SignInHint(
        serverUrl: Uri.parse('https://Sync.Acme.test/'),
        method: SignInMethod.ldap,
        displayName: 'ACME',
        login: r'ACME\jdoe',
        at: at,
      ),
      profileId: const ProfileId('p1'),
    );
    final back = SignInHints.fromJson(hints.toJson());
    final h = back.forServer(Uri.parse('https://sync.acme.test'))!;
    expect(h.method, SignInMethod.ldap);
    expect(h.login, r'ACME\jdoe');
    expect(h.at, at);
    expect(back.forProfile(const ProfileId('p1'))?.displayName, 'ACME');
    expect(hints.toJson().toString(), isNot(contains('password')));
    expect(back.forgetServer(Uri.parse('https://sync.acme.test')).forServer(Uri.parse('https://sync.acme.test')), isNull);
  });

  test('enterprise error codes map to localized messages', () {
    final en = lookupAppLocalizations(const Locale('en'));
    final ru = lookupAppLocalizations(const Locale('ru'));
    AppException server(String code) =>
        mapBridgeError(BridgeError(code: 'server', message: 'x', details: {'protocol_code': code}));
    expect(mapBridgeError(const BridgeError(code: 'account_disabled', message: 'x', details: {})).code,
        AppErrorCode.accountDisabled);
    final cases = {
      'enterprise_auth_required': AppErrorCode.enterpriseAuthRequired,
      'identity_conflict': AppErrorCode.identityConflict,
      'domain_not_allowed': AppErrorCode.domainNotAllowed,
      'group_required': AppErrorCode.groupRequired,
      'mfa_required': AppErrorCode.mfaRequired,
      'idp_unavailable': AppErrorCode.idpUnavailable,
      'account_disabled': AppErrorCode.accountDisabled,
    };
    for (final e in cases.entries) {
      final error = server(e.key);
      expect(error.code, e.value, reason: e.key);
      expect(errorMessage(en, error), isNot(en.errorInternal), reason: e.key);
      expect(errorMessage(ru, error), isNot(ru.errorInternal), reason: e.key);
    }
    expect(errorMessage(ru, server('account_disabled')), ru.errorAccountDisabled);
    // LDAP lockout: rate limited with Retry-After.
    final limited = mapBridgeError(
      const BridgeError(code: 'rate_limited', message: 'x', details: {'retry_after_seconds': '30'}),
    );
    expect(errorMessage(en, limited), en.errorRateLimitedRetry(30));
    final callback = mapBridgeError(
      const BridgeError(
        code: 'invalid_input',
        message: 'x',
        reason: 'enterprise_callback_invalid',
        details: {'field': 'enterprise_auth', 'rule': 'invalid_callback'},
      ),
    );
    expect(errorMessage(en, callback), en.errorReasonEnterpriseCallbackInvalid);
  });
}
