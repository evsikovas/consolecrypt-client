import 'dart:async';

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/models/saved_forward.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/docker/docker_providers.dart';
import 'package:consolecrypt/docker/docker_service.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_forward_restore.dart';
import 'package:consolecrypt/kubernetes/kube_forward_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

final savedForwardManagerProvider = Provider<SavedForwardManager>((ref) {
  final profile = ref.watch(activeProfileProvider.select((p) => p?.id.value));
  final unlocked = ref.watch(vaultStatusProvider.select((s) => s.value?.isUnlocked == true));
  final manager = SavedForwardManager(ref, profile ?? '', unlocked);
  ref.onDispose(manager.dispose);
  return manager;
});
final savedForwardChangesProvider = StreamProvider<int>((ref) => ref.watch(savedForwardManagerProvider).changes);
final savedForwardsProvider = Provider<List<SavedForward>>((ref) {
  final manager = ref.watch(savedForwardManagerProvider);
  final settings = ref.watch(localSettingsProvider).value;
  return !manager.valid
      ? const []
      : [
          for (final plan in settings?.savedForwards ?? const <SavedForward>[])
            if (plan.profile == manager.profile) plan,
        ];
});

final class ForwardHandle {
  const ForwardHandle(this.kind, this.sessionId, this.forwardId, this.address, {this.owned = false});
  final SavedForwardKind kind;
  final String sessionId, forwardId, address;
  final bool owned;
}

// A loopback endpoint may already exist in another browser session (for
// example after reconnecting). Bind only once, and never adopt by display name.
ForwardHandle? _runningPreset(SavedForward plan, List<DockerForward> docker, List<KubeForward> kube) {
  if (plan.network) return null;
  final host = plan.localAddress == 'localhost' ? '127.0.0.1' : plan.localAddress;
  bool sameListener(String address, int? requestedPort) {
    final endpoint = Uri.tryParse('tcp://$address');
    if (endpoint == null || endpoint.host != host || !endpoint.hasPort) return false;
    if (plan.localPort == 0) return requestedPort == 0;
    return (requestedPort == null || requestedPort == plan.localPort) && endpoint.port == plan.localPort;
  }

  if (plan.kind == SavedForwardKind.docker) {
    for (final f in docker) {
      if (f.state == DockerForwardState.running &&
          f.hostId == plan.connectionId &&
          f.containerName == plan.target &&
          f.remotePort == plan.remotePort &&
          sameListener(f.address, f.requestedLocalPort)) {
        return ForwardHandle(plan.kind, f.sessionId, f.id, f.address);
      }
    }
  } else {
    for (final f in kube) {
      if (f.state == 'running' &&
          f.contextId == plan.connectionId &&
          f.namespace == plan.namespace &&
          f.pod == plan.target &&
          f.remotePort == plan.remotePort &&
          sameListener(f.address, f.requestedLocalPort)) {
        return ForwardHandle(plan.kind, f.sessionId, f.id, f.address);
      }
    }
  }
  return null;
}

/// Saves metadata only. Starting always goes through the current native
/// services, which enforce host-key, credential and production policies.
class SavedForwardManager {
  SavedForwardManager(this.ref, this.profile, this.unlocked);
  final Ref ref;
  final String profile;
  final bool unlocked;
  bool _disposed = false;
  bool get valid => !_disposed && unlocked && profile.isNotEmpty;
  final handles = <String, ForwardHandle>{};
  final busy = <String>{};
  final _cleanup = <String, Future<void> Function()>{};
  final _changes = StreamController<int>.broadcast();
  int _revision = 0;
  Future<void> _settingsTail = Future.value();
  Stream<int> get changes async* {
    yield _revision;
    yield* _changes.stream;
  }

  void _notify() {
    if (!_disposed) _changes.add(++_revision);
  }

  void _check() {
    if (!valid) throw const AppException(AppErrorCode.cancelled, 'Forwarding scope expired'); // l10n-ignore: diagnostic
  }

  void _checkPlan(SavedForward plan) {
    _check();
    if (plan.profile != profile) {
      throw const AppException(AppErrorCode.cancelled, 'Wrong profile'); // l10n-ignore: diagnostic
    }
  }

  Future<SavedForward> save(SavedForward plan) async {
    _check();
    if (plan.profile != profile) {
      throw const AppException(AppErrorCode.cancelled, 'Wrong profile'); // l10n-ignore: diagnostic
    }
    final settings = ref.read(settingsServiceProvider);
    SavedForward saved = plan;
    final operation = _settingsTail.catchError((Object _) {}).then((_) async {
      _check();
      final current = settings.currentLocal;
      saved = current.savedForwards.where((old) => old.identity == plan.identity).firstOrNull ?? plan;
      await settings.updateLocal(
        current.copyWith(savedForwards: [...current.savedForwards.where((old) => old.id != saved.id), saved]),
      );
    });
    _settingsTail = operation;
    await operation;
    _check();
    return saved;
  }

  void attach(SavedForward plan, ForwardHandle handle) {
    _check();
    handles[plan.id] = handle;
    _notify();
  }

  Future<void> remove(SavedForward plan) async {
    _checkPlan(plan);
    if (busy.contains(plan.id)) {
      throw const AppException(AppErrorCode.conflict, 'Forward is busy'); // l10n-ignore: diagnostic
    }
    await stop(plan);
    _check();
    final settings = ref.read(settingsServiceProvider);
    final operation = _settingsTail.catchError((Object _) {}).then((_) async {
      _check();
      final current = settings.currentLocal;
      await settings.updateLocal(
        current.copyWith(
          savedForwards: [
            for (final p in current.savedForwards)
              if (p.id != plan.id || p.profile != profile) p,
          ],
        ),
      );
    });
    _settingsTail = operation;
    await operation;
  }

  Future<void> start(SavedForward plan, {bool approveNetwork = false}) async {
    _checkPlan(plan);
    if (busy.contains(plan.id) || handles.containsKey(plan.id)) return;
    busy.add(plan.id);
    _notify();
    try {
      if (plan.kind == SavedForwardKind.docker) {
        final service = ref.read(dockerServiceProvider);
        if (service == null) {
          throw const AppException(AppErrorCode.unsupported, 'Docker unavailable'); // l10n-ignore: diagnostic
        }
        if (!plan.network) {
          final running = _runningPreset(plan, await service.forwards(), const []);
          _check();
          if (running != null) {
            attach(plan, running);
            return;
          }
        }
        final session = await service.connect(plan.connectionId);
        _cleanup[plan.id] = () => service.disconnect(session.id);
        try {
          _check();
          // Native resolves the saved name on the current engine, even after recreation.
          final forward = await service.startForward(
            session.id,
            plan.target,
            remotePort: plan.remotePort,
            localPort: plan.localPort,
          );
          if (!valid) {
            await service.stopForward(forward.id);
            _check();
          }
          attach(plan, ForwardHandle(plan.kind, session.id, forward.id, forward.address, owned: true));
          ref.invalidate(dockerForwardsProvider);
        } catch (_) {
          _cleanup.remove(plan.id);
          await service.disconnect(session.id).catchError((Object _) {});
          rethrow;
        }
      } else {
        final service = ref.read(kubeServiceProvider);
        if (service is! KubeBrowserService || service is! KubeForwardService) {
          throw const AppException(
            AppErrorCode.unsupported,
            'Kubernetes forwarding unavailable', // l10n-ignore: diagnostic; AppErrorCode is localized by the UI.
          ); // l10n-ignore: diagnostic
        }
        final browser = service as KubeBrowserService, forwards = service as KubeForwardService;
        if (!plan.network) {
          final running = _runningPreset(plan, const [], await forwards.forwards());
          _check();
          if (running != null) {
            attach(plan, running);
            return;
          }
        }
        final session = await browser.connect(plan.connectionId);
        _cleanup[plan.id] = () => browser.disconnect(session.id);
        try {
          _check();
          if (!session.canPortForward ||
              (plan.network &&
                  (!approveNetwork ||
                      session.production ||
                      AppPlatform.isMobile ||
                      service is! KubeNetworkForwardService))) {
            throw const AppException(
              AppErrorCode.unsupported,
              'Forwarding blocked by current policy', // l10n-ignore: diagnostic; AppErrorCode is localized by the UI.
            ); // l10n-ignore: diagnostic
          }
          final forward = plan.network
              ? await (service as KubeNetworkForwardService).startNetworkForward(
                  session.id,
                  namespace: plan.namespace,
                  pod: plan.target,
                  remotePort: plan.remotePort,
                  localPort: plan.localPort,
                  localAddress: plan.localAddress,
                  approveNetwork: true,
                )
              : await forwards.startForward(
                  session.id,
                  namespace: plan.namespace,
                  pod: plan.target,
                  remotePort: plan.remotePort,
                  localPort: plan.localPort,
                );
          if (!valid) {
            await forwards.stopForward(session.id, forward.id);
            _check();
          }
          attach(plan, ForwardHandle(plan.kind, session.id, forward.id, forward.address, owned: true));
        } catch (_) {
          _cleanup.remove(plan.id);
          await browser.disconnect(session.id).catchError((Object _) {});
          rethrow;
        }
      }
    } finally {
      busy.remove(plan.id);
      _notify();
    }
  }

  Future<void> stop(SavedForward plan) async {
    _checkPlan(plan);
    final handle = handles[plan.id];
    if (handle == null || busy.contains(plan.id)) return;
    busy.add(plan.id);
    _notify();
    try {
      if (handle.kind == SavedForwardKind.docker) {
        final service = ref.read(dockerServiceProvider);
        await service?.stopForward(handle.forwardId);
        if (handle.owned) await service?.disconnect(handle.sessionId);
      } else {
        final service = ref.read(kubeServiceProvider);
        if (service is KubeForwardService) {
          await (service as KubeForwardService).stopForward(handle.sessionId, handle.forwardId);
          if (valid) ref.read(kubeForwardRestoreProvider.notifier).forget(handle.forwardId);
        }
        if (handle.owned && service is KubeBrowserService) {
          await (service as KubeBrowserService).disconnect(handle.sessionId);
        }
      }
      handles.remove(plan.id);
      _cleanup.remove(plan.id);
    } finally {
      busy.remove(plan.id);
      _notify();
    }
  }

  /// Reconciles stops made in the shared forwards panel.
  Future<void> refresh() async {
    if (!valid) return;
    final plans = ref
        .read(settingsServiceProvider)
        .currentLocal
        .savedForwards
        .where((p) => p.profile == profile)
        .toList();
    if (handles.isEmpty && plans.isEmpty) return;
    final docker = ref.read(dockerServiceProvider), kube = ref.read(kubeServiceProvider);
    final snapshot = Map.of(handles);
    final revision = _revision;
    try {
      final d = docker == null ? <DockerForward>[] : await docker.forwards();
      final k = kube is KubeForwardService ? await (kube as KubeForwardService).forwards() : <KubeForward>[];
      if (!valid || revision != _revision) return;
      for (final entry in snapshot.entries) {
        final h = entry.value;
        if (busy.contains(entry.key) || !identical(handles[entry.key], h)) continue;
        final running = h.kind == SavedForwardKind.docker
            ? d.any((f) => f.id == h.forwardId && f.state == DockerForwardState.running)
            : k.any((f) => f.id == h.forwardId && f.state == 'running');
        if (!running) {
          handles.remove(entry.key);
          final cleanup = _cleanup.remove(entry.key);
          if (cleanup != null) unawaited(cleanup().catchError((Object _) {}));
        }
      }
      for (final plan in plans) {
        if (handles.containsKey(plan.id) || busy.contains(plan.id)) continue;
        final running = _runningPreset(plan, d, k);
        if (running != null) handles[plan.id] = running;
      }
      _notify();
    } catch (_) {
      /* The explicit start/stop actions report errors. */
    }
  }

  void dispose() {
    if (_disposed) return;
    _disposed = true;
    for (final cleanup in _cleanup.values) {
      unawaited(cleanup().catchError((Object _) {}));
    }
    _cleanup.clear();
    unawaited(_changes.close());
    // Native sessions are scoped to the vault/profile. Do not disconnect
    // sessions borrowed from a Docker/Kubernetes browser.
  }
}
