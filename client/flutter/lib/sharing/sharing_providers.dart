import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/enrollment_service.dart';
import 'package:consolecrypt/core/services/sharing_service.dart';
import 'package:consolecrypt/sharing/sharing_models.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// Identity token for one unlocked UI session. A lock/profile transition
/// invalidates old previews and confirmations even if the same profile unlocks.
final class SharingSessionScope {
  const SharingSessionScope(this.profile, this.unlocked);
  final Object? profile;
  final bool unlocked;
}

final sharingSessionScopeProvider = Provider<SharingSessionScope>(
  (ref) => SharingSessionScope(
    ref.watch(activeProfileProvider.select((p) => p?.id)),
    ref.watch(vaultStatusProvider.select((s) => s.value?.phase)) == VaultPhase.unlocked,
  ),
);

final sharingServiceProvider = Provider<SharingService>(
  (ref) => ref.watch(appServicesProvider).sharing ?? const UnavailableSharingService(),
);

final sharingStatusProvider = FutureProvider.autoDispose<SharingStatus>((ref) async {
  final profile = ref.watch(activeProfileProvider);
  final phase = ref.watch(vaultStatusProvider).value?.phase;
  if (profile == null || profile.isLocal || phase != VaultPhase.unlocked) {
    return const SharingStatus(locked: true);
  }
  return ref.watch(sharingServiceProvider).status();
});

final sharingItemsProvider = FutureProvider.autoDispose<List<SharingItem>>((ref) async {
  ref.watch(activeProfileProvider);
  final phase = ref.watch(vaultStatusProvider).value?.phase;
  if (phase != VaultPhase.unlocked) return const [];
  final status = await ref.watch(sharingStatusProvider.future);
  if (!status.enabled || status.locked) return const [];
  return ref.watch(sharingServiceProvider).list();
});

final sharingOutboxProvider = FutureProvider.autoDispose<List<SharingOutboxEntry>>((ref) async {
  ref.watch(activeProfileProvider);
  if (ref.watch(vaultStatusProvider).value?.phase != VaultPhase.unlocked) return const [];
  return ref.watch(sharingServiceProvider).outbox();
});

void refreshSharing(WidgetRef ref) {
  ref.invalidate(sharingStatusProvider);
  ref.invalidate(sharingItemsProvider);
  ref.invalidate(sharingOutboxProvider);
}

final enrollmentServiceProvider = Provider<EnrollmentService>(
  (ref) => ref.watch(appServicesProvider).enrollment ?? const UnavailableEnrollmentService(),
);
