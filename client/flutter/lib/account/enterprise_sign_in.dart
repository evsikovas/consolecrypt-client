import 'dart:async';

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/platform/system_browser.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/secret_field.dart';
import 'package:consolecrypt/core/widgets/security_widgets.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

// ---- labels -------------------------------------------------------------------------------

/// The organisation's sign-in name: the server's `display_name`, else a
/// generic name of the provider.
String corporateAuthName(AppLocalizations l, {String? displayName, SignInMethod method = SignInMethod.oidc}) {
  final name = displayName?.trim();
  if (name != null && name.isNotEmpty) return name;
  return method == SignInMethod.ldap ? l.authProviderLdap : l.authProviderSsoFallback;
}

String serverAuthName(AppLocalizations l, ServerAuth auth) =>
    corporateAuthName(l, displayName: auth.displayName, method: auth.method);

/// "e-mail and password" / "Acme SSO (OIDC)" / "ACME (Active Directory)".
String signInMethodLabel(AppLocalizations l, SignInMethod method, String? displayName) => switch (method) {
  SignInMethod.email => l.signInMethodEmail,
  SignInMethod.oidc => l.signInMethodOidc(corporateAuthName(l, displayName: displayName)),
  SignInMethod.ldap => l.signInMethodLdap(corporateAuthName(l, displayName: displayName, method: method)),
};

/// Compact per-server badge: «Email», «SSO: Acme», «AD».
String signInBadgeLabel(AppLocalizations l, SignInMethod method, String? displayName) => switch (method) {
  SignInMethod.email => l.authBadgeEmail,
  SignInMethod.oidc => l.authBadgeSso(corporateAuthName(l, displayName: displayName)),
  SignInMethod.ldap => l.authBadgeAd,
};

/// "Authorization" value: corporate server with its kind, or e-mail and
/// account password.
String authorizationLabel(AppLocalizations l, SignInMethod method, String? displayName) => switch (method) {
  SignInMethod.email => l.settingsAuthStandard,
  SignInMethod.oidc => l.settingsAuthCorporate(corporateAuthName(l, displayName: displayName), l.authProviderOidc),
  SignInMethod.ldap => l.settingsAuthCorporate(
    corporateAuthName(l, displayName: displayName, method: method),
    l.authProviderLdap,
  ),
};

/// How the active profile signs in: what this device remembered for it,
/// else for its server; profiles from before enterprise sign-in use e-mail.
SignInHint? profileSignIn(SignInHints hints, Profile? profile) =>
    hints.forProfile(profile?.id) ?? hints.forServer(profile?.serverUrl);

// ---- notices ------------------------------------------------------------------------------

/// "Sign-in through the corporate authorization server {name}. The
/// organisation's password does not open the vault…".
class CorporateAuthNotice extends StatelessWidget {
  const CorporateAuthNotice({required this.auth, super.key});

  final ServerAuth auth;

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    return InfoBanner(
      key: const ValueKey('corporate-auth-notice'),
      icon: Icons.apartment_rounded,
      title: l10n.corporateAuthNoticeTitle,
      message: l10n.corporateAuthNotice(serverAuthName(l10n, auth)),
    );
  }
}

/// "Last time you signed in with {method} as {login}" + Forget.
class SignInHintBanner extends StatelessWidget {
  const SignInHintBanner({required this.hint, required this.onForget, super.key});

  final SignInHint hint;
  final VoidCallback? onForget;

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final tokens = GlassTokens.of(context);
    final method = signInMethodLabel(l10n, hint.method, hint.displayName);
    final login = hint.login;
    return Row(
      key: const ValueKey('sign-in-hint'),
      children: [
        Icon(Icons.history_rounded, size: 16, color: tokens.secondaryLabel),
        const SizedBox(width: GlassSpacing.s6),
        Expanded(
          child: Text(
            login == null ? l10n.signInLastTimeNoLogin(method) : l10n.signInLastTime(method, login),
            style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel),
          ),
        ),
        GlassButton.plain(key: const ValueKey('forget-sign-in-hint'), onPressed: onForget, label: l10n.signInForget),
      ],
    );
  }
}

/// The server's sign-in mode differs from what this device remembers.
class AuthModeChangedNotice extends StatelessWidget {
  const AuthModeChangedNotice({required this.auth, super.key});

  final ServerAuth auth;

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    return InfoBanner(
      key: const ValueKey('auth-mode-changed'),
      tone: BannerTone.warning,
      message: auth.isEnterprise
          ? l10n.authModeChangedToEnterprise(serverAuthName(l10n, auth))
          : l10n.authModeChangedToStandard,
    );
  }
}

/// Remembered servers with their sign-in badge; tapping one fills it in.
class RecentServers extends StatelessWidget {
  const RecentServers({required this.hints, required this.onPick, super.key});

  final List<SignInHint> hints;
  final ValueChanged<SignInHint> onPick;

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final tokens = GlassTokens.of(context);
    return Column(
      key: const ValueKey('recent-servers'),
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(l10n.recentServersLabel, style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel)),
        const SizedBox(height: GlassSpacing.s4),
        Wrap(
          spacing: GlassSpacing.s8,
          runSpacing: GlassSpacing.s4,
          children: [
            for (final h in hints.take(5))
              Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  GlassButton.plain(
                    key: ValueKey('recent-server-${h.serverUrl.host}'),
                    onPressed: () => onPick(h),
                    icon: Icons.dns_rounded,
                    label: h.serverUrl.host.isEmpty ? '${h.serverUrl}' : h.serverUrl.host,
                  ),
                  GlassBadge(
                    key: ValueKey('recent-server-badge-${h.serverUrl.host}'),
                    dense: true,
                    tone: h.method.isCorporate ? GlassTone.accent : GlassTone.neutral,
                    label: signInBadgeLabel(l10n, h.method, h.displayName),
                  ),
                ],
              ),
          ],
        ),
      ],
    );
  }
}

/// Settings / sync / devices: "Authorization: corporate server — {name}
/// ({kind})" or "E-mail and account password", plus how and when this
/// device signed in.
class AuthorizationSummary extends ConsumerWidget {
  const AuthorizationSummary({required this.profile, super.key, this.showHelp = true});

  final Profile profile;
  final bool showHelp;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    final tokens = GlassTokens.of(context);
    final hints = ref.watch(signInHintsProvider).value ?? ref.read(authServiceProvider).currentSignInHints;
    final record = profileSignIn(hints, profile);
    final method = record?.method ?? SignInMethod.email;
    final own = hints.forProfile(profile.id);
    final secondary = tokens.typography.callout.copyWith(color: tokens.secondaryLabel);
    return Column(
      key: const ValueKey('authorization-summary'),
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          children: [
            Icon(
              method.isCorporate ? Icons.apartment_rounded : Icons.alternate_email_rounded,
              size: 16,
              color: method.isCorporate ? tokens.palette.accent : tokens.secondaryLabel,
            ),
            const SizedBox(width: GlassSpacing.s6),
            Flexible(
              child: Text(
                authorizationLabel(l10n, method, record?.displayName),
                key: const ValueKey('authorization-value'),
              ),
            ),
          ],
        ),
        if (method.isCorporate && showHelp) ...[
          const SizedBox(height: GlassSpacing.s4),
          Text(l10n.settingsAuthCorporateHelp, style: secondary),
        ],
        if (own != null) ...[
          const SizedBox(height: GlassSpacing.s4),
          Text(
            own.login == null
                ? l10n.settingsAuthSignedIn(
                    signInMethodLabel(l10n, own.method, own.displayName),
                    formatDateTime(l10n, own.at),
                  )
                : l10n.settingsAuthSignedInAs(
                    signInMethodLabel(l10n, own.method, own.displayName),
                    own.login!,
                    formatDateTime(l10n, own.at),
                  ),
            key: const ValueKey('authorization-signed-in'),
            style: secondary,
          ),
        ],
      ],
    );
  }
}

// ---- browser hand-off ---------------------------------------------------------------------

/// Opens the organisation's sign-in page. Desktop: the system browser (the
/// core receives the loopback redirect) → `null`. Mobile: Custom Tabs /
/// `ASWebAuthenticationSession` → the callback URL, `null` if dismissed.
abstract interface class AuthSessionBrowser {
  Future<String?> open(OidcChallenge challenge);

  Future<void> cancel(String ticket);
}

final authSessionBrowserProvider = Provider<AuthSessionBrowser>(
  (ref) => ref.watch(appServicesProvider).isMock ? const _MockAuthSessionBrowser() : const SystemAuthSessionBrowser(),
);

final class SystemAuthSessionBrowser implements AuthSessionBrowser {
  const SystemAuthSessionBrowser();

  static const _channel = MethodChannel('consolecrypt/auth_session');

  /// The sync server: https, or http to a loopback development server.
  static bool _acceptable(Uri url) =>
      url.host.isNotEmpty &&
      url.userInfo.isEmpty &&
      !url.hasFragment &&
      (url.scheme == 'https' || (url.scheme == 'http' && const {'localhost', '127.0.0.1', '::1'}.contains(url.host)));

  @override
  Future<String?> open(OidcChallenge challenge) async {
    final url = challenge.authorizationUrl;
    if (!_acceptable(url)) {
      throw const FormatException('Invalid sign-in URL'); // l10n-ignore: diagnostic
    }
    if (AppPlatform.isMobile) {
      return _channel.invokeMethod<String>('openBrowser', {
        'url': url.toString(),
        'redirectUri': challenge.redirectUri.toString(),
        'ticket': challenge.ticket,
      });
    }
    await openInSystemBrowser(url);
    return null;
  }

  @override
  Future<void> cancel(String ticket) async {
    if (AppPlatform.isMobile) await _channel.invokeMethod<void>('cancelBrowser', {'ticket': ticket});
  }
}

/// Demo / tests: the mock "IdP" signs in without a browser.
final class _MockAuthSessionBrowser implements AuthSessionBrowser {
  const _MockAuthSessionBrowser();

  @override
  Future<String?> open(OidcChallenge challenge) async => null;

  @override
  Future<void> cancel(String ticket) async {}
}

/// One browser sign-in: begin → browser → [run]'s `finish` with the
/// hand-off. [cancel] works at any point (closes the OS auth session and
/// abandons the flow in the core).
final class OidcSignInRun {
  OidcSignInRun(this._auth, this._browser);

  final AuthService _auth;
  final AuthSessionBrowser _browser;
  String? _ticket;
  bool _cancelled = false;

  static const _cancelledError = AppException(AppErrorCode.cancelled, 'Sign-in cancelled'); // l10n-ignore: diagnostic

  Future<T> run<T>({
    required Uri serverUrl,
    required EnterpriseSignInPurpose purpose,
    required Future<T> Function(OidcSignIn signIn) finish,
    String? loginHint,
    String? displayName,
    bool allowNewDeviceIdentity = false,
  }) async {
    final challenge = await _auth.beginOidc(
      serverUrl: serverUrl,
      purpose: purpose,
      loginHint: (loginHint ?? '').trim().isEmpty ? null : loginHint!.trim(),
      displayName: displayName,
      allowNewDeviceIdentity: allowNewDeviceIdentity,
    );
    _ticket = challenge.ticket;
    if (_cancelled) {
      await _auth.cancelOidc(challenge.ticket);
      throw _cancelledError;
    }
    String? callback;
    try {
      callback = await _browser.open(challenge);
    } on Object {
      await _auth.cancelOidc(challenge.ticket);
      throw const AppException(
        AppErrorCode.validation,
        'Browser unavailable', // l10n-ignore: diagnostic
        reason: AppErrorReason.enterpriseBrowserUnavailable,
      );
    }
    if (_cancelled || (!challenge.loopback && callback == null)) {
      await _auth.cancelOidc(challenge.ticket);
      throw _cancelledError;
    }
    return finish(OidcSignIn(ticket: challenge.ticket, callbackUrl: callback));
  }

  Future<void> cancel() async {
    _cancelled = true;
    final ticket = _ticket;
    if (ticket == null) return;
    try {
      await _browser.cancel(ticket);
    } on Object {
      // The auth session may already be closed.
    }
    await _auth.cancelOidc(ticket);
  }
}

// ---- form ---------------------------------------------------------------------------------

/// Called with the organisation's credentials (and the login / e-mail
/// typed, for the remembered hint). Throws on failure.
typedef EnterpriseSubmit = Future<void> Function(EnterpriseSignIn signIn, String? login);

/// The organisation's sign-in for an enterprise server:
/// * OIDC — optional e-mail (login hint), «Sign in with {name}», waiting
///   for the browser with Cancel;
/// * LDAP — «Login (email or DOMAIN\user)» + «Password», caption «Your
///   organisation's account». The password is wiped after every attempt.
/// No registration and no "forgot password".
class EnterpriseSignInForm extends ConsumerStatefulWidget {
  const EnterpriseSignInForm({
    required this.serverUrl,
    required this.auth,
    required this.purpose,
    required this.onSubmit,
    super.key,
    this.initialLogin,
    this.onRunStarted,
    this.footer,
  });

  final Uri serverUrl;
  final ServerAuth auth;
  final EnterpriseSignInPurpose purpose;
  final EnterpriseSubmit onSubmit;
  final String? initialLogin;

  /// Lets a parent cancel the browser sign-in after this form is gone.
  final ValueChanged<OidcSignInRun>? onRunStarted;

  /// Shown above the submit button (e.g. the device name field).
  final Widget? footer;

  @override
  ConsumerState<EnterpriseSignInForm> createState() => _EnterpriseSignInFormState();
}

class _EnterpriseSignInFormState extends ConsumerState<EnterpriseSignInForm> {
  final _login = TextEditingController();
  final _password = TextEditingController();
  OidcSignInRun? _run;
  bool _busy = false;
  Object? _error;
  bool _loginMissing = false;

  @override
  void initState() {
    super.initState();
    _login.text = widget.initialLogin ?? '';
  }

  @override
  void didUpdateWidget(EnterpriseSignInForm old) {
    super.didUpdateWidget(old);
    if (old.initialLogin != widget.initialLogin && _login.text.isEmpty) _login.text = widget.initialLogin ?? '';
  }

  @override
  void dispose() {
    _password.clear();
    _login.dispose();
    _password.dispose();
    super.dispose();
  }

  bool _isCancelled(Object e) => e is AppException && e.code == AppErrorCode.cancelled;

  Future<void> _signInOidc() async {
    final auth = ref.read(authServiceProvider);
    final browser = ref.read(authSessionBrowserProvider);
    setState(() {
      _busy = true;
      _error = null;
    });
    final hint = _login.text.trim();
    Future<void> attempt({required bool allowNewDeviceIdentity}) {
      final run = OidcSignInRun(auth, browser);
      _run = run;
      widget.onRunStarted?.call(run);
      return run.run(
        serverUrl: widget.serverUrl,
        purpose: widget.purpose,
        loginHint: hint,
        allowNewDeviceIdentity: allowNewDeviceIdentity,
        finish: (signIn) => widget.onSubmit(signIn, hint.isEmpty ? null : hint),
      );
    }

    try {
      try {
        await attempt(allowNewDeviceIdentity: false);
      } on AppException catch (e) {
        // Re-authentication with a revoked device identity: a browser code
        // is bound to the device id, so sign in again with a new identity.
        if (widget.purpose != EnterpriseSignInPurpose.reauthenticate || e.code != AppErrorCode.deviceRevoked) {
          rethrow;
        }
        await attempt(allowNewDeviceIdentity: true);
      }
    } on Object catch (e) {
      if (mounted && !_isCancelled(e)) setState(() => _error = e);
    } finally {
      _run = null;
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _cancel() async {
    await _run?.cancel();
  }

  Future<void> _signInLdap() async {
    final login = _login.text.trim();
    if (login.isEmpty || _password.text.isEmpty) {
      setState(() => _loginMissing = true);
      return;
    }
    setState(() {
      _busy = true;
      _error = null;
      _loginMissing = false;
    });
    final password = SecretText(_password.text);
    // The field never keeps the directory password after an attempt.
    _password.clear();
    try {
      await widget.onSubmit(LdapSignIn(login: login, password: password), login);
    } on Object catch (e) {
      if (mounted) setState(() => _error = e);
    } finally {
      password.wipe();
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final tokens = GlassTokens.of(context);
    final auth = widget.auth;
    final error = _error;
    final errorWidget = error == null
        ? null
        : GateErrorText(key: const ValueKey('enterprise-error'), text: errorMessage(l10n, error));
    if (auth.isLdap) {
      return Column(
        key: const ValueKey('ldap-form'),
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(l10n.loginLdapCaption, style: tokens.typography.bodyEmph.copyWith(color: tokens.secondaryLabel)),
          const SizedBox(height: GlassSpacing.s8),
          TextField(
            key: const ValueKey('ldap-login'),
            controller: _login,
            enabled: !_busy,
            autocorrect: false,
            autofillHints: const [AutofillHints.username],
            decoration: InputDecoration(
              labelText: l10n.loginLdapLoginLabel,
              errorText: _loginMissing && _login.text.trim().isEmpty ? l10n.loginLdapLoginRequired : null,
            ),
          ),
          const SizedBox(height: GlassSpacing.s12),
          SecretField(
            key: const ValueKey('ldap-password'),
            controller: _password,
            label: l10n.loginLdapPasswordLabel,
            enabled: !_busy,
            autofillHints: const [AutofillHints.password],
            onSubmitted: (_) => _busy ? null : _signInLdap(),
          ),
          if (_loginMissing && _password.text.isEmpty) ...[
            const SizedBox(height: GlassSpacing.s4),
            GateErrorText(text: l10n.loginPasswordRequired),
          ],
          if (widget.footer != null) ...[const SizedBox(height: GlassSpacing.s12), widget.footer!],
          if (errorWidget != null) ...[const SizedBox(height: GlassSpacing.s12), errorWidget],
          const SizedBox(height: GlassSpacing.s16),
          GlassButton.prominent(
            key: const ValueKey('ldap-submit'),
            size: GlassControlSize.lg,
            expand: true,
            busy: _busy,
            onPressed: _busy ? null : _signInLdap,
            label: l10n.loginSubmitSignIn,
          ),
        ],
      );
    }
    final name = auth.displayName;
    return Column(
      key: const ValueKey('oidc-form'),
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        TextField(
          key: const ValueKey('oidc-login-hint'),
          controller: _login,
          enabled: !_busy,
          keyboardType: TextInputType.emailAddress,
          autocorrect: false,
          autofillHints: const [AutofillHints.email],
          decoration: InputDecoration(labelText: l10n.loginSsoEmailLabel, helperText: l10n.loginSsoEmailHelper),
        ),
        if (widget.footer != null) ...[const SizedBox(height: GlassSpacing.s12), widget.footer!],
        if (errorWidget != null) ...[const SizedBox(height: GlassSpacing.s12), errorWidget],
        const SizedBox(height: GlassSpacing.s16),
        if (_busy) ...[
          ContentSurface(
            key: const ValueKey('oidc-waiting'),
            padding: const EdgeInsets.all(GlassSpacing.s12),
            child: Row(
              children: [
                const SizedBox.square(dimension: 18, child: CircularProgressIndicator(strokeWidth: 2)),
                const SizedBox(width: GlassSpacing.s12),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(l10n.loginSsoWaiting, style: tokens.typography.bodyEmph),
                      Text(
                        l10n.loginSsoWaitingHelp,
                        style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel),
                      ),
                    ],
                  ),
                ),
                const SizedBox(width: GlassSpacing.s8),
                GlassButton(key: const ValueKey('oidc-cancel'), onPressed: _cancel, label: l10n.commonCancel),
              ],
            ),
          ),
        ] else
          GlassButton.prominent(
            key: const ValueKey('oidc-submit'),
            size: GlassControlSize.lg,
            expand: true,
            icon: Icons.open_in_browser_rounded,
            onPressed: _signInOidc,
            label: name == null ? l10n.loginSsoButtonDefault : l10n.loginSsoButton(name),
          ),
      ],
    );
  }
}
