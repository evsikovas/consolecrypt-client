import 'package:consolecrypt/app/critical_notice_providers.dart';
import 'package:consolecrypt/automation/automation_agent_installer.dart';
import 'package:consolecrypt/automation/automation_monitoring_node.dart';
import 'package:consolecrypt/automation/automation_monitoring_node_source.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// This creates owned project source only. Normal Signed Run separately owns
/// review, host-key checks, credentials and execution on the selected target.
class AutomationMonitoringNodeDialog extends ConsumerStatefulWidget {
  const AutomationMonitoringNodeDialog({
    super.key,
    required this.scope,
    required this.service,
    required this.contextService,
    required this.originalCurrent,
    required this.originalLive,
  });
  final AutomationScope scope;
  final AutomationMonitoringNodeService service;
  final AutomationAgentInstallerService contextService;
  final bool Function() originalCurrent;
  final ValueListenable<bool> originalLive;
  @override
  ConsumerState<AutomationMonitoringNodeDialog> createState() => _State();
}

class _State extends ConsumerState<AutomationMonitoringNodeDialog> {
  final name = TextEditingController(),
      cpu = TextEditingController(text: '2'),
      memory = TextEditingController(text: '2048'),
      disk = TextEditingController(text: '50'),
      days = TextEditingController(text: '15'),
      retention = TextEditingController(text: '40'),
      samples = TextEditingController(text: '2000');
  AgentInstallerContext? metadata;
  AgentInstallerHost? host;
  MonitoringNodeOperation operation = MonitoringNodeOperation.install;
  MonitoringNodeSourceCreated? created;
  bool receiver = false, alertmanager = false, loading = true, busy = false, spent = false, retired = false;
  Object? error;
  bool invalidPlan = false;
  int generation = 0;
  bool get current =>
      mounted &&
      !retired &&
      widget.originalLive.value &&
      widget.originalCurrent() &&
      identical(ref.read(automationScopeProvider), widget.scope);
  bool get canCreate =>
      current && !loading && !busy && !spent && metadata != null && host != null && name.text.trim().isNotEmpty;
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
  void didUpdateWidget(covariant AutomationMonitoringNodeDialog old) {
    super.didUpdateWidget(old);
    if (!identical(old.originalLive, widget.originalLive)) {
      old.originalLive.removeListener(retire);
      widget.originalLive.addListener(retire);
    }
    if (!identical(old.scope, widget.scope) ||
        !identical(old.service, widget.service) ||
        !identical(old.contextService, widget.contextService) ||
        !widget.originalCurrent()) {
      retired = true;
      generation++;
    }
  }

  @override
  void dispose() {
    generation++;
    widget.originalLive.removeListener(retire);
    for (final c in [name, cpu, memory, disk, days, retention, samples]) {
      c.dispose();
    }
    super.dispose();
  }

  Future<void> load() async {
    final token = ++generation;
    try {
      if (!current) return;
      final events = ref.read(criticalRuntimeEventsProvider), snapshot = events?.currentScope, frame = snapshot?.scope;
      if (frame == null ||
          snapshot?.streamUnavailable != false ||
          snapshot?.unsupported != false ||
          frame.profileId != widget.scope.profileId?.toString()) {
        throw const FormatException('monitoring_node_scope');
      }
      final expected = AgentInstallerFrame(profileId: frame.profileId, notificationScopeId: frame.nonce);
      final raw = await widget.contextService.agentInstallerContext(expected.toJson());
      if (!current ||
          token != generation ||
          !identical(ref.read(criticalRuntimeEventsProvider), events) ||
          events?.currentScope.epoch != snapshot?.epoch ||
          events?.currentScope.scope != frame) {
        return;
      }
      final value = AgentInstallerContext.parse(raw, expected);
      setState(() {
        metadata = value;
        loading = false;
      });
    } catch (e) {
      if (current && token == generation) {
        setState(() {
          loading = false;
          error = e;
        });
      }
    }
  }

  MonitoringNodePlan plan() => MonitoringNodePlan(
    cpuCores: int.tryParse(cpu.text) ?? -1,
    memoryMib: int.tryParse(memory.text) ?? -1,
    diskGib: int.tryParse(disk.text) ?? -1,
    retentionDays: int.tryParse(days.text) ?? -1,
    retentionGib: int.tryParse(retention.text) ?? -1,
    expectedSamplesPerSecond: int.tryParse(samples.text) ?? -1,
    receiverEnabled: receiver,
    alertmanagerEnabled: alertmanager,
  );
  Future<void> create() async {
    if (!canCreate) return;
    final token = ++generation;
    final original = metadata!;
    late final Map<String, dynamic> request;
    try {
      request = plan().request(context: original, host: host!, name: name.text.trim(), operation: operation);
    } catch (_) {
      setState(() => invalidPlan = true);
      return;
    }
    setState(() {
      spent = true;
      busy = true;
      invalidPlan = false;
      error = null;
    });
    try {
      final raw = await widget.service.createMonitoringNodeProject(request);
      if (!current || token != generation) return;
      final result = MonitoringNodeSourceCreated.parse(raw, original, request);
      setState(() => created = result);
    } catch (e) {
      if (current && token == generation) setState(() => error = e);
    } finally {
      if (current && token == generation) setState(() => busy = false);
    }
  }

  Widget number(TextEditingController c, String key, String label) => TextField(
    key: ValueKey(key),
    controller: c,
    enabled: !busy && !spent,
    keyboardType: TextInputType.number,
    decoration: InputDecoration(labelText: label),
  );
  @override
  Widget build(BuildContext context) {
    if (!current) return const SizedBox.shrink();
    final l = context.l10n, result = created;
    return AlertDialog(
      key: const ValueKey('monitoring-node-dialog'),
      title: Text(l.monitoringNodeSourceTitle),
      content: SizedBox(
        width: 680,
        height: (MediaQuery.sizeOf(context).height - 240).clamp(100.0, 600.0),
        child: SingleChildScrollView(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              InfoBanner(tone: BannerTone.warning, message: l.monitoringNodeSourceOnly),
              Text(l.monitoringNodeSourcePrerequisites),
              const SelectableText('/srv/consolecrypt-monitoring\n/srv/consolecrypt-monitoring/config-backup'), // l10n-ignore: exact technical path, CLI syntax or SHA value; never translated.
              Text(l.monitoringNodeSourcePrivileges),
              if (loading || busy) const LinearProgressIndicator(),
              if (error case final Object e) InfoBanner(tone: BannerTone.warning, message: errorMessage(l, e)),
              if (error != null && spent) InfoBanner(tone: BannerTone.info, message: l.agentInstallOutcomeUnknown),
              if (invalidPlan) InfoBanner(tone: BannerTone.warning, message: l.monitoringNodeSourcePlanInvalid),
              if (result == null && metadata != null) ...[
                if (metadata!.hosts.isEmpty) Text(l.agentInstallNoTargets),
                TextField(
                  key: const ValueKey('monitoring-node-name'),
                  controller: name,
                  maxLength: 120,
                  enabled: !busy && !spent,
                  onChanged: (_) => setState(() {}),
                  decoration: InputDecoration(labelText: l.automationNewProject),
                ),
                DropdownButtonFormField<AgentInstallerHost>(
                  key: const ValueKey('monitoring-node-host'),
                  initialValue: host,
                  isExpanded: true,
                  decoration: InputDecoration(labelText: l.automationTargets),
                  items: [
                    for (final h in metadata!.hosts)
                      DropdownMenuItem(
                        value: h,
                        child: Text(h.name, overflow: TextOverflow.ellipsis),
                      ),
                  ],
                  onChanged: busy || spent ? null : (v) => setState(() => host = v),
                ),
                DropdownButtonFormField<MonitoringNodeOperation>(
                  key: const ValueKey('monitoring-node-operation'),
                  initialValue: operation,
                  isExpanded: true,
                  decoration: InputDecoration(labelText: l.agentInstallOperation),
                  items: [
                    DropdownMenuItem(
                      value: MonitoringNodeOperation.install,
                      child: Text(l.monitoringNodeSourceInstall),
                    ),
                    DropdownMenuItem(
                      value: MonitoringNodeOperation.restore,
                      child: Text(l.monitoringNodeSourceRestore),
                    ),
                  ],
                  onChanged: busy || spent
                      ? null
                      : (v) {
                          if (v != null) setState(() => operation = v);
                        },
                ),
                number(cpu, 'monitoring-node-cpu', l.monitoringNodeSourceCpu),
                number(memory, 'monitoring-node-memory', l.monitoringNodeSourceMemory),
                number(disk, 'monitoring-node-disk', l.monitoringNodeSourceDisk),
                number(days, 'monitoring-node-days', l.monitoringNodeSourceDays),
                number(retention, 'monitoring-node-retention', l.monitoringNodeSourceRetention),
                number(samples, 'monitoring-node-samples', l.monitoringNodeSourceSamples),
                CheckboxListTile(
                  value: receiver,
                  onChanged: busy || spent ? null : (v) => setState(() => receiver = v == true),
                  title: Text(l.monitoringNodeSourceReceiver),
                ),
                CheckboxListTile(
                  value: alertmanager,
                  onChanged: busy || spent ? null : (v) => setState(() => alertmanager = v == true),
                  title: Text(l.monitoringNodeSourceAlertmanager),
                ),
                Text(l.monitoringNodeSourceEstimate),
                if (operation == MonitoringNodeOperation.restore)
                  InfoBanner(tone: BannerTone.warning, message: l.monitoringNodeSourceRestoreDisclosure),
              ],
              if (result != null) ...[
                InfoBanner(tone: BannerTone.info, message: l.agentInstallProjectCreated),
                SelectableText('SHA256: ${result.projectDigest}'), // l10n-ignore: exact technical path, CLI syntax or SHA value; never translated.
                Text(l.monitoringNodeSourceRuntimeUnknown),
              ],
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
          key: const ValueKey('monitoring-node-close'),
          onPressed: busy ? null : () => Navigator.pop(context),
          child: Text(l.commonClose),
        ),
        if (result == null)
          TextButton(
            key: const ValueKey('monitoring-node-create'),
            onPressed: canCreate ? create : null,
            child: Text(l.agentInstallCreate),
          ),
        if (result != null)
          TextButton(
            key: const ValueKey('monitoring-node-open'),
            onPressed: current ? () => Navigator.pop(context, result.project) : null,
            child: Text(l.agentInstallOpen),
          ),
      ],
    );
  }
}
