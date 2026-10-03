import 'dart:async';

import 'package:consolecrypt/core/bridge/rust_rdp_service.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/rdp/rdp_controller.dart';
import 'package:consolecrypt/rdp/rdp_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

final rdpServiceProvider = Provider<RdpService>(
  (ref) => useMockBackend ? const UnavailableRdpService() : const RustRdpService(),
);

/// Every profile/lock transition produces a new identity, including a quick
/// lock/unlock. Dialogs and delayed operations compare the captured identity.
final class RdpScope {
  const RdpScope(this.profileId, this.unlocked);
  final ProfileId? profileId;
  final bool unlocked;
}

final rdpScopeProvider = Provider<RdpScope>((ref) {
  final profile = ref.watch(activeProfileProvider.select((value) => value?.id));
  final status = ref.watch(
    vaultStatusProvider.select(
      (value) => (
        value is AsyncData<VaultStatus> ? value.value.phase : null,
        value is AsyncData<VaultStatus> && value.value.recoveryKitConfirmed,
      ),
    ),
  );
  final unlocked = status.$1 == VaultPhase.unlocked && status.$2 && profile != null;
  return RdpScope(profile, unlocked);
});

final rdpWorkspaceProvider = Provider<RdpWorkspaceController>((ref) {
  final controller = RdpWorkspaceController(ref.watch(rdpServiceProvider));
  ref.listen(rdpScopeProvider, (previous, next) {
    if (!identical(previous, next)) unawaited(controller.closeAll());
  });
  ref.onDispose(controller.dispose);
  return controller;
});
