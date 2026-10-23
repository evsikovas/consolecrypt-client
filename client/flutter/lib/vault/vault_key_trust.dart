import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/vault/vault_passphrase_confirmation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Vault key trust of the open profile (audit H2).
final vaultKeyStatusProvider = FutureProvider.autoDispose<VaultKeyStatus>((ref) async {
  ref.watch(activeProfileProvider);
  if (ref.watch(vaultStatusProvider).value?.phase != VaultPhase.unlocked) return const VaultKeyStatus();
  ref.watch(devicesProvider);
  return ref.watch(devicesServiceProvider).vaultKeyStatus();
});

/// TR-7: "Vault key not confirmed" with "Confirm with passphrase" — on the
/// devices screen and in the vault settings.
class VaultKeyUnconfirmedBanner extends ConsumerWidget {
  const VaultKeyUnconfirmedBanner({super.key});
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final status = ref.watch(vaultKeyStatusProvider).value;
    if (status == null || !status.unconfirmed) return const SizedBox.shrink();
    final l = context.l10n;
    return Padding(
      padding: const EdgeInsets.only(bottom: GlassSpacing.s12),
      child: InfoBanner(
        key: const ValueKey('vault-key-unconfirmed'),
        tone: BannerTone.warning,
        icon: Icons.key_off_rounded,
        title: l.devicesVaultKeyUnconfirmedTitle,
        message: l.devicesVaultKeyUnconfirmedMessage,
        action: GlassButton(
          key: const ValueKey('vault-key-confirm'),
          icon: Icons.key_rounded,
          label: l.devicesVaultKeyConfirm,
          onPressed: () => showAppDialog<void>(context, secure: true, builder: (_) => const VaultKeyConfirmDialog()),
        ),
      ),
    );
  }
}

/// TR-5: devices of the account that reported a different vault key.
class VaultKeyConflictBanners extends ConsumerWidget {
  const VaultKeyConflictBanners({required this.snapshot, required this.onRevoke, super.key});
  final DevicesSnapshot snapshot;
  final void Function(DeviceInfo device) onRevoke;
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l = context.l10n;
    final conflicting = [
      for (final id in snapshot.vaultKeyConflicts)
        ?snapshot.devices.where((d) => d.deviceId == id && !d.isRevoked).firstOrNull,
    ];
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        for (final d in conflicting)
          Padding(
            padding: const EdgeInsets.only(bottom: GlassSpacing.s12),
            child: InfoBanner(
              key: ValueKey('vault-key-conflict-${d.deviceId.value}'),
              tone: BannerTone.danger,
              title: l.devicesVaultKeyConflictTitle,
              message: l.devicesVaultKeyConflictMessage(d.name),
              action: GlassButton(
                style: GlassButtonStyle.destructiveQuiet,
                label: l.devicesRevokeConflicting,
                onPressed: () => onRevoke(d),
              ),
            ),
          ),
      ],
    );
  }
}

class VaultKeyConfirmDialog extends ConsumerStatefulWidget {
  const VaultKeyConfirmDialog({super.key});
  @override
  ConsumerState<VaultKeyConfirmDialog> createState() => _VaultKeyConfirmDialogState();
}

class _VaultKeyConfirmDialogState extends ConsumerState<VaultKeyConfirmDialog> {
  final _passphrase = TextEditingController();
  bool _busy = false, _wrong = false;
  Object? _error;

  @override
  void dispose() {
    _passphrase
      ..clear()
      ..dispose();
    super.dispose();
  }

  Future<void> _confirm() async {
    if (_busy || _passphrase.text.isEmpty) return;
    setState(() {
      _busy = true;
      _wrong = false;
      _error = null;
    });
    final secret = SecretText(_passphrase.text);
    _passphrase.clear();
    try {
      final ok = await ref.read(devicesServiceProvider).confirmVaultKey(secret);
      if (!mounted) return;
      if (ok) {
        ref.invalidate(vaultKeyStatusProvider);
        showSnack(context, context.l10n.vaultKeyConfirmed, tone: GlassTone.success);
        closeDialog<void>(context);
        return;
      }
      setState(() => _wrong = true);
    } on AppException catch (e) {
      // A different key raises the blocking mismatch screen as well.
      if (mounted) setState(() => _error = e);
    } finally {
      secret.wipe();
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return GlassDialog(
      key: const ValueKey('vault-key-confirm-dialog'),
      title: l.vaultKeyConfirmTitle,
      icon: Icons.key_rounded,
      width: 480,
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(l.devicesVaultKeyUnconfirmedMessage),
          const SizedBox(height: GlassSpacing.s12),
          VaultPassphraseField(
            key: const ValueKey('vault-key-confirm-passphrase'),
            controller: _passphrase,
            wrong: _wrong,
            enabled: !_busy,
            onChanged: (_) => setState(() => _wrong = false),
            onSubmitted: (_) => _confirm(),
          ),
          if (_error != null) ...[const SizedBox(height: GlassSpacing.s8), Text(errorMessage(l, _error!))],
        ],
      ),
      secondaryActions: [
        GlassButton(label: l.commonCancel, onPressed: _busy ? null : () => closeDialog<void>(context)),
      ],
      primaryAction: GlassButton.prominent(
        key: const ValueKey('vault-key-confirm-submit'),
        label: l.devicesVaultKeyConfirm,
        onPressed: _busy ? null : _confirm,
      ),
    );
  }
}
