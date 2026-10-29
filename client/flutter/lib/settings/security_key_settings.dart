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
      final service = ref.read(vaultServiceProvider);
      final p = pin;
      await withSecurityKeyTouch(
        context,
        info,
        () => service.setSecurityKeyEnabled(enabled, pin: p),
        touches: enabled ? 2 : 0,
      );
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

/// Runs [action] while the key waits for a touch, with a prompt saying so.
/// After the PIN every request needs the user's touch, but the key only
/// blinks: without the prompt the app looks frozen. Windows shows its own
/// dialog, so the prompt is only for [SecurityKeyPinPrompt.app].
Future<T> withSecurityKeyTouch<T>(
  BuildContext context,
  SecurityKeyInfo info,
  Future<T> Function() action, {
  int touches = 1,
}) async {
  if (info.pinPrompt != SecurityKeyPinPrompt.app || touches == 0) return action();
  BuildContext? dialog;
  showAppDialog<void>(
    context,
    barrierDismissible: false,
    builder: (context) {
      dialog = context;
      return _TouchDialog(touches: touches);
    },
  ).ignore();
  try {
    return await action();
  } finally {
    final open = dialog;
    if (open != null && open.mounted) closeDialog<void>(open);
  }
}

class _TouchDialog extends StatefulWidget {
  const _TouchDialog({required this.touches});
  final int touches;

  @override
  State<_TouchDialog> createState() => _TouchDialogState();
}

class _TouchDialogState extends State<_TouchDialog> with SingleTickerProviderStateMixin {
  late final _blink = AnimationController(vsync: this, duration: const Duration(milliseconds: 700))
    ..repeat(reverse: true);

  @override
  void dispose() {
    _blink.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    return GlassDialog(
      key: const ValueKey('security-key-touch-dialog'),
      title: l.securityKeyTouchTitle,
      content: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          FadeTransition(
            opacity: Tween<double>(begin: 0.25, end: 1).animate(_blink),
            child: Icon(Icons.touch_app_rounded, size: 56, color: tokens.palette.accent),
          ),
          const SizedBox(height: GlassSpacing.s12),
          Text(l.securityKeyTouchHelp, textAlign: TextAlign.center),
          if (widget.touches > 1) ...[
            const SizedBox(height: GlassSpacing.s8),
            Text(
              l.securityKeyTouchTwice,
              key: const ValueKey('security-key-touch-twice'),
              textAlign: TextAlign.center,
              style: tokens.typography.callout,
            ),
          ],
          const SizedBox(height: GlassSpacing.s8),
          Text(
            l.securityKeyTouchTimeout,
            textAlign: TextAlign.center,
            style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel),
          ),
        ],
      ),
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
