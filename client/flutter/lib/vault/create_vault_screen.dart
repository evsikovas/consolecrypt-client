import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/passphrase_strength.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/keyboard_layout.dart';
import 'package:consolecrypt/core/widgets/secret_field.dart';
import 'package:consolecrypt/core/widgets/security_widgets.dart';
import 'package:consolecrypt/vault/onboarding_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:material_ui/material_ui.dart';

/// Onboarding step 1: choose the Vault passphrase (with strength meter).
class CreateVaultScreen extends ConsumerStatefulWidget {
  const CreateVaultScreen({super.key});

  @override
  ConsumerState<CreateVaultScreen> createState() => _CreateVaultScreenState();
}

class _CreateVaultScreenState extends ConsumerState<CreateVaultScreen> {
  final _name = TextEditingController();
  final _passphrase = TextEditingController();
  final _confirm = TextEditingController();
  PassphraseStrength _strength = PassphraseStrength.estimate('');
  bool _busy = false;
  AppException? _error;
  String? _defaultName;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final initial = ref.read(activeProfileProvider)?.name ?? context.l10n.createVaultDefaultName;
    if (_defaultName == null || _name.text == _defaultName) _name.text = initial;
    _defaultName = initial;
  }

  @override
  void dispose() {
    _name.dispose();
    _passphrase.dispose();
    _confirm.dispose();
    super.dispose();
  }

  bool get _canSubmit => _strength.acceptable && _passphrase.text == _confirm.text && !_busy;

  Future<void> _submit() async {
    if (!_canSubmit) return;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      await ref
          .read(onboardingControllerProvider.notifier)
          .createVault(name: _name.text, passphrase: SecretText(_passphrase.text));
      _passphrase.clear();
      _confirm.clear();
      if (mounted) context.go(AppRoutes.recoveryKit);
    } on AppException catch (e) {
      if (mounted) setState(() => _error = e);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _cancel() async {
    final profile = ref.read(activeProfileProvider);
    if (profile == null) return;
    if (profile.isLocal) {
      await ref.read(profileServiceProvider).delete(profile.id);
    } else {
      await ref.read(authServiceProvider).logout();
    }
    if (mounted) context.go(AppRoutes.hosts);
  }

  @override
  Widget build(BuildContext context) {
    final profile = ref.watch(activeProfileProvider);
    final local = profile?.isLocal ?? false;
    final l10n = context.l10n;
    final mismatch = _confirm.text.isNotEmpty && _confirm.text != _passphrase.text;
    return GateScaffold(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          GateHeader(
            icon: Icons.key_rounded,
            title: l10n.createVaultTitle,
            subtitle: local ? l10n.createVaultSubtitleLocal : l10n.createVaultSubtitleSynced,
          ),
          TextField(
            controller: _name,
            decoration: InputDecoration(labelText: l10n.createVaultNameLabel),
          ),
          const SizedBox(height: GlassSpacing.s12),
          SecretField(
            key: const ValueKey('new-passphrase'),
            controller: _passphrase,
            label: l10n.createVaultPassphraseLabel,
            autofocus: true,
            autofillHints: const [AutofillHints.newPassword],
            onChanged: (v) => setState(() => _strength = PassphraseStrength.estimate(v)),
          ),
          const SizedBox(height: GlassSpacing.s8),
          StrengthMeter(strength: _strength),
          PassphraseLayoutNotice(passphrase: _passphrase.text),
          const SizedBox(height: GlassSpacing.s12),
          SecretField(
            key: const ValueKey('confirm-passphrase'),
            controller: _confirm,
            label: l10n.createVaultRepeatPassphraseLabel,
            onChanged: (_) => setState(() {}),
            onSubmitted: (_) => _submit(),
          ),
          if (mismatch)
            Padding(
              padding: const EdgeInsets.only(top: GlassSpacing.s6),
              child: GateErrorText(text: l10n.createVaultPassphraseMismatch),
            ),
          const SizedBox(height: GlassSpacing.s16),
          InfoBanner(message: l10n.createVaultNoResetNotice),
          if (_error != null) ...[
            const SizedBox(height: GlassSpacing.s12),
            GateErrorText(text: errorMessage(l10n, _error!)),
          ],
          const SizedBox(height: GlassSpacing.s20),
          OverflowBar(
            alignment: MainAxisAlignment.spaceBetween,
            overflowAlignment: OverflowBarAlignment.end,
            spacing: GlassSpacing.s8,
            overflowSpacing: GlassSpacing.s8,
            children: [
              GlassButton(onPressed: _busy ? null : _cancel, label: l10n.commonCancel),
              GlassButton.prominent(
                key: const ValueKey('create-vault'),
                size: GlassControlSize.lg,
                busy: _busy,
                onPressed: _canSubmit ? _submit : null,
                label: l10n.createVaultSubmit,
              ),
            ],
          ),
        ],
      ),
    );
  }
}
