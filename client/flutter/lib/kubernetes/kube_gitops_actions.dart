import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_gitops_monitor.dart';
import 'package:consolecrypt/kubernetes/kube_insights_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_write_access.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

Future<bool> runKubeGitOpsAction(
  BuildContext context,
  WidgetRef ref, {
  required KubeSession session,
  required KubeInsightResource resource,
  required KubeInsightsService service,
  required String action,
  KubeGitOpsMonitor? monitor,
}) async {
  final container = ProviderScope.containerOf(context, listen: false);
  final scope = ref.read(kubeScopeProvider);
  bool current() => context.mounted && scope.unlocked && identical(scope, container.read(kubeScopeProvider));
  bool canWrite() => current() && (container.read(kubeWriteAccessProvider(session)).value?.canWrite ?? false);
  if (!canWrite() || !kubeGitOpsActions(resource).contains(action)) return false;
  final approval = await showAppDialog<_GitOpsApproval>(
    context,
    secure: true,
    barrierDismissible: false,
    builder: (_) => _GitOpsConfirmation(
      resource: resource.resource.name,
      title: kubeGitOpsActionLabel(context.l10n, action),
      effect: resource.isFlux ? kubeGitOpsActionHelp(context.l10n, action) : null,
      reasonRequired: action == 'suspend' || action == 'resume',
      allowReminder: (action == 'suspend' || action == 'resume') && monitor?.preferences != null,
      help: action == 'reconcile_with_source' ? context.l10n.kubeGitOpsReconcileWithSourceHelp : null,
    ),
  );
  if (approval == null || !canWrite()) return false;
  if (resource.isArgo) {
    await service.argoAction(session.id, resource.reference, switch (action) {
      'sync' => KubeArgoAction.sync,
      'hard_refresh' => KubeArgoAction.hardRefresh,
      _ => KubeArgoAction.refresh,
    });
  } else {
    await service.fluxAction(session.id, resource.reference, switch (action) {
      'reconcile_with_source' => KubeFluxAction.reconcileWithSource,
      'suspend' => KubeFluxAction.suspend,
      'resume' => KubeFluxAction.resume,
      'reset' => KubeFluxAction.resetFailures,
      'force' => KubeFluxAction.forceUpgrade,
      _ => KubeFluxAction.reconcile,
    }, reason: approval.reason.isEmpty ? null : approval.reason);
  }
  if (current() && approval.reminder > Duration.zero) {
    await monitor?.remind(resource.reference, approval.reminder);
  } else if (current() && action == 'resume') {
    await monitor?.cancelReminder(resource.reference.id);
  }
  return current();
}

Future<void> runKubeGitOpsBulkAction(
  BuildContext context,
  WidgetRef ref, {
  required KubeSession session,
  required String contextName,
  required List<KubeInsightResource> resources,
  required KubeInsightsService service,
  required String action,
  required bool Function() shouldContinue,
  required ValueChanged<KubeInsightResource> onCompleted,
}) async {
  final container = ProviderScope.containerOf(context, listen: false);
  final scope = ref.read(kubeScopeProvider);
  bool canWrite() =>
      context.mounted &&
      scope.unlocked &&
      identical(scope, container.read(kubeScopeProvider)) &&
      !session.production &&
      (container.read(kubeWriteAccessProvider(session)).value?.canWrite ?? false);
  if (!canWrite() ||
      resources.isEmpty ||
      resources.length > 100 ||
      !const ['reconcile', 'suspend', 'resume'].contains(action) ||
      resources.any((r) => !r.isFlux || !kubeGitOpsActions(r).contains(action))) {
    return;
  }
  final approval = await showAppDialog<_GitOpsApproval>(
    context,
    secure: true,
    barrierDismissible: false,
    builder: (_) => _GitOpsConfirmation(
      resource: contextName,
      title: kubeGitOpsActionLabel(context.l10n, action),
      effect: kubeGitOpsActionHelp(context.l10n, action),
      reasonRequired: action == 'suspend' || action == 'resume',
      help: context.l10n.kubeBulkHelp,
      resources: resources.map((r) => '${r.api.kind} · ${r.resource.namespace ?? ''}/${r.resource.name}').toList(),
    ),
  );
  if (approval == null) return;
  for (final resource in resources) {
    if (!canWrite() || !shouldContinue()) return;
    await service.fluxAction(session.id, resource.reference, switch (action) {
      'suspend' => KubeFluxAction.suspend,
      'resume' => KubeFluxAction.resume,
      _ => KubeFluxAction.reconcile,
    }, reason: approval.reason.isEmpty ? null : approval.reason);
    if (!canWrite()) return;
    onCompleted(resource);
  }
}

List<String> kubeGitOpsActions(KubeInsightResource resource) {
  if (resource.isArgo) return ['refresh', 'hard_refresh', 'sync'];
  if (!resource.isFlux || resource.api.group == 'notification.toolkit.fluxcd.io') return [];
  return [
    'reconcile',
    if (const ['Kustomization', 'HelmRelease', 'HelmChart'].contains(resource.api.kind)) 'reconcile_with_source',
    if (resource.api.kind != 'ImagePolicy') resource.suspended ? 'resume' : 'suspend',
    if (resource.api.kind == 'HelmRelease') ...['reset', 'force'],
  ];
}

String kubeGitOpsActionLabel(AppLocalizations l, String action) => switch (action) {
  'reconcile_with_source' => l.kubeGitOpsReconcileWithSource,
  'suspend' => l.kubeGitOpsSuspend,
  'resume' => l.kubeGitOpsResume,
  'reset' => l.kubeGitOpsReset,
  'force' => l.kubeGitOpsForce,
  'sync' => l.kubeArgoSync,
  'refresh' => l.kubeArgoRefresh,
  'hard_refresh' => l.kubeArgoHardRefresh,
  _ => l.kubeGitOpsReconcile,
};

/// What a Flux action does, in plain words with the matching flux command
/// (tooltips and the confirmation dialog). Argo actions are self-explanatory.
String? kubeGitOpsActionHelp(AppLocalizations l, String action) => switch (action) {
  'reconcile' => l.kubeGitOpsReconcileTooltip,
  'reconcile_with_source' => l.kubeGitOpsReconcileWithSourceTooltip,
  'suspend' => l.kubeGitOpsSuspendTooltip,
  'resume' => l.kubeGitOpsResumeTooltip,
  'reset' => l.kubeGitOpsResetTooltip,
  'force' => l.kubeGitOpsForceTooltip,
  _ => null,
};

class _GitOpsConfirmation extends ConsumerStatefulWidget {
  const _GitOpsConfirmation({
    required this.resource,
    required this.title,
    required this.reasonRequired,
    this.effect,
    this.help,
    this.allowReminder = false,
    this.resources = const [],
  });
  final String resource, title;
  final String? help;

  /// What the controller will do (Flux actions).
  final String? effect;
  final bool reasonRequired;
  final bool allowReminder;
  final List<String> resources;
  @override
  ConsumerState<_GitOpsConfirmation> createState() => _GitOpsConfirmationState();
}

class _GitOpsConfirmationState extends ConsumerState<_GitOpsConfirmation> {
  final _name = TextEditingController(), _reason = TextEditingController();
  late final KubeScope _scope;
  Duration _reminder = Duration.zero;
  @override
  void initState() {
    super.initState();
    _scope = ref.read(kubeScopeProvider);
  }

  @override
  void dispose() {
    _name.clear();
    _reason.clear();
    _name.dispose();
    _reason.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(kubeScopeProvider, (_, next) {
      if (identical(next, _scope)) return;
      _name.clear();
      _reason.clear();
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted) return;
        final route = ModalRoute.of(context);
        if (route?.isActive == true) Navigator.of(context).removeRoute(route!);
      });
    });
    final l = context.l10n;
    return GlassDialog(
      key: const ValueKey('kube-gitops-confirm'),
      title: widget.title,
      content: SingleChildScrollView(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            if (widget.effect case final effect?) ...[
              Text(effect, key: const ValueKey('kube-gitops-confirm-effect')),
              const SizedBox(height: GlassSpacing.s8),
            ],
            Text(widget.help ?? l.kubeGitOpsChangeHelp),
            if (widget.resources.isNotEmpty)
              ConstrainedBox(
                constraints: const BoxConstraints(maxHeight: 200),
                child: ListView(shrinkWrap: true, children: widget.resources.map((name) => Text(name)).toList()),
              ),
            const SizedBox(height: 12),
            SelectableText(widget.resource),
            TextField(
              key: const ValueKey('kube-gitops-confirm-name'),
              controller: _name,
              decoration: InputDecoration(labelText: l.kubeTypeName),
              autocorrect: false,
              enableSuggestions: false,
              onChanged: (_) => setState(() {}),
            ),
            if (widget.reasonRequired)
              TextField(
                key: const ValueKey('kube-gitops-reason'),
                controller: _reason,
                decoration: InputDecoration(labelText: l.kubeGitOpsReason),
                maxLength: 1024,
                maxLines: 2,
                onChanged: (_) => setState(() {}),
              ),
            if (widget.allowReminder)
              GlassDropdownField<Duration>(
                key: const ValueKey('kube-gitops-reminder'),
                value: _reminder,
                label: l.kubeGitOpsRemind,
                items: [
                  GlassSelectItem(value: Duration.zero, label: l.kubeGitOpsNoReminder),
                  for (final hours in [1, 2, 24])
                    GlassSelectItem(
                      value: Duration(hours: hours),
                      label: l.kubeGitOpsRemindHours(hours),
                    ),
                ],
                onChanged: (value) => setState(() => _reminder = value),
              ),
          ],
        ),
      ),
      primaryAction: GlassButton.destructive(
        key: const ValueKey('kube-gitops-confirm-action'),
        label: widget.title,
        onPressed:
            _name.text == widget.resource &&
                widget.resource.isNotEmpty &&
                (!widget.reasonRequired ||
                    (_reason.text.trim().isNotEmpty && !RegExp(r'[\x00-\x1f\x7f]').hasMatch(_reason.text)))
            ? () => closeDialog(context, _GitOpsApproval(_reason.text.trim(), _reminder))
            : null,
      ),
      secondaryActions: [GlassButton(label: l.commonCancel, onPressed: () => closeDialog<_GitOpsApproval>(context))],
    );
  }
}

final class _GitOpsApproval {
  const _GitOpsApproval(this.reason, this.reminder);
  final String reason;
  final Duration reminder;
}
