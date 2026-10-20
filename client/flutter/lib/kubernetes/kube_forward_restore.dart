import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// Metadata-only opt-in plans live no longer than the unlocked browser. They
/// cannot start a connection or unlock credentials on their own.
final kubeForwardRestoreProvider = NotifierProvider<KubeForwardRestore, List<KubeForwardRestorePlan>>(
  KubeForwardRestore.new,
);

final class KubeForwardRestorePlan {
  const KubeForwardRestorePlan({
    required this.contextId,
    required this.namespace,
    required this.pod,
    required this.remotePort,
    required this.localPort,
    required this.sessionId,
    required this.forwardId,
    this.localAddress = '127.0.0.1',
    this.approveNetwork = false,
  });
  final String contextId, namespace, pod, sessionId, forwardId;
  final int remotePort, localPort;
  final String localAddress;
  final bool approveNetwork;
  String get key => '$contextId/$namespace/$pod:$remotePort:$localAddress:$localPort';
}

class KubeForwardRestore extends Notifier<List<KubeForwardRestorePlan>> {
  @override
  List<KubeForwardRestorePlan> build() {
    ref.watch(kubeScopeProvider);
    return const [];
  }

  void remember(KubeForwardRestorePlan plan) {
    if (!ref.mounted || !ref.read(kubeScopeProvider).unlocked) return;
    state = [...state.where((old) => old.key != plan.key), plan].take(8).toList();
  }

  void clearContext(String contextId) {
    if (!ref.mounted) return;
    state = state.where((plan) => plan.contextId != contextId).toList();
  }

  void forget(String forwardId) {
    if (!ref.mounted) return;
    state = state.where((plan) => plan.forwardId != forwardId).toList();
  }
}
