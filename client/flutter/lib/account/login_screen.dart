import 'dart:async';

import 'package:consolecrypt/account/enterprise_sign_in.dart';
import 'package:consolecrypt/account/profile_switcher.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/l10n/labels.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/secret_field.dart';
import 'package:consolecrypt/core/widgets/security_widgets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:material_ui/material_ui.dart';

enum LoginMode { signIn, register }

/// Localized text for a [validateServerUrl] result.
/// Errors raised by the login form itself (not by a service).
enum _LoginIssue { resetNeedsServerAndEmail }

/// "Connect to a server": sign in / create an account on a self-hosted
/// server (new synced profile), or re-authenticate the active synced
/// profile whose session ended. There is no default server URL.
///
/// After the server address the screen reads `/v1/meta`: a standard server
/// asks for e-mail + account password (+ registration); an enterprise
/// server asks for the organisation's sign-in (browser SSO or directory
/// login) and says that a corporate authorization server is used. The
/// method and login of the last successful sign-in per server are
/// remembered (never the password) and preselected.
class LoginScreen extends ConsumerStatefulWidget {
  const LoginScreen({super.key, this.newProfile = false});

  /// Creating a new synced profile (vs. re-auth of the active one).
  final bool newProfile;

  @override
  ConsumerState<LoginScreen> createState() => _LoginScreenState();
}

class _LoginScreenState extends ConsumerState<LoginScreen> {
  final _form = GlobalKey<FormState>();
  final _server = TextEditingController();
  final _email = TextEditingController();
  final _password = TextEditingController();
  final _confirm = TextEditingController();
  final _deviceName = TextEditingController();

  /// The localized default device name last put into [_deviceName]; replaced
  /// when the language changes unless the user edited it.
  String? _defaultDeviceName;
  LoginMode _mode = LoginMode.signIn;
  ServerInfo? _serverInfo;

  /// A [ServerUrlProblem], [_LoginIssue] or service error; rendered in build.
  Object? _error;
  bool _busy = false;
  bool _initialised = false;

  bool get _reauth => !widget.newProfile && ref.read(appStageProvider) == AppStage.signedOut;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final localized = AppPlatform.defaultDeviceName(context.l10n);
    if (_defaultDeviceName == null || _deviceName.text == _defaultDeviceName) _deviceName.text = localized;
    _defaultDeviceName = localized;
    if (_initialised) return;
    _initialised = true;
    if (_reauth) {
      final profile = ref.read(activeProfileProvider);
      _server.text = profile?.serverUrl?.toString() ?? '';
      _email.text = profile?.accountEmail ?? '';
      // The server may have switched sign-in modes since the last session.
      if (_server.text.isNotEmpty) scheduleMicrotask(_probe);
    }
  }

  /// The remembered sign-in of the server in the address field.
  SignInHint? _hintFor(SignInHints hints) {
    final url = Uri.tryParse(_server.text.trim());
    if (url == null || validateServerUrl(_server.text) != null) return null;
    return hints.forServer(url);
  }

  void _pickRecent(SignInHint hint) {
    _server.text = hint.serverUrl.toString();
    setState(() => _serverInfo = null);
    unawaited(_probe());
  }

  @override
  void dispose() {
    for (final c in [_server, _email, _password, _confirm, _deviceName]) {
      c.dispose();
    }
    super.dispose();
  }

  Future<void> _probe() async {
    final err = validateServerUrl(_server.text);
    if (err != null) {
      setState(() => _error = err);
      return;
    }
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final auth = ref.read(authServiceProvider);
      final url = Uri.parse(_server.text.trim());
      final info = await auth.probeServer(url);
      // Preselect what this device used last time on this server.
      final hint = auth.currentSignInHints.forServer(url);
      if (mounted) {
        setState(() {
          _serverInfo = info;
          if (_mode == LoginMode.register && info.auth.isEnterprise) _mode = LoginMode.signIn;
          final login = hint?.login;
          if (!_reauth && login != null && _email.text.isEmpty && hint?.method == info.auth.method) {
            _email.text = login;
          }
        });
      }
    } on AppException catch (e) {
      if (mounted) setState(() => _error = e);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _submit() async {
    if (!(_form.currentState?.validate() ?? false)) return;
    setState(() {
      _busy = true;
      _error = null;
    });
    final password = SecretText(_password.text);
    try {
      if (_reauth) {
        await ref.read(authServiceProvider).signIn(email: _email.text.trim(), password: password);
      } else {
        await ref
            .read(profileServiceProvider)
            .createSyncedProfile(
              serverUrl: Uri.parse(_server.text.trim()),
              email: _email.text.trim(),
              password: password,
              deviceName: _deviceName.text,
              createAccount: _mode == LoginMode.register,
            );
      }
      _password.clear();
      _confirm.clear();
      await _remember(SignInMethod.email, null, _email.text.trim());
      if (mounted) context.go(AppRoutes.hosts); // the gate routes onwards
    } on AppException catch (e) {
      if (mounted) setState(() => _error = e);
      // The server only takes the organisation's sign-in: show it.
      if (e.code == AppErrorCode.enterpriseAuthRequired && mounted) unawaited(_probe());
    } finally {
      password.wipe();
      if (mounted) setState(() => _busy = false);
    }
  }

  /// Remembers how this server (and the now active profile) signed in.
  Future<void> _remember(SignInMethod method, String? displayName, String? login) async {
    final url = Uri.tryParse(_server.text.trim());
    if (url == null) return;
    try {
      await ref
          .read(authServiceProvider)
          .rememberSignIn(
            serverUrl: url,
            method: method,
            displayName: displayName,
            login: login,
            profileId: ref.read(profileServiceProvider).currentProfiles.activeId,
          );
    } on AppException {
      // A convenience only.
    }
  }

  /// The organisation's sign-in (enterprise servers).
  Future<void> _submitEnterprise(ServerAuth auth, EnterpriseSignIn signIn, String? login) async {
    final url = Uri.parse(_server.text.trim());
    if (_reauth) {
      await ref.read(authServiceProvider).signInEnterprise(signIn);
    } else {
      await ref
          .read(profileServiceProvider)
          .createEnterpriseProfile(serverUrl: url, signIn: signIn, deviceName: _deviceName.text);
    }
    await _remember(auth.method, auth.displayName, login);
    if (mounted) context.go(AppRoutes.hosts); // the gate routes onwards
  }

  Future<void> _forgotPassword() async {
    final err = validateServerUrl(_server.text);
    if (err != null || !_email.text.contains('@')) {
      setState(() => _error = _LoginIssue.resetNeedsServerAndEmail);
      return;
    }
    await ref
        .read(authServiceProvider)
        .requestPasswordReset(serverUrl: Uri.parse(_server.text.trim()), email: _email.text.trim());
    if (!mounted) return;
    setState(() => _error = null);
    showSnack(context, context.l10n.loginResetEmailSent, tone: GlassTone.success);
  }

  String _errorText(AppLocalizations l10n, Object error) => switch (error) {
    final ServerUrlProblem p => p.localized(l10n),
    _LoginIssue.resetNeedsServerAndEmail => l10n.loginForgotNeedsServerAndEmail,
    _ => errorMessage(l10n, error),
  };

  @override
  Widget build(BuildContext context) {
    final reauth = _reauth;
    final register = !reauth && _mode == LoginMode.register;
    final tokens = GlassTokens.of(context);
    final t = tokens.typography;
    final l10n = context.l10n;
    final hasProfiles = (ref.watch(profilesProvider).value?.profiles ?? const []).isNotEmpty;
    final info = _serverInfo;
    final hints = ref.watch(signInHintsProvider).value ?? ref.read(authServiceProvider).currentSignInHints;
    final hint = _hintFor(hints);
    return GateScaffold(
      leading: reauth
          ? null
          : GlassButton(
              onPressed: () => context.go(hasProfiles && !widget.newProfile ? AppRoutes.hosts : AppRoutes.welcome),
              icon: Icons.arrow_back_rounded,
              label: l10n.commonBack,
            ),
      child: Form(
        key: _form,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            GateHeader(
              icon: Icons.cloud_sync_rounded,
              title: reauth ? l10n.loginTitleReauth : l10n.loginTitle,
              subtitle: reauth ? l10n.loginSubtitleReauth : l10n.loginSubtitle,
            ),
            TextFormField(
              key: const ValueKey('server-url'),
              controller: _server,
              readOnly: reauth,
              keyboardType: TextInputType.url,
              autocorrect: false,
              decoration: InputDecoration(
                labelText: l10n.loginServerUrlLabel,
                hintText: 'https://sync.consolecrypt.dev',
                suffixIcon: reauth
                    ? null
                    : Padding(
                        padding: const EdgeInsetsDirectional.only(end: GlassSpacing.s4),
                        child: TextButton(onPressed: _busy ? null : _probe, child: Text(l10n.loginCheckServer)),
                      ),
              ),
              validator: (v) => validateServerUrl(v ?? '')?.localized(l10n),
              onChanged: (_) => setState(() => _serverInfo = null),
              onFieldSubmitted: (_) => _busy ? null : _probe(),
            ),
            if (!reauth && info == null && hints.byServer.isNotEmpty) ...[
              const SizedBox(height: GlassSpacing.s8),
              RecentServers(hints: hints.recentServers, onPick: _pickRecent),
            ],
            if (info != null) ...[
              const SizedBox(height: GlassSpacing.s8),
              InfoBanner(
                tone: BannerTone.success,
                message: info.auth.isEnterprise
                    ? l10n.enableSyncDialogServerInfo(info.serverVersion, info.protocolVersion)
                    : info.registrationOpen
                    ? l10n.loginServerInfoRegistrationOpen(info.serverVersion, info.protocolVersion)
                    : l10n.loginServerInfoInviteOnly(info.serverVersion, info.protocolVersion),
              ),
            ],
            if (hint != null) ...[
              const SizedBox(height: GlassSpacing.s8),
              SignInHintBanner(
                hint: hint,
                onForget: () => ref.read(authServiceProvider).forgetSignInHint(hint.serverUrl),
              ),
            ],
            if (info != null && hint != null && hint.method.isCorporate != info.auth.isEnterprise) ...[
              const SizedBox(height: GlassSpacing.s8),
              AuthModeChangedNotice(auth: info.auth),
            ],
            const SizedBox(height: GlassSpacing.s16),
            if (info != null && info.auth.needsNewerApp)
              InfoBanner(
                key: const ValueKey('server-needs-newer-app'),
                tone: BannerTone.danger,
                message: l10n.serverNeedsNewerApp,
              )
            else if (info != null && info.auth.isEnterprise) ...[
              CorporateAuthNotice(auth: info.auth),
              const SizedBox(height: GlassSpacing.s16),
              EnterpriseSignInForm(
                key: ValueKey('enterprise-${info.auth.method.wireName}'),
                serverUrl: Uri.parse(_server.text.trim()),
                auth: info.auth,
                purpose: reauth ? EnterpriseSignInPurpose.reauthenticate : EnterpriseSignInPurpose.newProfile,
                initialLogin: hint?.method == info.auth.method
                    ? hint?.login
                    : (reauth && info.auth.isOidc ? _email.text : null),
                onSubmit: (signIn, login) => _submitEnterprise(info.auth, signIn, login),
                footer: reauth
                    ? null
                    : TextFormField(
                        controller: _deviceName,
                        decoration: InputDecoration(
                          labelText: l10n.loginDeviceNameLabel,
                          helperText: l10n.loginDeviceNameHelper,
                        ),
                      ),
              ),
            ] else ...[
              if (!reauth) ...[
                GlassSegmented<LoginMode>(
                  key: const ValueKey('login-mode'),
                  inChrome: false,
                  expand: true,
                  segments: [
                    GlassSegment(value: LoginMode.signIn, label: l10n.loginModeSignIn),
                    GlassSegment(value: LoginMode.register, label: l10n.loginModeRegister),
                  ],
                  selected: _mode,
                  onChanged: (m) => setState(() => _mode = m),
                ),
                const SizedBox(height: GlassSpacing.s16),
              ],
              TextFormField(
                key: const ValueKey('email'),
                controller: _email,
                keyboardType: TextInputType.emailAddress,
                autocorrect: false,
                autofillHints: const [AutofillHints.email],
                decoration: InputDecoration(labelText: l10n.loginEmailLabel),
                validator: (v) => (v ?? '').contains('@') ? null : l10n.loginEmailRequired,
              ),
              const SizedBox(height: GlassSpacing.s12),
              SecretField(
                key: const ValueKey('password'),
                controller: _password,
                label: l10n.loginPasswordLabel,
                autofillHints: [register ? AutofillHints.newPassword : AutofillHints.password],
                helper: register ? l10n.loginPasswordHelperRegister : null,
                validator: (v) {
                  if ((v ?? '').isEmpty) return l10n.loginPasswordRequired;
                  if (register && v!.length < 10) return l10n.loginPasswordTooShort;
                  return null;
                },
                onSubmitted: (_) => register ? null : _submit(),
              ),
              if (register) ...[
                const SizedBox(height: GlassSpacing.s12),
                SecretField(
                  key: const ValueKey('password-confirm'),
                  controller: _confirm,
                  label: l10n.loginPasswordRepeatLabel,
                  validator: (v) => v == _password.text ? null : l10n.loginPasswordsDoNotMatch,
                ),
              ],
              if (!reauth) ...[
                const SizedBox(height: GlassSpacing.s12),
                TextFormField(
                  controller: _deviceName,
                  decoration: InputDecoration(
                    labelText: l10n.loginDeviceNameLabel,
                    helperText: l10n.loginDeviceNameHelper,
                  ),
                  validator: (v) => (v ?? '').trim().isEmpty ? l10n.loginDeviceNameRequired : null,
                ),
              ],
              if (_error != null) ...[
                const SizedBox(height: GlassSpacing.s12),
                GateErrorText(key: const ValueKey('login-error'), text: _errorText(l10n, _error!)),
              ],
              const SizedBox(height: GlassSpacing.s20),
              GlassButton.prominent(
                key: const ValueKey('login-submit'),
                size: GlassControlSize.lg,
                expand: true,
                busy: _busy,
                onPressed: _busy ? null : _submit,
                label: register ? l10n.loginSubmitRegister : l10n.loginSubmitSignIn,
              ),
              if (!register) ...[
                const SizedBox(height: GlassSpacing.s8),
                Center(
                  child: GlassButton.plain(onPressed: _busy ? null : _forgotPassword, label: l10n.loginForgotPassword),
                ),
              ],
            ],
            if (info != null && info.auth.isEnterprise && _error != null) ...[
              const SizedBox(height: GlassSpacing.s12),
              GateErrorText(key: const ValueKey('login-error'), text: _errorText(l10n, _error!)),
            ],
            if (reauth) ...[const SizedBox(height: GlassSpacing.s8), const ProfileSwitcherRow()],
            const SizedBox(height: GlassSpacing.s8),
            Text(l10n.loginPassphraseNeverLeaves, style: t.callout.copyWith(color: tokens.secondaryLabel)),
            const DemoHints(),
          ],
        ),
      ),
    );
  }
}
