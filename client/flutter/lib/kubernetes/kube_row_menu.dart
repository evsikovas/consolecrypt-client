import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

/// Shared by compact lists, desktop tables and Helm releases.
class KubeRowMenu extends StatelessWidget {
  const KubeRowMenu({
    required this.name,
    required this.child,
    this.open,
    this.actions = const [],
    this.onAction,
    this.extraItems,
    super.key,
  });
  final List<GlassMenuEntry<VoidCallback>> Function()? extraItems;
  final String name;
  final Widget child;
  final VoidCallback? open;
  final List<KubeResourceOpenAction> actions;
  final ValueChanged<KubeResourceOpenAction>? onAction;

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    Future<void> menu(Offset at) async {
      final chosen = await showGlassMenu<VoidCallback>(
        context: context,
        anchor: at & Size.zero,
        entries: [
          if (open case final cb?) GlassMenuItem(value: cb, label: l.kubeOverview, icon: Icons.info_outline),
          GlassMenuItem(
            value: () => Clipboard.setData(ClipboardData(text: name)),
            label: l.sftpActionCopyName,
            icon: Icons.copy_outlined,
          ),
          ...?extraItems?.call(),
          for (final a in actions) ...[
            if (a == KubeResourceOpenAction.remove) const GlassMenuDivider(),
            GlassMenuItem(
              value: () => onAction?.call(a),
              label: switch (a) {
                KubeResourceOpenAction.yaml => l.kubeObjectYaml,
                KubeResourceOpenAction.events => l.kubeEvents,
                KubeResourceOpenAction.logs => l.kubeLogs,
                KubeResourceOpenAction.shell => l.kubePodTerminal,
                KubeResourceOpenAction.restart => l.kubeRestart,
                KubeResourceOpenAction.scale => l.kubeScale,
                KubeResourceOpenAction.remove => l.commonDelete,
              },
              icon: switch (a) {
                KubeResourceOpenAction.yaml => Icons.code,
                KubeResourceOpenAction.events => Icons.event_outlined,
                KubeResourceOpenAction.logs => Icons.article_outlined,
                KubeResourceOpenAction.shell => Icons.terminal,
                KubeResourceOpenAction.restart => Icons.restart_alt,
                KubeResourceOpenAction.scale => Icons.tune,
                KubeResourceOpenAction.remove => Icons.delete_outline,
              },
              destructive: a == KubeResourceOpenAction.remove,
            ),
          ],
        ],
      );
      if (context.mounted) chosen?.call();
    }

    return GestureDetector(
      behavior: HitTestBehavior.opaque,
      onSecondaryTapUp: (e) => menu(e.globalPosition),
      onLongPressStart: (e) => menu(e.globalPosition),
      child: child,
    );
  }
}
