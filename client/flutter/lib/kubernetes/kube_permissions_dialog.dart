import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_operations_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class KubePermissionsDialog extends ConsumerStatefulWidget {
  const KubePermissionsDialog({required this.sessionId, required this.namespace, required this.service, super.key});
  final String sessionId, namespace;
  final KubeOperationsService service;
  @override
  ConsumerState<KubePermissionsDialog> createState() => _KubePermissionsDialogState();
}

class _KubePermissionsDialogState extends ConsumerState<KubePermissionsDialog> {
  late final KubeScope _scope = ref.read(kubeScopeProvider);
  KubePermissionReport? _report;
  bool _failed = false;
  String _filter = '';
  @override
  void initState() {
    super.initState();
    unawaited(_load());
  }

  Future<void> _load() async {
    try {
      final report = await widget.service.permissions(widget.sessionId, widget.namespace);
      if (mounted && identical(_scope, ref.read(kubeScopeProvider))) setState(() => _report = report);
    } catch (_) {
      if (mounted && identical(_scope, ref.read(kubeScopeProvider))) setState(() => _failed = true);
    }
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(kubeScopeProvider, (_, next) {
      if (identical(_scope, next)) return;
      _report = null;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted) return;
        final route = ModalRoute.of(context);
        if (route?.isActive == true) Navigator.of(context).removeRoute(route!);
      });
    });
    final l = context.l10n;
    final report = _report;
    final rules =
        report?.rules
            .where(
              (rule) =>
                  '${rule.apiGroups.join(' ')} ${rule.resources.join(' ')} ${rule.verbs.join(' ')} ${rule.resourceNames.join(' ')}'
                      .toLowerCase()
                      .contains(_filter.toLowerCase()),
            )
            .toList() ??
        [];
    return GlassDialog(
      title: '${l.kubePermissions} · ${widget.namespace}',
      width: 800,
      content: SizedBox(
        height: 460,
        child: !_scope.unlocked || !identical(_scope, ref.watch(kubeScopeProvider))
            ? const SizedBox.shrink()
            : Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Text(l.kubePermissionsHelp),
                  if (report?.incomplete == true) Text(l.kubePermissionsIncomplete),
                  if (report == null && !_failed) const LinearProgressIndicator(),
                  if (_failed) Text(l.kubeRequestFailed),
                  TextField(
                    key: const ValueKey('kube-permissions-search'),
                    decoration: InputDecoration(labelText: l.commonSearch),
                    onChanged: (value) => setState(() => _filter = value),
                  ),
                  Expanded(
                    child: ListView.builder(
                      itemCount: rules.length,
                      itemBuilder: (_, i) {
                        final rule = rules[i];
                        return ListTile(
                          title: Text(
                            // l10n-ignore: Kubernetes RBAC API groups, resources, verbs and resource names.
                            /* l10n-ignore: Kubernetes RBAC API groups, resources, verbs and resource names. */ '${rule.apiGroups.map((group) => group.isEmpty ? 'core' : group).join(', ')} · ${rule.resources.join(', ')}',
                          ),
                          subtitle: Text(
                            // l10n-ignore: Kubernetes RBAC API groups, resources, verbs and resource names.
                            /* l10n-ignore: Kubernetes RBAC API groups, resources, verbs and resource names. */ '${rule.verbs.join(', ')}${rule.resourceNames.isEmpty ? '' : '\n${rule.resourceNames.join(', ')}'}',
                          ),
                        );
                      },
                    ),
                  ),
                ],
              ),
      ),
      primaryAction: GlassButton(label: l.commonClose, onPressed: () => closeDialog<void>(context)),
    );
  }
}
