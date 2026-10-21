import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/l10n/labels.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Vaults offered a re-enable during their current unlock (one prompt per
/// unlock; "Not now" clears the core's flag for good).
final _offered = <String>{};

/// After a passphrase unlock: offer to switch device unlock on again when
/// the core asks for it — after the update to OS-protected device unlock
/// (it was switched off) or after biometrics changed (the key became
/// invalid). One tap + the OS prompt; Linux shows its warning first.
Future<void> maybeOfferDeviceUnlock(BuildContext context, WidgetRef ref, VaultStatus status) async {
  final info = status.deviceUnlock;
  final reoffer = info.reoffer;
  final key = status.vaultId?.value;
  if (key == null) return;
  if (!status.isUnlocked) {
    _offered.remove(key);
    return;
  }
  if (reoffer == null || info.enabled || !info.canEnable) return;
  if (!_offered.add(key)) return;
  final l = context.l10n;
  final method = info.authName(l);
  final convenience = info.protection == DeviceUnlockProtection.convenience;
  final message = switch (reoffer) {
    DeviceUnlockReoffer.migrated => l.deviceUnlockReofferMigratedMessage,
    DeviceUnlockReoffer.invalidated => l.deviceUnlockReofferInvalidatedMessage(method),
  };
  final yes = await showConfirmDialog(
    context,
    title: switch (reoffer) {
      DeviceUnlockReoffer.migrated => l.deviceUnlockReofferMigratedTitle(method),
      DeviceUnlockReoffer.invalidated => l.deviceUnlockReofferInvalidatedTitle,
    },
    message: convenience ? '$message\n\n${l.deviceUnlockConvenienceWarning}' : message,
    confirmLabel: l.deviceUnlockReofferEnable,
    secure: true,
  );
  if (!context.mounted) return;
  final vault = ref.read(vaultServiceProvider);
  if (!yes) {
    await runWithFeedback(context, vault.dismissDeviceUnlockOffer);
    return;
  }
  await runWithFeedback(
    context,
    () => vault.setDeviceUnlockEnabled(true, reason: l.deviceUnlockEnableReason, allowUnprotected: convenience),
  );
}
