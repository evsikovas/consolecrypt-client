import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/saved_forward.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_auth_dialog.dart';
import 'package:consolecrypt/kubernetes/kube_forward_panel.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/tunnels/saved_forwards.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class SavedForwardTile extends ConsumerWidget {
  const SavedForwardTile(this.plan, {super.key});
  final SavedForward plan;

  Future<void> _toggle(BuildContext context, WidgetRef ref, SavedForwardManager manager) async {
    try {
      if (manager.handles.containsKey(plan.id)) {
        await manager.stop(plan);
      } else {
        if (plan.network) {
          final ok = await showConfirmDialog(
            context,
            title: context.l10n.kubeForwardNetwork,
            message: context.l10n.kubeForwardNetworkWarning,
            confirmLabel: context.l10n.tunnelsStartSaved,
          );
          if (!ok || !context.mounted || !manager.valid) return;
        }
        try {
          await manager.start(plan, approveNetwork: plan.network);
        } on AppException catch (error) {
          if (plan.kind != SavedForwardKind.kubernetes ||
              !const {'exec_approval_required', 'oidc_login_required'}.contains(error.args['rule'])) {
            rethrow;
          }
          if (!context.mounted || !manager.valid) return;
          final inventory = await ref.read(kubeServiceProvider).inventory();
          if (!context.mounted || !manager.valid) return;
          final connection = inventory.where((c) => c.id == plan.connectionId).firstOrNull;
          if (connection == null) rethrow;
          final ok = await authenticateKubeConnection(context, connection);
          if (!ok || !context.mounted || !manager.valid) return;
          await manager.start(plan, approveNetwork: plan.network);
        }
      }
      if (context.mounted && manager.valid) ref.invalidate(kubeForwardsProvider(ref.read(kubeScopeProvider)));
    } catch (error) {
      if (context.mounted && manager.valid) showSnack(context, errorMessage(context.l10n, error), error: true);
    }
  }

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    ref.watch(savedForwardChangesProvider);
    final manager = ref.watch(savedForwardManagerProvider);
    final handle = manager.handles[plan.id];
    final busy = manager.busy.contains(plan.id);
    final l = context.l10n;
    final host = plan.kind == SavedForwardKind.docker
        ? plan.connectionId == 'local'
              ? l.dockerLocalHost
              : ref.watch(hostByIdProvider).values.where((h) => h.id.value == plan.connectionId).firstOrNull?.name ??
                    l.tunnelsDeletedHost
        : ref
                  .watch(kubeConnectionsProvider(ref.watch(kubeScopeProvider)))
                  .value
                  ?.where((c) => c.id == plan.connectionId)
                  .firstOrNull
                  ?.name ??
              plan.connectionId;
    final address = handle?.address ?? '${plan.localAddress}:${plan.localPort == 0 ? '0' : plan.localPort}';
    final product = plan.kind == SavedForwardKind.docker ? 'Docker' : 'Kubernetes';
    return ListTile(
      key: ValueKey('saved-forward-${plan.id}'),
      leading: Icon(plan.kind == SavedForwardKind.docker ? Icons.view_in_ar_outlined : Icons.hub_outlined),
      title: Text('${plan.name}:${plan.remotePort}', maxLines: 1, overflow: TextOverflow.ellipsis),
      subtitle: Text(
        '$product · $host\n$address · '
        '${busy
            ? l.tunnelsStatusStarting
            : handle == null
            ? l.tunnelsStatusStopped
            : l.dockerForwardRunning}',
      ),
      trailing: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          if (handle != null)
            GlassIconButton(
              icon: Icons.content_copy_rounded,
              tooltip: l.dockerForwardCopyAddress,
              style: GlassIconButtonStyle.plain,
              onPressed: () => ref.read(secureClipboardProvider).copyPlain(handle.address),
            ),
          GlassIconButton(
            key: ValueKey('saved-forward-toggle-${plan.id}'),
            icon: handle == null ? Icons.play_arrow_rounded : Icons.stop_rounded,
            tooltip: handle == null ? l.tunnelsStartSaved : l.tunnelsStopSaved,
            onPressed: busy ? null : () => _toggle(context, ref, manager),
          ),
          GlassIconButton(
            key: ValueKey('saved-forward-remove-${plan.id}'),
            icon: Icons.delete_outline_rounded,
            tooltip: l.commonDelete,
            style: GlassIconButtonStyle.plain,
            onPressed: busy
                ? null
                : () async {
                    final ok = await showConfirmDialog(
                      context,
                      title: l.tunnelsDeleteTitle(plan.name),
                      message: l.tunnelsRemoveSavedMessage,
                      confirmLabel: l.commonDelete,
                      destructive: true,
                    );
                    if (ok && context.mounted && manager.valid) {
                      await runWithFeedback(context, () => manager.remove(plan));
                    }
                  },
          ),
        ],
      ),
    );
  }
}
