import 'dart:async';

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
import 'package:material_ui/material_ui.dart';

/// Takes the account password: the secure material (§4.7).
Future<void> showEnableSyncWizard(BuildContext context) =>
    showAppDialog<void>(context, secure: true, barrierDismissible: false, builder: (_) => const EnableSyncWizard());

enum _WizardStep { server, account, upload }

/// Input problems detected by the wizard itself (service failures are kept as
/// the [AppException] and turned into text by [errorMessage]).
enum _WizardError { credentialsRequired }

String _errorText(AppLocalizations l10n, Object error) => switch (error) {
  final ServerUrlProblem problem => problem.localized(l10n),
  _WizardError.credentialsRequired => l10n.enableSyncDialogCredentialsRequired,
  _ => errorMessage(l10n, error),
};

/// Local → Synced (ADR-0106): server URL → sign in / register → upload
/// progress. The vault keeps its id and keys; nothing is re-encrypted with
/// new keys and the server only ever receives ciphertext.
class EnableSyncWizard extends ConsumerStatefulWidget {
  const EnableSyncWizard({super.key});

  @override
  ConsumerState<EnableSyncWizard> createState() => _EnableSyncWizardState();
}

class _EnableSyncWizardState extends ConsumerState<EnableSyncWizard> {
  final _server = TextEditingController();
  final _email = TextEditingController();
  final _password = TextEditingController();
  final _deviceName = TextEditingController();

  /// The localized default device name last put into [_deviceName]; replaced
  /// when the language changes unless the user edited it.
  String? _defaultDeviceName;
  _WizardStep _step = _WizardStep.server;
  bool _createAccount = true;
  ServerInfo? _info;
  EnableSyncProgress? _progress;
  StreamSubscription<EnableSyncProgress>? _sub;
  Object? _error;
  bool _busy = false;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final localized = AppPlatform.defaultDeviceName(context.l10n);
    if (_defaultDeviceName == null || _deviceName.text == _defaultDeviceName) _deviceName.text = localized;
    _defaultDeviceName = localized;
  }

  @override
  void dispose() {
    unawaited(_sub?.cancel());
    for (final c in [_server, _email, _password, _deviceName]) {
      c.dispose();
    }
    super.dispose();
  }

  Future<void> _checkServer() async {
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
      final info = await ref.read(authServiceProvider).probeServer(Uri.parse(_server.text.trim()));
      if (mounted) {
        setState(() {
          _info = info;
          _createAccount = info.registrationOpen;
          _step = _WizardStep.account;
        });
      }
    } on AppException catch (e) {
      if (mounted) setState(() => _error = e);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  void _start() {
    if (!_email.text.contains('@') || _password.text.isEmpty) {
      setState(() => _error = _WizardError.credentialsRequired);
      return;
    }
    final password = SecretText(_password.text);
    setState(() {
      _step = _WizardStep.upload;
      _error = null;
      _progress = const EnableSyncProgress(EnableSyncStep.authenticating);
    });
    _sub = ref
        .read(syncServiceProvider)
        .enableSync(
          serverUrl: Uri.parse(_server.text.trim()),
          email: _email.text.trim(),
          password: password,
          deviceName: _deviceName.text,
          createAccount: _createAccount,
        )
        .listen(
          (p) => setState(() => _progress = p),
          onDone: () {
            password.wipe();
            _password.clear();
          },
        );
  }

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final l10n = context.l10n;
    final progress = _progress;
    final done = progress?.step == EnableSyncStep.done;
    final failed = progress?.step == EnableSyncStep.failed;
    final registrationOpen = _info?.registrationOpen ?? true;
    return GlassDialog(
      key: const ValueKey('enable-sync-wizard'),
      icon: Icons.cloud_upload_rounded,
      iconTone: GlassTone.accent,
      title: l10n.enableSyncDialogTitle,
      width: 568,
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(switch (_step) {
            _WizardStep.server => l10n.enableSyncDialogStepServer,
            _WizardStep.account => l10n.enableSyncDialogStepAccount,
            _WizardStep.upload => l10n.enableSyncDialogStepUpload,
          }, style: tokens.typography.bodyEmph.copyWith(color: tokens.secondaryLabel)),
          const SizedBox(height: GlassSpacing.s12),
          ...switch (_step) {
            _WizardStep.server => [
              Text(l10n.enableSyncDialogIntro),
              const SizedBox(height: GlassSpacing.s12),
              TextField(
                key: const ValueKey('sync-server-url'),
                controller: _server,
                autofocus: true,
                keyboardType: TextInputType.url,
                decoration: InputDecoration(
                  labelText: l10n.enableSyncDialogServerUrl,
                  hintText: 'https://sync.example.org',
                ),
                onSubmitted: (_) => _checkServer(),
              ),
            ],
            _WizardStep.account => [
              if (_info != null)
                InfoBanner(
                  tone: BannerTone.success,
                  message: l10n.enableSyncDialogServerInfo(_info!.serverVersion, _info!.protocolVersion),
                ),
              const SizedBox(height: GlassSpacing.s12),
              // Invite-only servers: signing in is the only option.
              if (registrationOpen) ...[
                GlassSegmented<bool>(
                  key: const ValueKey('sync-account-mode'),
                  inChrome: false,
                  expand: true,
                  segments: [
                    GlassSegment(value: true, label: l10n.enableSyncDialogCreateAccount),
                    GlassSegment(value: false, label: l10n.enableSyncDialogSignIn),
                  ],
                  selected: _createAccount,
                  onChanged: (v) => setState(() => _createAccount = v),
                ),
                const SizedBox(height: GlassSpacing.s12),
              ],
              TextField(
                key: const ValueKey('sync-email'),
                controller: _email,
                keyboardType: TextInputType.emailAddress,
                decoration: InputDecoration(labelText: l10n.enableSyncDialogEmail),
              ),
              const SizedBox(height: GlassSpacing.s12),
              SecretField(
                key: const ValueKey('sync-password'),
                controller: _password,
                label: l10n.enableSyncDialogPassword,
                helper: _createAccount ? l10n.enableSyncDialogPasswordHelper : null,
              ),
              const SizedBox(height: GlassSpacing.s12),
              TextField(
                controller: _deviceName,
                decoration: InputDecoration(labelText: l10n.enableSyncDialogDeviceName),
              ),
            ],
            _WizardStep.upload => [
              Text(progress?.step.localized(l10n) ?? '', key: const ValueKey('sync-progress-label')),
              const SizedBox(height: GlassSpacing.s8),
              if (!done && !failed) LinearProgressIndicator(value: progress?.fraction),
              if (progress?.step == EnableSyncStep.uploading && progress!.total > 0)
                Padding(
                  padding: const EdgeInsets.only(top: GlassSpacing.s4),
                  child: Text(l10n.enableSyncDialogUploadedObjects(progress.uploaded, progress.total)),
                ),
              if (done) InfoBanner(tone: BannerTone.success, message: l10n.enableSyncDialogDone),
              // The service's message is an English diagnostic: secondary detail only.
              if (failed)
                InfoBanner(
                  tone: BannerTone.danger,
                  title: progress?.message == null ? null : l10n.enableSyncDialogFailedTitle,
                  message: progress?.message ?? l10n.enableSyncDialogFailed,
                ),
            ],
          },
          if (_error != null) ...[
            const SizedBox(height: GlassSpacing.s12),
            GateErrorText(text: _errorText(l10n, _error!)),
          ],
        ],
      ),
      secondaryActions: [
        if (!done)
          GlassButton(
            onPressed: _step == _WizardStep.upload && !failed ? null : () => closeDialog<void>(context),
            label: l10n.commonCancel,
          ),
        if (_step == _WizardStep.account)
          GlassButton(onPressed: () => setState(() => _step = _WizardStep.server), label: l10n.commonBack),
      ],
      primaryAction: switch (_step) {
        _WizardStep.server => GlassButton.prominent(
          key: const ValueKey('sync-next'),
          busy: _busy,
          onPressed: _busy ? null : _checkServer,
          label: l10n.commonNext,
        ),
        _WizardStep.account => GlassButton.prominent(
          key: const ValueKey('sync-start'),
          onPressed: _start,
          label: l10n.enableSyncDialogStart,
        ),
        _WizardStep.upload => GlassButton.prominent(
          key: const ValueKey('sync-finish'),
          onPressed: done || failed
              ? () => failed ? setState(() => _step = _WizardStep.account) : closeDialog<void>(context)
              : null,
          label: failed ? l10n.enableSyncDialogTryAgain : l10n.commonDone,
        ),
      },
    );
  }
}

/// Synced → Local (ADR-0106).
Future<void> showDisconnectDialog(BuildContext context, WidgetRef ref) async {
  var revoke = false;
  final ok = await showAppDialog<bool>(
    context,
    builder: (context) => StatefulBuilder(
      builder: (context, setState) => GlassDialog(
        key: const ValueKey('disconnect-dialog'),
        icon: Icons.link_off_rounded,
        title: context.l10n.syncDisconnectTitle,
        width: 520,
        content: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(context.l10n.syncDisconnectMessage),
            const SizedBox(height: GlassSpacing.s8),
            CheckboxListTile(
              key: const ValueKey('disconnect-revoke'),
              contentPadding: EdgeInsets.zero,
              controlAffinity: ListTileControlAffinity.leading,
              value: revoke,
              onChanged: (v) => setState(() => revoke = v ?? false),
              title: Text(context.l10n.syncDisconnectRevoke),
            ),
          ],
        ),
        secondaryActions: [
          GlassButton(autofocus: true, onPressed: () => closeDialog(context, false), label: context.l10n.commonCancel),
        ],
        primaryAction: GlassButton.prominent(
          key: const ValueKey('disconnect-confirm'),
          onPressed: () => closeDialog(context, true),
          label: context.l10n.syncDisconnectConfirm,
        ),
      ),
    ),
  );
  if (ok ?? false) {
    try {
      await ref.read(syncServiceProvider).disconnect(revokeThisDevice: revoke);
    } on AppException catch (e) {
      if (context.mounted) showSnack(context, errorMessage(context.l10n, e), error: true);
    }
  }
}
