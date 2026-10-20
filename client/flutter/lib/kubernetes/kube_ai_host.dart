import 'package:consolecrypt/kubernetes/kube_ai_service.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// The connected cluster whose assistant the app's right AI panel can show.
/// Published by the desktop cluster workspace while its native session is
/// open; cleared on disconnect, lock and profile change.
final class KubeAiHost {
  const KubeAiHost({
    required this.session,
    required this.contextName,
    required this.service,
    required this.namespace,
    this.selected,
    this.question,
    this.request = 0,
    this.onReviewedCommand,
  });
  final KubeSession session;
  final String contextName;
  final KubeAiService service;
  final String namespace;
  final KubeResourceRef? selected;
  final String? question;

  /// Increments for every explicit "ask AI" so the panel adopts the new
  /// object/question even when the session is unchanged.
  final int request;
  final Future<void> Function(String runtimeId, String command)? onReviewedCommand;
}

final kubeAiHostProvider = NotifierProvider<KubeAiHostController, KubeAiHost?>(KubeAiHostController.new);

/// Whether the right AI panel shows the cluster assistant (true) or the
/// general chat (false) while a cluster is connected.
final kubeAiClusterModeProvider = NotifierProvider<KubeAiClusterMode, bool>(KubeAiClusterMode.new);

class KubeAiClusterMode extends Notifier<bool> {
  @override
  bool build() => true;
  void set(bool value) => state = value;
}

class KubeAiHostController extends Notifier<KubeAiHost?> {
  @override
  KubeAiHost? build() => null;

  void publish(KubeAiHost host) {
    if (ref.mounted) state = host;
  }

  /// Only the owner session may clear its host.
  void clear(String sessionId) {
    if (ref.mounted && state?.session.id == sessionId) state = null;
  }
}
