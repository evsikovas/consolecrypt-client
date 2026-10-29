import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/secret_field.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Security key unlock (FEAT-16): an optional, per-profile switch on this
/// installation. On: the key registers a credential and its answer seals the
/// vault key; the lock screen then offers "Unlock with a security key". The
/// passphrase and the Recovery Key keep working.
class SecurityKeySettings extends ConsumerStatefulWidget {
  const SecurityKeySettings({super.key});

  @override
  ConsumerState<SecurityKeySettings> createState() => _SecurityKeySettingsState();
}

class _SecurityKeySettingsState extends ConsumerState<SecurityKeySettings> {
  bool _busy = false;
  AppException? _error;

  Future<void> _toggle(SecurityKeyInfo info, bool enabled) async {
    if (_busy) return;
    SecretText? pin;
    if (enabled && info.pinPrompt == SecurityKeyPinPrompt.app) {
      pin = await askSecurityKeyPin(context);
      if (pin == null || !mounted) return;
    }
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      await ref.read(vaultServiceProvider).setSecurityKeyEnabled(enabled, pin: pin);
      if (mounted && enabled) showSnack(context, context.l10n.securityKeyEnabled, tone: GlassTone.success);
    } on AppException catch (e) {
      if (mounted) setState(() => _error = e);
    } finally {
      pin?.wipe();
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final status = ref.watch(vaultStatusProvider).value;
    final info = status?.securityKey ?? const SecurityKeyInfo();
    if (!info.supported || !info.hasVault) return const SizedBox.shrink();
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    return Column(
      key: const ValueKey('security-key-settings'),
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        SwitchListTile.adaptive(
          key: const ValueKey('security-key-switch'),
          contentPadding: EdgeInsets.zero,
          secondary: const Icon(Icons.usb_rounded),
          title: Text(l.securityKeyTitle),
          subtitle: Text(info.enabled ? l.securityKeyOnHelp : l.securityKeyHelp),
          value: info.enabled,
          onChanged: !_busy && (status?.isUnlocked ?? false) ? (enabled) => _toggle(info, enabled) : null,
        ),
        if (_busy)
          Padding(
            padding: const EdgeInsets.only(top: 8),
            child: Row(
              children: [
                const SizedBox.square(dimension: 16, child: CircularProgressIndicator(strokeWidth: 2)),
                const SizedBox(width: GlassSpacing.s8),
                Expanded(child: Text(l.securityKeyTouch, key: const ValueKey('security-key-touch'))),
              ],
            ),
          ),
        if (_error != null)
          Padding(
            padding: const EdgeInsets.only(top: 8),
            child: Text(
              securityKeyErrorMessage(l, _error!),
              key: const ValueKey('security-key-error'),
              style: TextStyle(color: tokens.palette.danger),
            ),
          ),
      ],
    );
  }
}

/// Asks for the security key PIN (platforms where the app passes it).
Future<SecretText?> askSecurityKeyPin(BuildContext context) => showAppDialog<SecretText>(
  context,
  secure: true,
  barrierDismissible: false,
  builder: (_) => const _PinDialog(),
);

class _PinDialog extends StatefulWidget {
  const _PinDialog();

  @override
  State<_PinDialog> createState() => _PinDialogState();
}

class _PinDialogState extends State<_PinDialog> {
  final _pin = TextEditingController();

  @override
  void dispose() {
    _pin.clear();
    _pin.dispose();
    super.dispose();
  }

  void _submit() {
    if (_pin.text.isEmpty) return;
    final secret = SecretText(_pin.text);
    _pin.clear();
    closeDialog(context, secret);
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return GlassDialog(
      title: l.securityKeyPinTitle,
      icon: Icons.usb_rounded,
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(l.securityKeyPinHelp),
          const SizedBox(height: GlassSpacing.s12),
          SecretField(
            key: const ValueKey('security-key-pin'),
            controller: _pin,
            label: l.securityKeyPinLabel,
            autofocus: true,
            layoutHint: false,
            textInputAction: TextInputAction.done,
            onSubmitted: (_) => _submit(),
          ),
        ],
      ),
      secondaryActions: [
        GlassButton(
          key: const ValueKey('security-key-pin-cancel'),
          label: l.commonCancel,
          onPressed: () => closeDialog<SecretText>(context),
        ),
      ],
      primaryAction: GlassButton.prominent(
        key: const ValueKey('security-key-pin-ok'),
        label: l.securityKeyPinContinue,
        onPressed: _submit,
      ),
      onSubmit: _submit,
    );
  }
}

/// Localized security key failure (`invalidInput` with field
/// `security_key`), else the generic message.
String securityKeyErrorMessage(AppLocalizations l, AppException e) {
  if (e.args['field'] != 'security_key') return errorMessage(l, e);
  return switch (e.args['rule']) {
    'no_device' => l.securityKeyErrorNoDevice,
    'several_devices' => l.securityKeyErrorSeveral,
    'pin_required' || 'pin_invalid' => l.securityKeyErrorPin,
    'pin_blocked' => l.securityKeyErrorPinBlocked,
    'cancelled' => l.securityKeyErrorCancelled,
    'wrong_key' => l.securityKeyErrorWrongKey,
    'no_hmac_secret' => l.securityKeyErrorUnsupportedKey,
    'unsupported' => l.securityKeyErrorUnsupported,
    _ => l.securityKeyErrorDevice,
  };
}
