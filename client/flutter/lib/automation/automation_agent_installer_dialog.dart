import 'package:consolecrypt/app/critical_notice_providers.dart';
import 'package:consolecrypt/automation/automation_agent_installer.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/monitoring/agent_heartbeat_observation.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Builds source only. Native owns input snapshots and catalog publication;
/// normal Run owns Runner/target verification and the separate signed review.
class AutomationAgentInstallerDialog extends ConsumerStatefulWidget {
  const AutomationAgentInstallerDialog({
    super.key,
    required this.scope,
    required this.service,
    required this.originalCurrent,
    required this.originalLive,
    this.fixedHostId,
    this.observedHeartbeat,
    this.onOpenCreated,
    this.onCadvisorAnsible,
  });
  final String? fixedHostId;
  final AgentHeartbeatObservation? observedHeartbeat;
  final ValueChanged<AgentInstallerCreated>? onOpenCreated;
  final void Function(AgentInstallerContext context, AgentInstallerHost host)? onCadvisorAnsible;
  final AutomationScope scope;
  final AutomationAgentInstallerService service;
  final bool Function() originalCurrent;
  final ValueListenable<bool> originalLive;
  @override
  ConsumerState<AutomationAgentInstallerDialog> createState() => _State();
}

class _State extends ConsumerState<AutomationAgentInstallerDialog> {
  final name = TextEditingController();
  AgentInstallerContext? metadata;
  AgentInstallerHost? host;
  AgentInstallerOperation operation = AgentInstallerOperation.install;
  AgentInstallerCreated? created;
  String? artifact, manifest;
  bool loading = true, busy = false, start = false, spent = false, retired = false;
  Object? error;
  bool fixedHostUnavailable = false;
  int generation = 0;
  bool get current =>
      mounted &&
      !retired &&
      widget.originalLive.value &&
      widget.originalCurrent() &&
      identical(ref.read(automationScopeProvider), widget.scope);
  bool get release => operation == AgentInstallerOperation.install || operation == AgentInstallerOperation.update;
  bool get canCreate =>
      current &&
      !loading &&
      !busy &&
      !spent &&
      metadata != null &&
      host != null &&
      name.text.trim().isNotEmpty &&
      (!release || artifact != null && manifest != null);
  @override
  void initState() {
    super.initState();
    widget.originalLive.addListener(retire);
    load();
  }

  void retire() {
    if (!widget.originalLive.value) {
      retired = true;
      generation++;
    }
  }

  @override
  void didUpdateWidget(covariant AutomationAgentInstallerDialog old) {
    super.didUpdateWidget(old);
    if (old.fixedHostId != widget.fixedHostId ||
        !identical(old.scope, widget.scope) ||
        !identical(old.service, widget.service)) {
      retired = true;
      generation++;
    }
  }

  @override
  void dispose() {
    generation++;
    widget.originalLive.removeListener(retire);
    name.dispose();
    super.dispose();
  }

  Future<void> load() async {
    final token = ++generation;
    try {
      if (!current) return;
      final runtime = ref.read(criticalRuntimeEventsProvider), scope = runtime?.currentScope;
      final frame = scope?.scope;
      if (frame == null ||
          scope?.streamUnavailable != false ||
          scope?.unsupported != false ||
          frame.profileId != widget.scope.profileId?.toString()) {
        throw const FormatException('agent_installer_scope');
      }
      final expected = AgentInstallerFrame(profileId: frame.profileId, notificationScopeId: frame.nonce);
      final raw = await widget.service.agentInstallerContext(expected.toJson());
      if (!current ||
          token != generation ||
          !identical(ref.read(criticalRuntimeEventsProvider), runtime) ||
          runtime?.currentScope.epoch != scope?.epoch ||
          runtime?.currentScope.scope != frame) {
        return;
      }
      final value = AgentInstallerContext.parse(raw, expected);
      final fixed = widget.fixedHostId;
      final selected = fixed == null ? null : value.hosts.where((h) => h.id == fixed).firstOrNull;
      if (fixed != null && selected == null) {
        setState(() {
          loading = false;
          fixedHostUnavailable = true;
        });
        return;
      }

      setState(() {
        metadata = value;
        host = selected;
        loading = false;
      });
    } catch (e) {
      if (current && token == generation) {
        setState(() {
          error = e;
          loading = false;
        });
      }
    }
  }

  Future<void> pick(bool package) async {
    if (!current || busy || spent) return;
    final token = ++generation;
    final path = await ref.read(fileDialogServiceProvider).chooseOpenFile(extensions: [package ? 'zip' : 'json']);
    if (!current || token != generation || path == null) return;
    setState(() {
      if (package) {
        artifact = path;
      } else {
        manifest = path;
      }
    });
  }

  Future<void> create() async {
    if (!canCreate) return;
    final original = metadata!, selected = host!;
    final token = ++generation;
    final request = original.request(
      name: name.text.trim(),
      host: selected,
      operation: operation,
      start: start,
      artifactPath: release ? artifact : null,
      manifestPath: release ? manifest : null,
    );
    setState(() {
      busy = true;
      spent = true;
      error = null;
    });
    try {
      final raw = await widget.service.createAgentInstallerProject(request);
      if (!current || token != generation) return;
      final result = AgentInstallerCreated.parse(raw, original, request);
      setState(() => created = result);
    } catch (e) {
      if (current && token == generation) setState(() => error = e);
    } finally {
      if (current && token == generation) setState(() => busy = false);
    }
  }

  String label(AgentInstallerOperation value) => switch (value) {
    AgentInstallerOperation.install => context.l10n.agentInstallOperationInstall,
    AgentInstallerOperation.update => context.l10n.agentInstallOperationUpdate,
    AgentInstallerOperation.rollback => context.l10n.agentInstallOperationRollback,
    AgentInstallerOperation.remove => context.l10n.agentInstallOperationRemove,
    AgentInstallerOperation.status => context.l10n.agentInstallOperationStatus,
  };
  String leaf(String path) => path.replaceAll('\\', '/').split('/').last;
  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final result = created;
    return AlertDialog(
      key: const ValueKey('agent-installer-dialog'),
      title: Text(l.agentInstallTitle),
      content: SizedBox(
        width: 680,
        child: SingleChildScrollView(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              InfoBanner(tone: BannerTone.warning, message: l.agentInstallSourceOnly),
              const SizedBox(height: 12), Text(l.agentInstallTrustGuide),
              // l10n-ignore: fixed target bootstrap paths, never UI-supplied keys.
              const SelectableText( // l10n-ignore: fixed filesystem paths and executable names.
                // l10n-ignore: exact technical path, CLI syntax or SHA value; never translated.
                '/etc/consolecrypt-agent-release/trust.json\n/etc/consolecrypt-agent-release/public-key.json\n/usr/local/libexec/consolecrypt-agent-release/cc-agent-identity\nconsolecrypt-agent', // l10n-ignore: exact technical path, CLI syntax or SHA value; never translated.
              ),
              const SizedBox(height: 12), Text(l.agentInstallNoGrants),
              const SizedBox(height: 12),
              if (widget.fixedHostId != null)
                if (widget.observedHeartbeat case final observation?) ...[
                  Text(
                    l.hostAgentLastObservedVersion(
                      observation.dataset.agentVersion!,
                      observation.receivedAt.toIso8601String(),
                    ),
                    key: const ValueKey('host-agent-observed-version'),
                  ),
                  Text(l.hostAgentLastObservedHint),
                ] else
                  Text(l.hostAgentObservedUnknown),
              if (fixedHostUnavailable) InfoBanner(tone: BannerTone.warning, message: l.hostAgentTargetChanged),
              if (loading) const LinearProgressIndicator(),
              if (error != null) InfoBanner(tone: BannerTone.warning, message: errorMessage(l, error!)),
              if (error != null && spent) InfoBanner(tone: BannerTone.info, message: l.agentInstallOutcomeUnknown),
              if (result == null && metadata != null) ...[
                if (metadata!.hosts.isEmpty) InfoBanner(tone: BannerTone.info, message: l.agentInstallNoTargets),
                TextField(
                  key: const ValueKey('agent-installer-name'),
                  controller: name,
                  enabled: !busy && !spent,
                  maxLength: 120,
                  onChanged: (_) => setState(() {}),
                  decoration: InputDecoration(labelText: l.automationNewProject),
                ),
                if (widget.fixedHostId != null && host != null)
                  ListTile(
                    key: const ValueKey('agent-installer-fixed-host'),
                    contentPadding: EdgeInsets.zero,
                    title: Text(l.automationTargets),
                    subtitle: Text(host!.name),
                  ),
                if (widget.fixedHostId == null)
                  DropdownButtonFormField<AgentInstallerHost>(
                    key: const ValueKey('agent-installer-host'),
                    initialValue: host,
                    isExpanded: true,
                    decoration: InputDecoration(labelText: l.automationTargets),
                    items: [
                      for (final value in metadata!.hosts)
                        DropdownMenuItem(
                          value: value,
                          child: Text(value.name, overflow: TextOverflow.ellipsis),
                        ),
                    ],
                    onChanged: busy || spent ? null : (value) => setState(() => host = value),
                  ),
                const SizedBox(height: 12),
                DropdownButtonFormField<AgentInstallerOperation>(
                  key: const ValueKey('agent-installer-operation'),
                  initialValue: operation,
                  isExpanded: true,
                  decoration: InputDecoration(labelText: l.agentInstallOperation),
                  items: [
                    for (final value in AgentInstallerOperation.values)
                      DropdownMenuItem(
                        value: value,
                        child: Text(label(value), overflow: TextOverflow.ellipsis),
                      ),
                  ],
                  onChanged: busy || spent
                      ? null
                      : (value) {
                          if (value != null) {
                            setState(() {
                              operation = value;
                              start = false;
                              artifact = null;
                              manifest = null;
                            });
                          }
                        },
                ),
                if (release) ...[
                  const SizedBox(height: 12),
                  Text(l.agentInstallPublicFiles),
                  TextButton(
                    key: const ValueKey('agent-installer-artifact'),
                    onPressed: busy || spent ? null : () => pick(true),
                    child: Text(artifact == null ? l.agentInstallChooseArtifact : leaf(artifact!)),
                  ),
                  TextButton(
                    key: const ValueKey('agent-installer-manifest'),
                    onPressed: busy || spent ? null : () => pick(false),
                    child: Text(manifest == null ? l.agentInstallChooseManifest : leaf(manifest!)),
                  ),
                ],
                if (operation == AgentInstallerOperation.install ||
                    operation == AgentInstallerOperation.update ||
                    operation == AgentInstallerOperation.rollback)
                  CheckboxListTile(
                    key: const ValueKey('agent-installer-start'),
                    value: start,
                    onChanged: busy || spent ? null : (value) => setState(() => start = value == true),
                    title: Text(l.agentInstallStart),
                    subtitle: Text(l.agentInstallStartHint),
                    controlAffinity: ListTileControlAffinity.leading,
                  ),
                if (operation == AgentInstallerOperation.remove)
                  InfoBanner(tone: BannerTone.warning, message: l.agentInstallRemoveHint),
                Text(l.agentInstallPrivilegeReview),
              ],
              if (result != null) ...[
                InfoBanner(tone: BannerTone.info, message: l.agentInstallProjectCreated),
                const SizedBox(height: 12),
                Text(l.agentInstallUnverified),
                SelectableText( // l10n-ignore: exact digest and artifact filenames.
                  'SHA256: ${result.projectDigest}', // l10n-ignore: standardized digest label and value.
                ), // l10n-ignore: exact technical path, CLI syntax or SHA value; never translated.
                if (result.artifactSha256 != null)
                  SelectableText( // l10n-ignore: exact digest and artifact filenames.
                    // l10n-ignore: exact technical path, CLI syntax or SHA value; never translated.
                    'candidate.zip SHA256: ${result.artifactSha256}\ncandidate-release.json SHA256: ${result.manifestSha256}', // l10n-ignore: exact technical path, CLI syntax or SHA value; never translated.
                  ),
                const SizedBox(height: 12),
                Text(l.agentInstallRuntimeUnknown),
              ],
            ],
          ),
        ),
      ),
      actions: [
        if (widget.onCadvisorAnsible != null)
          TextButton(
            key: const ValueKey('host-agent-cadvisor-ansible'),
            onPressed: current && !busy && !spent && metadata != null && host != null && widget.fixedHostId == host!.id
                ? () => widget.onCadvisorAnsible!(metadata!, host!)
                : null,
            child: Text(l.hostAgentCadvisorAnsible),
          ),
        TextButton(
          key: const ValueKey('agent-installer-cancel'),
          onPressed: busy ? null : () => Navigator.pop(context),
          child: Text(l.commonClose),
        ),
        if (result == null)
          TextButton(
            key: const ValueKey('agent-installer-create'),
            onPressed: canCreate ? create : null,
            child: Text(l.agentInstallCreate),
          ),
        if (result != null)
          TextButton(
            key: const ValueKey('agent-installer-open'),
            onPressed: current
                ? () {
                    final open = widget.onOpenCreated;
                    if (open != null) {
                      open(result);
                    } else {
                      Navigator.pop(context, result.project);
                    }
                  }
                : null,
            child: Text(l.agentInstallOpen),
          ),
      ],
    );
  }
}
