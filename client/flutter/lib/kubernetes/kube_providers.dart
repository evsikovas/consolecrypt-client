import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

final kubeServiceProvider = Provider<KubeService>(
  (ref) => ref.watch(appServicesProvider).kubernetes ?? const UnavailableKubeService(),
);

final class KubeScope {
  const KubeScope(this.profileId, this.unlocked);
  final Object? profileId;
  final bool unlocked;
}

final kubeScopeProvider = Provider<KubeScope>(
  (ref) => KubeScope(
    ref.watch(activeProfileProvider.select((p) => p?.id)),
    ref.watch(appStageProvider) == AppStage.unlocked,
  ),
);

final _kubeChangesProvider = StreamProvider.autoDispose<void>((ref) => ref.watch(kubeServiceProvider).changes());

// A new family instance on every lock/profile transition cannot retain the
// previous profile's AsyncData while the next inventory is loading.
final kubeConnectionsProvider = FutureProvider.autoDispose.family<List<KubeConnection>, KubeScope>((ref, scope) async {
  if (!scope.unlocked) return const [];
  ref.watch(_kubeChangesProvider);
  return ref.watch(kubeServiceProvider).inventory();
});
