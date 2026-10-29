import 'dart:async';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
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

/// Cluster sessions kept across a lock (often the idle auto-lock). The lock
/// screen disposes the browser, but the native session with its terminals
/// and forwards lives on; the next browser of that cluster adopts it. A
/// profile change forgets them: the core closes them with the profile.
final kubeParkedSessionsProvider = NotifierProvider<KubeParkedSessions, Map<String, KubeSession>>(
  KubeParkedSessions.new,
);

final class KubeParkedSessions extends Notifier<Map<String, KubeSession>> {
  final _sessions = <String, KubeSession>{};

  @override
  Map<String, KubeSession> build() {
    ref.watch(activeProfileProvider.select((p) => p?.id));
    _sessions.clear();
    return const {};
  }

  void park(KubeSession session) {
    if (!ref.mounted) return;
    _sessions[session.contextId] = session;
    state = Map.unmodifiable(_sessions);
  }

  /// Takes the session at once; listeners hear of it after the current
  /// build (a browser adopts it from initState).
  KubeSession? take(String contextId) {
    final session = _sessions.remove(contextId);
    if (session != null) {
      scheduleMicrotask(() {
        if (ref.mounted) state = Map.unmodifiable(_sessions);
      });
    }
    return session;
  }
}
