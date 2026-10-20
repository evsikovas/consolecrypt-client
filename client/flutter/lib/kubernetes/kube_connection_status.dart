import 'dart:async';

import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

enum KubeConnectionState { checking, available, forbidden, failed }

final class KubeConnectionStatus {
  const KubeConnectionStatus(this.state, {this.serverVersion});
  final KubeConnectionState state;
  final String? serverVersion;
}

final class KubeConnectionCheck {
  KubeConnectionCheck(this.scope, this.contextId);
  final KubeScope scope;
  final String contextId;
}

/// Last explicit connection result, never a background reachability probe.
/// Neither results nor native session handles are persisted or synchronized.
final kubeConnectionStatusProvider = NotifierProvider<KubeConnectionStatuses, Map<String, KubeConnectionStatus>>(
  KubeConnectionStatuses.new,
);

final class KubeConnectionStatuses extends Notifier<Map<String, KubeConnectionStatus>> {
  final _runs = <String, KubeConnectionCheck>{};

  @override
  Map<String, KubeConnectionStatus> build() {
    final scope = ref.watch(kubeScopeProvider);
    _runs.clear();
    // A refreshed inventory can reflect changed keys/routes not present in its
    // display DTO. Drop previous results instead of presenting stale assurance.
    ref.listen(kubeConnectionsProvider(scope), (previous, next) {
      if (previous?.value != null && next.hasValue && !identical(previous?.value, next.value)) {
        _runs.clear();
        state = const {};
      }
    });
    return const {};
  }

  KubeConnectionCheck begin(KubeScope scope, String contextId) {
    final run = KubeConnectionCheck(scope, contextId);
    if (ref.mounted && scope.unlocked && identical(scope, ref.read(kubeScopeProvider))) {
      _runs[contextId] = run;
      _report(run, const KubeConnectionStatus(KubeConnectionState.checking));
    }
    return run;
  }

  void available(KubeConnectionCheck run, String version) =>
      _report(run, KubeConnectionStatus(KubeConnectionState.available, serverVersion: version));

  void failed(KubeConnectionCheck run, Object error) => _report(
    run,
    KubeConnectionStatus(
      error is AppException && (error.code == AppErrorCode.forbidden || error.args['rule'] == 'forbidden')
          ? KubeConnectionState.forbidden
          : KubeConnectionState.failed,
    ),
  );

  void cancel(KubeConnectionCheck run) {
    if (!ref.mounted || !identical(_runs[run.contextId], run)) return;
    _runs.remove(run.contextId);
    scheduleMicrotask(() {
      if (ref.mounted &&
          identical(run.scope, ref.read(kubeScopeProvider)) &&
          !_runs.containsKey(run.contextId) &&
          state[run.contextId]?.state == KubeConnectionState.checking) {
        state = {...state}..remove(run.contextId);
      }
    });
  }

  void _report(KubeConnectionCheck run, KubeConnectionStatus result) {
    // Browser initialization runs while widgets build. Publishing on the next
    // microtask also gives lock and inventory invalidations a generation fence.
    scheduleMicrotask(() {
      if (!ref.mounted ||
          !run.scope.unlocked ||
          !identical(run.scope, ref.read(kubeScopeProvider)) ||
          !identical(_runs[run.contextId], run)) {
        return;
      }
      state = {...state, run.contextId: result};
    });
  }
}
