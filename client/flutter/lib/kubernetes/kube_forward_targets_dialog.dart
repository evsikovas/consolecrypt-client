import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_forward_panel.dart';
import 'package:consolecrypt/kubernetes/kube_forward_service.dart';
import 'package:consolecrypt/kubernetes/kube_forward_targets_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

Future<void> showKubeResourceForwardDialog(
  BuildContext context, {
  required KubeSession session,
  required KubeResourceRef resource,
  required KubeBrowserService service,
  required KubeResourceDetail detail,
}) async {
  final container = ProviderScope.containerOf(context, listen: false),
      scope = ProviderScope.containerOf(context, listen: false).read(kubeScopeProvider);
  if (!scope.unlocked ||
      !session.canPortForward ||
      service is! KubeForwardTargetsService ||
      service is! KubeForwardService) {
    return;
  }
  final selected = await showAppDialog<KubeForwardTarget>(
    context,
    secure: true,
    builder: (_) => _TargetsDialog(session: session, resource: resource, service: service as KubeForwardTargetsService),
  );
  if (!context.mounted || selected == null || !identical(scope, container.read(kubeScopeProvider))) return;
  await showKubeForwardDialog(
    context,
    session: session,
    resource: selected.pod,
    service: service as KubeForwardService,
    detail: detail,
    ports: selected.ports.map((p) => p.port).toList(),
  );
}

class _TargetsDialog extends ConsumerStatefulWidget {
  const _TargetsDialog({required this.session, required this.resource, required this.service});
  final KubeSession session;
  final KubeResourceRef resource;
  final KubeForwardTargetsService service;
  @override
  ConsumerState<_TargetsDialog> createState() => _TargetsDialogState();
}

class _TargetsDialogState extends ConsumerState<_TargetsDialog> {
  late final KubeScope _scope = ref.read(kubeScopeProvider);
  KubeForwardTargets? _targets;
  bool _failed = false;
  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(kubeScopeProvider));
  @override
  void initState() {
    super.initState();
    unawaited(_load());
  }

  Future<void> _load() async {
    try {
      final targets = await widget.service.forwardTargets(widget.session.id, widget.resource);
      if (_current) setState(() => _targets = targets);
    } catch (_) {
      if (_current) setState(() => _failed = true);
    }
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(kubeScopeProvider, (_, next) {
      if (identical(_scope, next)) return;
      _targets = null;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted) return;
        final route = ModalRoute.of(context);
        if (route?.isActive == true) Navigator.of(context).removeRoute(route!);
      });
    });
    final l = context.l10n, targets = _targets;
    return GlassDialog(
      title: l.kubeForwardChoosePod,
      content: SizedBox(
        width: 650,
        height: 400,
        child: !_current
            ? const SizedBox.shrink()
            : Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Text(l.kubeForwardChoosePodHelp),
                  if (_failed) Text(l.kubeRequestFailed),
                  if (targets == null && !_failed) const LinearProgressIndicator(),
                  if (targets?.partial == true) Text(l.kubeInsightsPartial),
                  if (targets != null)
                    Expanded(
                      child: targets.items.isEmpty
                          ? Text(l.kubeNoResources)
                          : ListView.builder(
                              itemCount: targets.items.length,
                              itemBuilder: (_, index) {
                                final target = targets.items[index];
                                return ListTile(
                                  key: ValueKey('kube-forward-target-${target.pod.name}'),
                                  title: Text(target.pod.name),
                                  subtitle: Text(
                                    // l10n-ignore: Pod namespace and declared numeric port identifiers.
                                    /* l10n-ignore: Pod namespace and declared numeric port identifiers. */ '${target.pod.namespace} · ${target.ports.map((p) => '${p.name ?? 'TCP'}:${p.port}').join(', ')}',
                                  ),
                                  leading: Icon(
                                    target.ready ? Icons.check_circle_outline : Icons.warning_amber_outlined,
                                  ),
                                  onTap: () => closeDialog(context, target),
                                );
                              },
                            ),
                    ),
                ],
              ),
      ),
      primaryAction: GlassButton(label: l.commonCancel, onPressed: () => closeDialog<KubeForwardTarget>(context)),
    );
  }
}
