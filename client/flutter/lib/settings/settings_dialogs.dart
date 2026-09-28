import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/passphrase_strength.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/keyboard_layout.dart';
import 'package:consolecrypt/core/widgets/secret_field.dart';
import 'package:consolecrypt/core/widgets/security_widgets.dart';
import 'package:consolecrypt/vault/recovery_kit_screen.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Passphrases, passwords and Recovery Kits: the secure material (§4.7).
Future<void> showChangePassphraseDialog(BuildContext context) =>
    showAppDialog<void>(context, secure: true, builder: (_) => const ChangePassphraseDialog());

Future<void> showRegenerateKitDialog(BuildContext context) =>
    showAppDialog<void>(context, secure: true, builder: (_) => const RegenerateKitDialog());

Future<void> showChangeAccountPasswordDialog(BuildContext context) =>
    showAppDialog<void>(context, secure: true, builder: (_) => const ChangeAccountPasswordDialog());

/// Change the vault passphrase (new password envelope).
class ChangePassphraseDialog extends ConsumerStatefulWidget {
  const ChangePassphraseDialog({super.key});

  @override
  ConsumerState<ChangePassphraseDialog> createState() => _ChangePassphraseDialogState();
}

class _ChangePassphraseDialogState extends ConsumerState<ChangePassphraseDialog> {
  final _current = TextEditingController();
  final _next = TextEditingController();
  final _confirm = TextEditingController();
  PassphraseStrength _strength = PassphraseStrength.estimate('');
  AppException? _error;
  bool _busy = false;

  @override
  void dispose() {
    _current.dispose();
    _next.dispose();
    _confirm.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    setState(() {
      _busy = true;
      _error = null;
    });
    final current = SecretText(_current.text);
    final next = SecretText(_next.text);
    try {
      await ref.read(vaultServiceProvider).changePassphrase(current: current, next: next);
      if (mounted) {
        showSnack(context, context.l10n.settingsDialogPassphraseChanged, tone: GlassTone.success);
        closeDialog<void>(context);
      }
    } on AppException catch (e) {
      if (mounted) setState(() => _error = e);
    } finally {
      current.wipe();
      next.wipe();
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final ok = _strength.acceptable && _next.text == _confirm.text && _current.text.isNotEmpty;
    return GlassDialog(
      key: const ValueKey('change-passphrase-dialog'),
      icon: Icons.key_rounded,
      title: l10n.settingsDialogChangePassphraseTitle,
      width: 520,
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          SecretField(
            controller: _current,
            label: l10n.settingsDialogCurrentPassphrase,
            autofocus: true,
            onChanged: (_) => setState(() {}),
          ),
          const SizedBox(height: GlassSpacing.s12),
          SecretField(
            controller: _next,
            label: l10n.settingsDialogNewPassphrase,
            onChanged: (v) => setState(() => _strength = PassphraseStrength.estimate(v)),
          ),
          const SizedBox(height: GlassSpacing.s8),
          StrengthMeter(strength: _strength),
          PassphraseLayoutNotice(passphrase: _next.text),
          const SizedBox(height: GlassSpacing.s12),
          SecretField(
            controller: _confirm,
            label: l10n.settingsDialogRepeatPassphrase,
            onChanged: (_) => setState(() {}),
          ),
          if (_error != null) ...[
            const SizedBox(height: GlassSpacing.s8),
            GateErrorText(text: errorMessage(l10n, _error!)),
          ],
        ],
      ),
      secondaryActions: [GlassButton(label: l10n.commonCancel, onPressed: () => closeDialog<void>(context))],
      primaryAction: GlassButton.prominent(
        busy: _busy,
        onPressed: ok && !_busy ? _save : null,
        label: l10n.settingsDialogChangePassphraseConfirm,
      ),
    );
  }
}

/// Regenerate the Recovery Key (trusted device) and show the new kit.
class RegenerateKitDialog extends ConsumerStatefulWidget {
  const RegenerateKitDialog({super.key});

  @override
  ConsumerState<RegenerateKitDialog> createState() => _RegenerateKitDialogState();
}

class _RegenerateKitDialogState extends ConsumerState<RegenerateKitDialog> {
  RecoveryKit? _kit;
  bool _revealed = false;
  bool _busy = false;

  @override
  void dispose() {
    _kit?.forget();
    super.dispose();
  }

  Future<void> _generate() async {
    setState(() => _busy = true);
    final kit = await runWithFeedback(context, () => ref.read(vaultServiceProvider).regenerateRecoveryKit());
    if (mounted) {
      setState(() {
        _kit = kit;
        _busy = false;
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final kit = _kit;
    final tokens = GlassTokens.of(context);
    return GlassDialog(
      key: const ValueKey('regenerate-kit-dialog'),
      icon: Icons.health_and_safety_rounded,
      title: l10n.settingsDialogNewKitTitle,
      width: kit == null ? 520 : 780,
      content: kit == null
          ? Text(l10n.settingsDialogNewKitWarning)
          : Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                LabeledValue(
                  label: l10n.settingsVaultIdLabel,
                  value: SelectableText(
                    kit.vaultId.value,
                    style: tokens.typography.mono.copyWith(fontSize: 12, color: tokens.palette.label),
                  ),
                ),
                const SizedBox(height: GlassSpacing.s8),
                // Rendered only after an explicit reveal, never blurred (§4.14).
                if (!_revealed)
                  ContentSurface(
                    kind: ContentSurfaceKind.paper,
                    padding: const EdgeInsets.symmetric(vertical: GlassSpacing.s24),
                    child: Center(
                      child: GlassButton(
                        key: const ValueKey('reveal-new-kit'),
                        size: GlassControlSize.lg,
                        icon: Icons.visibility_rounded,
                        onPressed: () => setState(() => _revealed = true),
                        label: l10n.settingsDialogShowKit,
                      ),
                    ),
                  )
                else
                  RecoveryKitPaper(kit: kit, qrSize: 150),
              ],
            ),
      secondaryActions: [
        GlassButton(
          onPressed: () => closeDialog<void>(context),
          label: kit == null ? l10n.commonCancel : l10n.settingsDialogKitSaved,
        ),
      ],
      primaryAction: kit == null
          ? GlassButton.prominent(
              busy: _busy,
              onPressed: _busy ? null : _generate,
              label: l10n.settingsDialogGenerateKit,
            )
          : null,
    );
  }
}

/// Minimum account password length (enforced by the server/app-core).
const _minAccountPasswordLength = 10;

/// Account password change (synced profiles).
class ChangeAccountPasswordDialog extends ConsumerStatefulWidget {
  const ChangeAccountPasswordDialog({super.key});

  @override
  ConsumerState<ChangeAccountPasswordDialog> createState() => _ChangeAccountPasswordDialogState();
}

class _ChangeAccountPasswordDialogState extends ConsumerState<ChangeAccountPasswordDialog> {
  final _current = TextEditingController();
  final _next = TextEditingController();
  AppException? _error;

  @override
  void dispose() {
    _current.dispose();
    _next.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    final current = SecretText(_current.text);
    final next = SecretText(_next.text);
    try {
      await ref.read(authServiceProvider).changeAccountPassword(current: current, next: next);
      if (mounted) {
        showSnack(context, context.l10n.settingsDialogAccountPasswordChanged, tone: GlassTone.success);
        closeDialog<void>(context);
      }
    } on AppException catch (e) {
      if (mounted) setState(() => _error = e);
    } finally {
      current.wipe();
      next.wipe();
    }
  }

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    return GlassDialog(
      key: const ValueKey('change-account-password-dialog'),
      icon: Icons.password_rounded,
      title: l10n.settingsDialogAccountPasswordTitle,
      width: 480,
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          SecretField(controller: _current, label: l10n.settingsDialogCurrentPassword, autofocus: true),
          const SizedBox(height: GlassSpacing.s12),
          SecretField(
            controller: _next,
            label: l10n.settingsDialogNewPassword,
            helper: l10n.settingsDialogPasswordMinLength(_minAccountPasswordLength),
          ),
          const SizedBox(height: GlassSpacing.s8),
          Text(l10n.settingsDialogVaultPassphraseUnchanged),
          if (_error != null) ...[
            const SizedBox(height: GlassSpacing.s8),
            GateErrorText(text: errorMessage(l10n, _error!)),
          ],
        ],
      ),
      secondaryActions: [GlassButton(label: l10n.commonCancel, onPressed: () => closeDialog<void>(context))],
      primaryAction: GlassButton.prominent(onPressed: _save, label: l10n.commonChange),
    );
  }
}
