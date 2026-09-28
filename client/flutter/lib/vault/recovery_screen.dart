import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/l10n/labels.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/passphrase_strength.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/keyboard_layout.dart';
import 'package:consolecrypt/core/widgets/obscurable_text_controller.dart';
import 'package:consolecrypt/core/widgets/secret_field.dart';
import 'package:consolecrypt/core/widgets/security_widgets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:material_ui/material_ui.dart';

enum RecoveryMethod { recoveryKey, trustedDevice }

/// Vault recovery (CLIENT_SPEC §8.5 / ADR-0004 / ADR-0106): Recovery Key
/// (24 words or QR text) or this trusted device (OS auth), then a new
/// passphrase.
class RecoveryScreen extends ConsumerStatefulWidget {
  const RecoveryScreen({super.key});

  @override
  ConsumerState<RecoveryScreen> createState() => _RecoveryScreenState();
}

class _RecoveryScreenState extends ConsumerState<RecoveryScreen> {
  final _recovery = ObscurableTextController();
  final _passphrase = TextEditingController();
  final _confirm = TextEditingController();
  RecoveryMethod _method = RecoveryMethod.recoveryKey;
  PassphraseStrength _strength = PassphraseStrength.estimate('');
  bool _showRecoveryText = false;
  bool _busy = false;
  AppException? _error;

  @override
  void initState() {
    super.initState();
    if (ref.read(vaultStatusProvider).value?.deviceUnlockAvailable ?? false) {
      _method = RecoveryMethod.trustedDevice;
    }
  }

  @override
  void dispose() {
    _recovery.dispose();
    _passphrase.dispose();
    _confirm.dispose();
    super.dispose();
  }

  bool get _passphraseOk => _strength.acceptable && _passphrase.text == _confirm.text;

  bool get _recoveryOk => RecoveryInput.parse(_recovery.text) != null;

  Future<void> _submit() async {
    setState(() {
      _busy = true;
      _error = null;
    });
    final newPass = SecretText(_passphrase.text);
    final recovery = SecretText(_recovery.text);
    try {
      final vault = ref.read(vaultServiceProvider);
      if (_method == RecoveryMethod.recoveryKey) {
        await vault.recoverWithRecoveryKey(recoveryInput: recovery, newPassphrase: newPass);
      } else {
        await vault.resetPassphraseWithTrustedDevice(newPassphrase: newPass);
      }
      _recovery.clear();
      _passphrase.clear();
      _confirm.clear();
      if (mounted) context.go(AppRoutes.hosts);
    } on AppException catch (e) {
      if (mounted) setState(() => _error = e);
    } finally {
      newPass.wipe();
      recovery.wipe();
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final status = ref.watch(vaultStatusProvider).value;
    final profile = ref.watch(activeProfileProvider);
    final trusted = status?.deviceTrusted ?? false;
    final deviceAvailable = status?.deviceUnlockAvailable ?? false;
    final tokens = GlassTokens.of(context);
    final t = tokens.typography;
    final l10n = context.l10n;
    final words = RecoveryInput.countWords(_recovery.text);
    final canSubmit =
        !_busy && _passphraseOk && (_method == RecoveryMethod.trustedDevice ? deviceAvailable : _recoveryOk);
    return GateScaffold(
      maxWidth: 620,
      leading: GlassButton(
        onPressed: () => context.go(AppRoutes.unlock),
        icon: Icons.arrow_back_rounded,
        label: l10n.commonBack,
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          GateHeader(icon: Icons.health_and_safety_rounded, title: l10n.recoveryTitle, subtitle: l10n.recoverySubtitle),
          GlassSegmented<RecoveryMethod>(
            key: const ValueKey('recovery-method'),
            inChrome: false,
            expand: true,
            segments: [
              GlassSegment(
                value: RecoveryMethod.recoveryKey,
                icon: Icons.key_rounded,
                label: l10n.recoveryMethodRecoveryKey,
              ),
              GlassSegment(
                value: RecoveryMethod.trustedDevice,
                icon: Icons.fingerprint_rounded,
                label: l10n.recoveryMethodTrustedDevice,
              ),
            ],
            selected: _method,
            onChanged: (m) => setState(() {
              _method = m;
              _error = null;
            }),
          ),
          const SizedBox(height: GlassSpacing.s16),
          if (_method == RecoveryMethod.recoveryKey) ...[
            Text(l10n.recoveryWordsInstructions, style: t.body.copyWith(color: tokens.palette.label)),
            const SizedBox(height: GlassSpacing.s8),
            // The words are never blurred: while hidden, each character is
            // drawn as a bullet, so the secret is not rendered at all (§4.14).
            TextField(
              key: const ValueKey('recovery-input'),
              controller: _recovery,
              minLines: 3,
              maxLines: 5,
              autocorrect: false,
              enableSuggestions: false,
              enableIMEPersonalizedLearning: false,
              style: t.mono.copyWith(color: tokens.palette.label),
              decoration: InputDecoration(
                hintText: l10n.recoveryInputHint,
                suffixIcon: Align(
                  alignment: Alignment.topCenter,
                  widthFactor: 1,
                  heightFactor: 1,
                  child: IconButton(
                    tooltip: _showRecoveryText ? l10n.commonHide : l10n.commonShow,
                    icon: Icon(_showRecoveryText ? Icons.visibility_off_rounded : Icons.visibility_rounded, size: 18),
                    onPressed: () => setState(() {
                      _showRecoveryText = !_showRecoveryText;
                      _recovery.obscured = !_showRecoveryText;
                    }),
                  ),
                ),
              ),
              onChanged: (_) => setState(() => _error = null),
            ),
            const SizedBox(height: GlassSpacing.s4),
            Text(
              _recovery.text.trim().startsWith(RecoveryInput.qrPrefix)
                  ? (_recoveryOk ? l10n.recoveryQrRecognised : l10n.recoveryQrIncomplete)
                  : l10n.recoveryWordCount(words, RecoveryKit.wordCount),
              style: t.callout.copyWith(color: tokens.secondaryLabel),
            ),
            // TODO(client/ui): scan the Recovery Kit QR with the camera — needs a camera
            // plugin; desktop users paste the QR text for now; next: evaluate at M5.
          ] else ...[
            if (deviceAvailable)
              InfoBanner(message: l10n.recoveryTrustedDeviceInfo(status!.deviceUnlock.authName(l10n)))
            else if (trusted)
              InfoBanner(tone: BannerTone.warning, message: l10n.deviceUnlockRecoveryUnavailable)
            else
              InfoBanner(tone: BannerTone.warning, message: l10n.recoveryDeviceNotTrusted),
          ],
          const SizedBox(height: GlassSpacing.s16),
          SecretField(
            key: const ValueKey('recovery-new-passphrase'),
            controller: _passphrase,
            label: l10n.recoveryNewPassphraseLabel,
            onChanged: (v) => setState(() => _strength = PassphraseStrength.estimate(v)),
          ),
          const SizedBox(height: GlassSpacing.s8),
          StrengthMeter(strength: _strength),
          PassphraseLayoutNotice(passphrase: _passphrase.text),
          const SizedBox(height: GlassSpacing.s12),
          SecretField(
            key: const ValueKey('recovery-confirm'),
            controller: _confirm,
            label: l10n.recoveryRepeatNewPassphraseLabel,
            onChanged: (_) => setState(() {}),
          ),
          if (_error != null) ...[
            const SizedBox(height: GlassSpacing.s12),
            GateErrorText(key: const ValueKey('recovery-error'), text: errorMessage(l10n, _error!)),
          ],
          const SizedBox(height: GlassSpacing.s20),
          GlassButton.prominent(
            key: const ValueKey('recovery-submit'),
            size: GlassControlSize.lg,
            expand: true,
            busy: _busy,
            onPressed: canSubmit ? _submit : null,
            label: _method == RecoveryMethod.trustedDevice
                ? l10n.recoverySubmitTrustedDevice
                : l10n.recoverySubmitRecoveryKey,
          ),
          const SizedBox(height: GlassSpacing.s12),
          ExpansionTile(
            tilePadding: EdgeInsets.zero,
            title: Text(l10n.recoveryLostEverythingTitle, style: t.bodyEmph.copyWith(color: tokens.palette.label)),
            children: [
              Text(
                profile?.isLocal ?? false ? l10n.recoveryLostEverythingLocal : l10n.recoveryLostEverythingSynced,
                style: t.body.copyWith(color: tokens.palette.label),
              ),
              const SizedBox(height: GlassSpacing.s8),
            ],
          ),
        ],
      ),
    );
  }
}
