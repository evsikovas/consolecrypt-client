import 'dart:async';

import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_view.dart';
import 'package:consolecrypt/monitoring/agent_setup_controller.dart';
import 'package:consolecrypt/monitoring/agent_setup_models.dart';
import 'package:consolecrypt/monitoring/agent_setup_port.dart';
import 'package:material_ui/material_ui.dart';

class AgentSetupWizard extends StatefulWidget {
  const AgentSetupWizard({
    required this.port,
    required this.isCurrent,
    required this.onOpenGraphs,
    this.initialHostId,
    super.key,
  });
  final AgentSetupPort port;
  final bool Function() isCurrent;
  final String? initialHostId;
  final ValueChanged<AgentSetupVerification> onOpenGraphs;
  @override
  State<AgentSetupWizard> createState() => _AgentSetupWizardState();
}

class _AgentSetupWizardState extends State<AgentSetupWizard> {
  late AgentSetupController controller;
  final _scroll = ScrollController();
  Future<void> _cancelAndClose() async {
    if (controller.stopping) return;
    final cleanup = await controller.cancel();
    if (!mounted || !controller.current || !cleanup.closedObserved) return;
    await WidgetsBinding.instance.endOfFrame;
    if (mounted && controller.current) Navigator.of(context).pop();
  }

  @override
  void initState() {
    super.initState();
    _create();
  }

  void _create() {
    controller = AgentSetupController(
      port: widget.port,
      isCurrent: () => mounted && widget.isCurrent(),
      initialHostId: widget.initialHostId,
    );
    unawaited(controller.load());
  }

  @override
  void didUpdateWidget(covariant AgentSetupWizard oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!identical(widget.port, oldWidget.port) || widget.initialHostId != oldWidget.initialHostId) {
      controller.dispose();
      _create();
    }
  }

  @override
  void dispose() {
    controller.dispose();
    _scroll.dispose();
    super.dispose();
  }

  String _issue(AppLocalizations l, AgentSetupIssue issue) => switch (issue) {
    AgentSetupIssue.releaseUnavailable => l.agentSetupReleaseUnavailable,
    AgentSetupIssue.unsupportedHost => l.agentSetupUnsupportedHost,
    AgentSetupIssue.insufficientPrivileges => l.agentSetupPrivileges,
    AgentSetupIssue.policyConflict => l.agentSetupPolicyConflict,
    AgentSetupIssue.destinationUnavailable => l.agentSetupDestinationUnavailable,
    AgentSetupIssue.ownerApprovalRequired => l.agentSetupOwnerApproval,
    AgentSetupIssue.collectionUnavailable => l.agentSetupCollectionUnavailable,
    AgentSetupIssue.readinessNotObserved => l.agentSetupAwaitingEvidence,
    AgentSetupIssue.cleanupUnknown => l.agentSetupCleanupUnknown,
    AgentSetupIssue.contextChanged => l.agentSetupContextChanged,
    AgentSetupIssue.cancelled => l.agentSetupCancelled,
    AgentSetupIssue.unavailable => l.agentSetupFailed,
  };
  String _stage(AppLocalizations l) => switch (controller.step) {
    AgentSetupStep.selection => l.agentSetupSelectionTitle,
    AgentSetupStep.review => l.agentSetupReviewTitle,
    AgentSetupStep.installing => l.agentSetupInstalling,
    AgentSetupStep.offerReview || AgentSetupStep.certificateReview => l.agentSetupPairingTitle,
    AgentSetupStep.collectionReview || AgentSetupStep.configuring => l.agentSetupCollectionTitle,
    AgentSetupStep.verifying => l.agentSetupVerifying,
    AgentSetupStep.awaitingEvidence => l.agentSetupStatusTitle,
    AgentSetupStep.complete => l.agentSetupComplete,
    AgentSetupStep.failed => l.agentSetupStatusTitle,
  };
  String _profileName(AppLocalizations l, AgentSetupProfile profile) =>
      profile.id == 'linux_basic' ? l.agentSetupLinuxProfile : profile.name;
  String _profileDescription(AppLocalizations l, AgentSetupProfile profile) =>
      profile.id == 'linux_basic' ? l.agentSetupLinuxProfileDescription : profile.description;
  Widget _notice(String message, {Key? key}) => Card(
    key: key,
    child: Padding(padding: const EdgeInsets.all(12), child: Text(message)),
  );

  Widget _selection(AppLocalizations l) {
    final choices = controller.context;
    if (choices == null) return const SizedBox.shrink();
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(l.agentSetupPurpose),
        const SizedBox(height: 12),
        if (choices.hosts.isEmpty)
          Text(l.monitoringSetupNoHosts)
        else
          DropdownButtonFormField<AgentSetupHost>(
            key: ValueKey('agent-setup-host-${controller.host?.id}'),
            initialValue: controller.host,
            isExpanded: true,
            decoration: InputDecoration(labelText: l.monitoringSetupHost),
            items: [
              for (final host in choices.hosts)
                DropdownMenuItem(
                  value: host,
                  child: Text('${host.name} · ${host.address}', overflow: TextOverflow.ellipsis),
                ),
            ],
            onChanged: controller.busy ? null : (value) => controller.select(host: value),
          ),
        const SizedBox(height: 12),
        DropdownButtonFormField<AgentSetupProfile>(
          key: ValueKey('agent-setup-profile-${controller.profile?.id}'),
          initialValue: controller.profile,
          isExpanded: true,
          decoration: InputDecoration(labelText: l.agentSetupProfile),
          items: [
            for (final profile in choices.profiles)
              DropdownMenuItem(
                value: profile,
                enabled: profile.available,
                child: Text(_profileName(l, profile), overflow: TextOverflow.ellipsis),
              ),
          ],
          onChanged: controller.busy ? null : (value) => controller.select(profile: value),
        ),
        if (controller.profile case final profile?)
          Padding(
            padding: const EdgeInsets.only(top: 6),
            child: Text(_profileDescription(l, profile), style: Theme.of(context).textTheme.bodySmall),
          ),
        if (controller.profile?.requiresCollection == true) ...[
          const SizedBox(height: 12),
          if (choices.destinations.where((d) => d.available).isEmpty)
            _notice(l.agentSetupDestinationUnavailable)
          else
            DropdownButtonFormField<AgentSetupDestinationRef>(
              key: ValueKey('agent-setup-destination-${controller.destination?.id}'),
              initialValue: controller.destination,
              isExpanded: true,
              decoration: InputDecoration(labelText: l.agentSetupDestination),
              items: [
                for (final destination in choices.destinations)
                  DropdownMenuItem(
                    value: destination,
                    enabled: destination.available,
                    child: Text(destination.name, overflow: TextOverflow.ellipsis),
                  ),
              ],
              onChanged: controller.busy ? null : (value) => controller.select(destination: value),
            ),
          const SizedBox(height: 8),
          Text(l.agentSetupExistingExporters),
        ],
      ],
    );
  }

  String _action(AppLocalizations l, AgentSetupPlannedAction action) => switch (action) {
    AgentSetupPlannedAction.installAgent => l.agentSetupInstallAction,
    AgentSetupPlannedAction.createServiceAccount => l.agentSetupServiceAccount,
    AgentSetupPlannedAction.pairDevice => l.agentSetupPairingTitle,
    AgentSetupPlannedAction.configureCollection => l.agentSetupCollectionTitle,
    AgentSetupPlannedAction.reuseExporter => l.monitoringSetupReuse,
    AgentSetupPlannedAction.installExporter => l.agentSetupInstallExporter,
    AgentSetupPlannedAction.useExistingDestination => l.agentSetupUseDestination,
  };
  Widget _review(AppLocalizations l, AgentSetupPlan plan) => Column(
    crossAxisAlignment: CrossAxisAlignment.stretch,
    children: [
      Text(plan.selection.host.name, style: Theme.of(context).textTheme.titleMedium),
      Text(_profileName(l, plan.selection.profile)),
      if (plan.selection.destination case final destination?) Text('${l.agentSetupDestination}: ${destination.name}'),
      if (plan.release case final release?) Text('${l.agentSetupRelease}: ${release.version}'),
      const SizedBox(height: 8),
      if (plan.blocker case final issue?) _notice(_issue(l, issue), key: const ValueKey('agent-setup-blocker')),
      for (final action in plan.actions)
        Padding(
          padding: const EdgeInsets.symmetric(vertical: 4),
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              const Icon(Icons.checklist, size: 18),
              const SizedBox(width: 8),
              Expanded(child: Text(_action(l, action))),
            ],
          ),
        ),
    ],
  );

  Widget _facts(AppLocalizations l) {
    final snapshot = controller.verification;
    final paired = controller.paired != null || snapshot?.pairingAcknowledged == true;
    final software = snapshot?.software ?? controller.installed;
    final collection = snapshot?.collectorObservedReady == true
        ? l.agentSetupObservedReady
        : controller.collectionReview?.phase == AgentCollectorServicePhase.configurationRecorded
        ? l.agentSetupConfigurationRecorded
        : l.monitoringAgentStatusUnknown;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        ListTile(
          dense: true,
          title: Text(l.monitoringAgentSoftwareStatus),
          subtitle: Text(software?.installed == true ? l.agentSetupSoftwareObserved : l.monitoringAgentStatusUnknown),
        ),
        ListTile(
          dense: true,
          title: Text(l.monitoringAgentConnectionStatus),
          subtitle: Text(paired ? l.agentSetupPairingObserved : l.monitoringAgentStatusUnknown),
        ),
        ListTile(
          dense: true,
          title: Text(l.monitoringAgentMetricsStatus),
          subtitle: Text(controller.profile?.requiresCollection == true ? collection : l.agentSetupNotSelected),
        ),
        if (snapshot?.heartbeat case final heartbeat?) Text('${l.agentSetupRunningVersion}: ${heartbeat.version}'),
      ],
    );
  }

  Widget _pairing(AppLocalizations l) => Column(
    crossAxisAlignment: CrossAxisAlignment.stretch,
    children: [
      Text(l.agentSetupPairingExplanation),
      const SizedBox(height: 8),
      Text(controller.plan!.selection.host.name),
      Text(switch (controller.pairingReview!.purpose) {
        AgentAssociationPurpose.metrics => l.agentSetupMetricsPermission,
        AgentAssociationPurpose.tunnel => l.agentSetupTunnelPermission,
        AgentAssociationPurpose.exporters => l.agentSetupExportersPermission,
      }),
    ],
  );

  @override
  Widget build(BuildContext context) {
    if (!controller.current) return const SizedBox.shrink();
    final l = context.l10n;
    return AnimatedBuilder(
      animation: controller,
      builder: (context, _) {
        if (!controller.current) return const SizedBox.shrink();
        final step = controller.step, plan = controller.plan;
        return PopScope(
          canPop: !controller.busy,
          child: AlertDialog(
            key: const ValueKey('agent-setup-wizard'),
            title: Text(l.agentSetupTitle),
            content: SizedBox(
              width: 680,
              height: (MediaQuery.sizeOf(context).height - 240).clamp(180.0, 400.0),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Text(
                    controller.stopping ? l.agentSetupStopping : _stage(l),
                    style: Theme.of(context).textTheme.titleSmall,
                  ),
                  const SizedBox(height: 8),
                  if (controller.busy) const LinearProgressIndicator(),
                  Expanded(
                    child: Scrollbar(
                      controller: _scroll,
                      thumbVisibility: true,
                      child: SingleChildScrollView(
                        controller: _scroll,
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.stretch,
                          children: [
                            if (controller.issue case final issue?)
                              _notice(_issue(l, issue), key: const ValueKey('agent-setup-error')),
                            if (step == AgentSetupStep.selection)
                              _selection(l)
                            else if (step == AgentSetupStep.review && plan != null)
                              _review(l, plan)
                            else ...[
                              if (step == AgentSetupStep.offerReview || step == AgentSetupStep.certificateReview)
                                _pairing(l),
                              if (step == AgentSetupStep.collectionReview) _notice(l.agentSetupCollectionExplanation),
                              _facts(l),
                              if (step == AgentSetupStep.awaitingEvidence) _notice(l.agentSetupAwaitingEvidence),
                              if (step == AgentSetupStep.complete) _notice(l.agentSetupVerified),
                            ],
                          ],
                        ),
                      ),
                    ),
                  ),
                ],
              ),
            ),
            actions: [
              if (step == AgentSetupStep.review)
                TextButton(
                  key: const ValueKey('agent-setup-back'),
                  onPressed: controller.busy ? null : () => unawaited(controller.back()),
                  child: Text(l.commonBack),
                ),
              TextButton(
                key: const ValueKey('agent-setup-close'),
                onPressed: controller.stopping ? null : () => unawaited(_cancelAndClose()),
                child: Text(
                  controller.stopping
                      ? l.agentSetupStopping
                      : controller.busy
                      ? l.commonCancel
                      : l.commonClose,
                ),
              ),
              if (step == AgentSetupStep.selection)
                FilledButton(
                  key: const ValueKey('agent-setup-prepare'),
                  onPressed: controller.canPrepare ? () => unawaited(controller.prepare()) : null,
                  child: Text(l.agentSetupPreflight),
                ),
              if (step == AgentSetupStep.review)
                FilledButton(
                  key: const ValueKey('agent-setup-install'),
                  onPressed: controller.canInstall ? () => unawaited(controller.install()) : null,
                  child: Text(plan?.requiresInstall == true ? l.agentSetupInstallAction : l.commonContinue),
                ),
              if (step == AgentSetupStep.offerReview)
                FilledButton(
                  key: const ValueKey('agent-setup-connect'),
                  onPressed: controller.busy ? null : () => unawaited(controller.connect()),
                  child: Text(l.commonConnect),
                ),
              if (step == AgentSetupStep.certificateReview)
                FilledButton(
                  key: const ValueKey('agent-setup-confirm-connection'),
                  onPressed: controller.busy ? null : () => unawaited(controller.confirmConnection()),
                  child: Text(l.agentSetupConfirmConnection),
                ),
              if (step == AgentSetupStep.collectionReview)
                FilledButton(
                  key: const ValueKey('agent-setup-enable-collection'),
                  onPressed: controller.busy ? null : () => unawaited(controller.enableCollection()),
                  child: Text(l.agentSetupEnableCollection),
                ),
              if (step == AgentSetupStep.awaitingEvidence)
                FilledButton(
                  key: const ValueKey('agent-setup-verify'),
                  onPressed: controller.busy ? null : () => unawaited(controller.verify()),
                  child: Text(l.agentSetupVerify),
                ),
              if (controller.canOpenGraphs)
                FilledButton(
                  key: const ValueKey('agent-setup-open-graphs'),
                  onPressed: () {
                    if (controller.current) widget.onOpenGraphs(controller.verification!);
                  },
                  child: Text(l.monitoringSetupOpenGraphs),
                ),
            ],
          ),
        );
      },
    );
  }
}
