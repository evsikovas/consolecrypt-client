import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/docker/docker_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// Root installs the native adapter. Null means Docker is unavailable in
/// this build (the section says so instead of showing an empty list).
final dockerServiceProvider = Provider<DockerService?>((ref) => ref.watch(appServicesProvider).docker);

/// Profile and lock state the Docker section belongs to: a change closes the
/// open engine session.
final class DockerScope {
  const DockerScope(this.profileId, this.unlocked);
  final Object? profileId;
  final bool unlocked;
}

final dockerScopeProvider = Provider<DockerScope>(
  (ref) => DockerScope(
    ref.watch(activeProfileProvider.select((p) => p?.id)),
    ref.watch(appStageProvider) == AppStage.unlocked,
  ),
);

/// Running Docker port forwards (all engine sessions). Kept apart from the
/// Kubernetes forwards on purpose.
final dockerForwardsProvider = FutureProvider.autoDispose<List<DockerForward>>((ref) async {
  final scope = ref.watch(dockerScopeProvider);
  final service = ref.watch(dockerServiceProvider);
  if (!scope.unlocked || service == null) return const [];
  return service.forwards();
});
