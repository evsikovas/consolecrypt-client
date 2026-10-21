import 'package:consolecrypt/app/features.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/docker/docker_forwards.dart';
import 'package:consolecrypt/docker/docker_providers.dart';
import 'package:consolecrypt/docker/docker_service.dart';
import 'package:consolecrypt/kubernetes/kube_forward_panel.dart';
import 'package:consolecrypt/kubernetes/kube_forward_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Running forwards of the right workspace panel: Kubernetes and Docker,
/// each in its own section, one empty state when neither has any.
class WorkspaceForwardsPanel extends ConsumerWidget {
  const WorkspaceForwardsPanel({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    final scope = ref.watch(kubeScopeProvider);
    final kube = kubernetesEnabled && scope.unlocked
        ? (ref.watch(kubeForwardsProvider(scope)).value ?? const <KubeForward>[])
        : const <KubeForward>[];
    final docker = ref.watch(dockerServiceProvider);
    final dockerForwards = dockerEnabled && docker != null
        ? (ref.watch(dockerForwardsProvider).value ?? const <DockerForward>[])
        : const <DockerForward>[];
    Widget heading(String text) => Padding(
      padding: const EdgeInsets.only(top: GlassSpacing.s4, bottom: GlassSpacing.s6),
      child: Text(text, style: tokens.typography.bodyEmph.copyWith(color: tokens.palette.label)),
    );
    return ListView(
      key: const ValueKey('workspace-forwards'),
      padding: const EdgeInsets.all(GlassSpacing.s12),
      children: [
        if (kube.isEmpty && dockerForwards.isEmpty)
          EmptyState(
            icon: Icons.swap_horiz_rounded,
            title: l.kubeNoForwards,
            message: l.workspaceForwardsEmptyHelp,
            compact: true,
          ),
        // The sections keep polling while shown, so they stay mounted even
        // when empty (a new forward appears without reopening the panel).
        if (kubernetesEnabled)
          const KubeForwardsPanel(title: 'Kubernetes', bounded: false), // l10n-ignore: product name
        if (dockerEnabled && docker != null) ...[
          if (dockerForwards.isNotEmpty) heading('Docker'), // l10n-ignore: product name
          DockerForwardsPanel(service: docker, embedded: true),
        ],
      ],
    );
  }
}
