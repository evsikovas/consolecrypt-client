import 'dart:async';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_runner_candidate.dart';
import 'package:consolecrypt/automation/automation_runner_cleanup.dart';
import 'package:consolecrypt/automation/automation_runner_cleanup_dialog.dart';
import 'package:consolecrypt/automation/automation_scope_guard.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/automation/automation_service_delegation_service.dart';
import 'package:consolecrypt/automation/automation_wsl_wizard_entry.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/services/terminal_service.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/hosts/host_picker.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class AutomationRunnersDialog extends ConsumerStatefulWidget {
  const AutomationRunnersDialog({super.key, required this.scope, required this.service, this.initialRunner});
  final AutomationScope scope;
  final AutomationRunnerService service;
  final AutomationRunnerProfile? initialRunner;
  @override
  ConsumerState<AutomationRunnersDialog> createState() => _AutomationRunnersDialogState();
}

class _AutomationRunnersDialogState extends ConsumerState<AutomationRunnersDialog> {
  final probes = <String, AutomationJson>{};
  final runnerFormKey = GlobalKey<_RunnerFormState>();
  final formActions = ValueNotifier<({bool busy, bool canSave})>((busy: false, canSave: false));
  String? checking;
  Object? error;
  bool creating = false, scanOnOpen = false;
  AutomationRunnerProfile? editing;
  bool get current => mounted && identical(ref.read(automationScopeProvider), widget.scope);

  @override
  void initState() {
    super.initState();
    final initial = widget.initialRunner;
    if (initial != null &&
        !initial.readOnly &&
        initial.revision != null &&
        initial.configuration != null &&
        widget.service is AutomationRunnerEditingService) {
      editing = initial;
      creating = true;
    }
  }

  @override
  void dispose() {
    formActions.dispose();
    super.dispose();
  }

  Future<void> connect(AutomationRunnerProfile runner) async {
    if (checking != null) return;
    setState(() {
      checking = runner.id;
      error = null;
      probes.remove(runner.id);
    });
    try {
      final result = await widget.service.connectRunner(runner.id);
      if (current) setState(() => probes[runner.id] = result['probe'] as AutomationJson);
    } catch (e) {
      if (current) setState(() => error = e);
    } finally {
      if (current) setState(() => checking = null);
    }
  }

  Future<void> disconnect(AutomationRunnerProfile runner, {bool forget = false}) async {
    if (checking != null) return;
    setState(() {
      checking = runner.id;
      error = null;
      probes.remove(runner.id);
    });
    try {
      await widget.service.disconnectRunner(runner.id);
      if (!current) return;
      if (forget) {
        await widget.service.forgetRunner(runner.id, revision: runner.revision!);
        if (current) ref.invalidate(automationRunnersProvider);
      }
    } catch (e) {
      if (current) setState(() => error = e);
    } finally {
      if (current) setState(() => checking = null);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final catalog = ref.watch(automationRunnersProvider);
    return AlertDialog(
      title: Text(l.automationRunners),
      content: SizedBox(
        width: 700,
        height: 520,
        child: creating
            ? _RunnerForm(
                key: runnerFormKey,
                actions: formActions,
                scope: widget.scope,
                service: widget.service,
                initial: editing,
                scanOnOpen: scanOnOpen,
                onCreated: () {
                  if (current) {
                    ref.invalidate(automationRunnersProvider);
                    setState(() => creating = false);
                  }
                },
              )
            : ListView(
                children: [
                  Text(l.automationRunnerHint),
                  const SizedBox(height: 12),
                  ...catalog.when(
                    data: (value) => [
                      if (value?.readOnly == true) Text(l.automationNewerSchema),
                      if (value?.runners.isEmpty ?? false) Text(l.automationNoRunners),
                      for (final runner in value?.runners ?? <AutomationRunnerProfile>[])
                        Padding(
                          padding: const EdgeInsets.symmetric(vertical: 6),
                          child: ContentSurface(
                            child: Column(
                              crossAxisAlignment: CrossAxisAlignment.start,
                              children: [
                                Row(
                                  children: [
                                    Icon(
                                      const {'remote', 'service_remote'}.contains(runner.configuration?['kind'])
                                          ? Icons.dns_outlined
                                          : Icons.computer_outlined,
                                      size: 20,
                                    ),
                                    const SizedBox(width: 10),
                                    Expanded(
                                      child: Text(
                                        runner.name ?? runner.id,
                                        style: Theme.of(context).textTheme.titleSmall,
                                      ),
                                    ),
                                  ],
                                ),
                                const SizedBox(height: 4),
                                Text(
                                  runner.configuration?['kind'] == 'service_remote'
                                      ? l.automationServiceConnectionMode
                                      : runner.configuration?['kind'] == 'remote'
                                      ? l.automationRemoteRunner
                                      : l.automationLocalRunner,
                                ),
                                const SizedBox(height: 8),
                                Wrap(
                                  spacing: 8,
                                  runSpacing: 8,
                                  crossAxisAlignment: WrapCrossAlignment.center,
                                  children: [
                                    if (runner.configuration?['kind'] != 'service_remote') GlassButton(
                                      key: ValueKey('automation-diagnose-${runner.id}'),
                                      label: l.automationDiagnose,
                                      size: GlassControlSize.sm,
                                      busy: checking == runner.id,
                                      onPressed: checking != null || runner.readOnly ? null : () => connect(runner),
                                    ),
                                    if (widget.service is AutomationRunnerEditingService &&
                                        widget.service is AutomationRunnerSetupService &&
                                        runner.configuration?['kind'] != 'service_remote' &&
                                        runner.configuration != null)
                                      GlassButton(
                                        key: ValueKey('automation-update-runner-${runner.id}'),
                                        label: l.automationUpdateRunner,
                                        icon: Icons.system_update_alt,
                                        size: GlassControlSize.sm,
                                        onPressed: checking != null || runner.readOnly || value?.readOnly == true
                                            ? null
                                            : () => setState(() {
                                                editing = runner;
                                                creating = true;
                                                scanOnOpen = true;
                                                error = null;
                                              }),
                                      ),
                                    if (widget.service is AutomationRunnerEditingService)
                                      GlassIconButton(
                                        key: ValueKey('automation-edit-runner-${runner.id}'),
                                        tooltip: l.commonEdit,
                                        icon: Icons.edit_outlined,
                                      size: GlassControlSize.sm.height,
                                      iconSize: 14,
                                        onPressed: checking != null || runner.readOnly
                                            ? null
                                            : () => setState(() {
                                                editing = runner;
                                                creating = true;
                                                scanOnOpen = false;
                                                error = null;
                                              }),
                                      ),
                                    GlassIconButton(
                                      tooltip: l.automationDisconnectRunner,
                                      icon: Icons.link_off,
                                      size: GlassControlSize.sm.height,
                                      iconSize: 14,
                                      onPressed: checking != null || runner.readOnly ? null : () => disconnect(runner),
                                    ),
                                    GlassIconButton(
                                      tooltip: l.automationForgetRunner,
                                      icon: Icons.delete_outline,
                                      size: GlassControlSize.sm.height,
                                      iconSize: 14,
                                      onPressed: checking != null || runner.readOnly
                                          ? null
                                          : () => disconnect(runner, forget: true),
                                    ),
                                  ],
                                ),
                                if (runner.configuration?['kind'] == 'service_remote') ...[
                                  const SizedBox(height: 6),
                                  Text(l.automationServiceConnectionManageHint),
                                ],
                                if (probes[runner.id] case final result?) ...[
                                  const SizedBox(height: 12),
                                  _RunnerProbe(key: ValueKey('automation-runner-probe-${runner.id}'), result: result),
                                ],
                              ],
                            ),
                          ),
                        ),
                      const SizedBox(height: 12),
                      GlassButton(
                        label: l.automationAddRunner,
                        icon: Icons.add,
                        size: GlassControlSize.sm,
                        onPressed: checking != null || value?.readOnly == true
                            ? null
                            : () => setState(() {
                                creating = true;
                                editing = null;
                                scanOnOpen = false;
                                error = null;
                              }),
                      ),
                    ],
                    error: (e, _) => [
                      Text(errorMessage(l, e)),
                      TextButton(
                        onPressed: () => ref.invalidate(automationRunnersProvider),
                        child: Text(l.commonRetry),
                      ),
                    ],
                    loading: () => [const LinearProgressIndicator()],
                  ),
                  if (error != null)
                    Padding(
                      padding: const EdgeInsets.symmetric(vertical: 12),
                      child: Text(
                        errorMessage(l, error!),
                        style: TextStyle(color: Theme.of(context).colorScheme.error),
                      ),
                    ),
                ],
              ),
      ),
      actions: [
        ValueListenableBuilder(
          valueListenable: formActions,
          builder: (context, state, _) => Wrap(
            key: const ValueKey('automation-runner-footer'),
            alignment: WrapAlignment.end,
            spacing: 8,
            runSpacing: 8,
            children: [
              if (creating)
                GlassButton(
                  key: const ValueKey('automation-runner-save'),
                  label: l.commonSave,
                  size: GlassControlSize.sm,
                  busy: state.busy,
                  onPressed: state.busy || !state.canSave ? null : () => runnerFormKey.currentState?.save(),
                ),
              if (creating) TextButton(onPressed: () => setState(() => creating = false), child: Text(l.commonBack)),
              TextButton(onPressed: () => Navigator.pop(context), child: Text(l.commonClose)),
            ],
          ),
        ),
      ],
    );
  }
}

class _RunnerProbe extends StatelessWidget {
  const _RunnerProbe({super.key, required this.result});
  final AutomationJson result;
  @override
  Widget build(BuildContext context) {
    final l = context.l10n, theme = Theme.of(context);
    final sandbox = result['sandbox'] as AutomationJson;
    final tools = result['tools'];
    final ssh = tools is Map ? tools['ssh'] : null;
    final helper = result['helper'] is String ? result['helper'] as String : l.automationResultMissing;
    final capabilities = result['capabilities'];
    final macOS = result['system'] == 'Darwin';
    final processIsolation = macOS
        ? capabilities is Map && capabilities['macos_worker_sandbox_v1'] == true
        : sandbox['pid'] == true;
    final ready =
        result['dependencies_ready'] == true &&
        result['non_root'] == true &&
        sandbox['filesystem'] == true &&
        sandbox['network'] == true &&
        processIsolation;
    final components = Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(l.automationRunnerComponents, style: theme.textTheme.labelLarge),
        const SizedBox(height: 6),
        Text('${l.automationRunnerHelperVersion}: $helper'),
        if (ssh is bool)
          _CheckRow(label: 'OpenSSH', ready: ssh)
        else
          Text('OpenSSH: ${l.automationResultMissing}'), // l10n-ignore: fixed product name and localized unknown value

        for (final entry in (result['packages'] as AutomationJson).entries)
          _CheckRow(
            label: '${entry.key}: ${(entry.value as AutomationJson)['actual'] ?? '—'}', // l10n-ignore: package identity and recorded version
            ready: (entry.value as AutomationJson)['compatible'] == true,
          ),
      ],
    );
    final isolation = Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(l.automationRunnerIsolation, style: theme.textTheme.labelLarge),
        const SizedBox(height: 6),
        _CheckRow(label: l.automationDependencyLock, ready: result['dependencies_ready'] == true),
        _CheckRow(label: l.automationNonRoot, ready: result['non_root'] == true),
        _CheckRow(label: l.automationFilesystemIsolation, ready: sandbox['filesystem'] == true),
        _CheckRow(label: l.automationNetworkIsolation, ready: sandbox['network'] == true),
        _CheckRow(label: macOS ? l.automationMacProcessIsolation : l.automationProcessIsolation, ready: processIsolation),
      ],
    );
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        const Divider(height: 1),
        const SizedBox(height: 12),
        Row(
          children: [
            Icon(
              ready ? Icons.check_circle_outline : Icons.error_outline,
              size: 18,
              color: ready ? theme.colorScheme.primary : theme.colorScheme.error,
            ),
            const SizedBox(width: 8),
            Expanded(
              child: Text(
                ready ? l.automationRunnerReady : l.automationRunnerNeedsSetup,
                style: theme.textTheme.labelLarge,
              ),
            ),
          ],
        ),
        const SizedBox(height: 4),
        Text /* l10n-ignore: platform and package receipt */ (
          // l10n-ignore: recorded platform and package receipt
          '${result['system']} · ${result['machine']} · Python ${result['python']}',
          style: theme.textTheme.bodySmall,
        ),
        ExpansionTile(
          key: ValueKey('runner-probe-details-${result['environment_digest']}'),
          title: Text(l.automationRunnerDetails),
          initiallyExpanded: !ready,
          tilePadding: EdgeInsets.zero,
          childrenPadding: const EdgeInsets.only(bottom: 8),
          children: [
            LayoutBuilder(
              builder: (context, constraints) => constraints.maxWidth >= 500
                  ? Row(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Expanded(child: components),
                        const SizedBox(width: 20),
                        Expanded(child: isolation),
                      ],
                    )
                  : Column(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [components, const SizedBox(height: 12), isolation],
                    ),
            ),
            const SizedBox(height: 10),
            Text(l.automationProbeOnly, style: theme.textTheme.bodySmall),
          ],
        ),
      ],
    );
  }
}

class _CheckRow extends StatelessWidget {
  const _CheckRow({required this.label, required this.ready});
  final String label;
  final bool ready;
  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.symmetric(vertical: 3),
    child: Row(
      children: [
        Icon(
          ready ? Icons.check_circle_outline : Icons.error_outline,
          semanticLabel: ready ? context.l10n.automationCheckReady : context.l10n.automationCheckUnavailable,
          size: 16,
          color: ready ? Theme.of(context).colorScheme.primary : Theme.of(context).colorScheme.error,
        ),
        const SizedBox(width: 8),
        Expanded(child: Text(label)),
      ],
    ),
  );
}

class _RunnerForm extends ConsumerStatefulWidget {
  const _RunnerForm({
    super.key,
    required this.scope,
    required this.service,
    required this.onCreated,
    required this.actions,
    this.initial,
    this.scanOnOpen = false,
  });
  final AutomationScope scope;
  final AutomationRunnerService service;
  final VoidCallback onCreated;
  final ValueNotifier<({bool busy, bool canSave})> actions;
  final AutomationRunnerProfile? initial;
  final bool scanOnOpen;
  @override
  ConsumerState<_RunnerForm> createState() => _RunnerFormState();
}

class _RunnerFormState extends ConsumerState<_RunnerForm> {
  final name = TextEditingController(),
      user = TextEditingController(),
      python = TextEditingController(),
      module = TextEditingController(),
      serviceIdentity = TextEditingController(),
      root = TextEditingController();
  bool get serviceMode => mode == 'service_remote';
  bool get macNative => AppPlatform.isMacOS && mode == 'native';
  bool get serviceCapability => widget.service is AutomationServiceDelegationService;
  bool get validServiceIdentity => RegExp(r'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$')
      .hasMatch(serviceIdentity.text.trim()) && serviceIdentity.text.trim() != '00000000-0000-0000-0000-000000000000';
  late String mode = AppPlatform.isWindows
      ? 'wsl'
      : AppPlatform.isLinux || AppPlatform.isMacOS
      ? 'native'
      : 'remote';
  String? distribution;
  Host? host;
  String? savedHostId;
  Object? error;
  bool actionsReady = false;
  bool _busy = false;
  bool get busy => _busy;
  set busy(bool value) {
    _busy = value;
    publishActions();
  }

  void publishActions() {
    if (!mounted || !actionsReady) return;
    final ready = serviceMode
        ? serviceCapability && name.text.trim().isNotEmpty && validServiceIdentity && (host != null || savedHostId != null)
        : ![python, module, root].any((field) => field.text.isEmpty);
    widget.actions.value = (busy: busy, canSave: ready);
  }

  AutomationJson? detected;
  String? setupId;
  late Future<LocalShellProfiles> profiles = ref.read(terminalServiceProvider).localProfiles();
  bool get current => mounted && identical(ref.read(automationScopeProvider), widget.scope);
  bool get needsUpdate {
    final outdated = (detected?['outdated_candidates'] as List? ?? []).cast<String>();
    final candidates = detected?['candidates'] as List? ?? [];
    // An explicit update also covers old receipts that predate capability discovery.
    return (widget.initial != null && widget.scanOnOpen) ||
        (widget.initial != null && candidates.isEmpty) ||
        outdated.isNotEmpty && (python.text.isEmpty || outdated.contains(python.text));
  }

  @override
  void initState() {
    super.initState();
    for (final controller in [name, python, module, root, serviceIdentity]) {
      controller.addListener(publishActions);
    }
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) {
        actionsReady = true;
        publishActions();
      }
    });
    final initial = widget.initial, configuration = widget.initial?.configuration;
    if (initial == null || configuration == null) return;
    name.text = initial.name ?? '';
    mode = const {'remote', 'service_remote'}.contains(configuration['kind'])
        ? configuration['kind'] as String : (configuration['launch'] as AutomationJson)['kind'] as String;
    if (serviceMode) {
      serviceIdentity.text = configuration['identity_id'] as String? ?? '';
    } else {
      final paths = (configuration[mode == 'remote' ? 'helper' : 'launch'] as AutomationJson);
      python.text = paths['python'] as String;
      module.text = paths['module_dir'] as String;
      root.text = paths['root'] as String;
      user.text = paths['user'] as String? ?? '';
      distribution = paths['distribution'] as String?;
    }
    savedHostId = configuration['host_id'] as String?;
    for (final candidate in ref.read(hostsProvider).value ?? <Host>[]) {
      if (candidate.id.value == savedHostId) host = candidate;
    }
    if (!serviceMode && widget.scanOnOpen && widget.service is AutomationRunnerSetupService) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (current) detect();
      });
    }
  }

  AutomationJson get setupTarget => mode == 'remote'
      ? {'kind': 'remote', 'host_id': host?.id.value ?? savedHostId}
      : mode == 'wsl'
      ? {'kind': 'wsl', 'distribution': distribution, 'user': user.text.trim().isEmpty ? null : user.text.trim()}
      : {'kind': 'native'};

  void resetDetection() {
    detected = null;
    user.clear();
    python.clear();
    module.clear();
    root.clear();
  }

  void applyPaths(AutomationJson paths) {
    python.text = paths['python'] as String;
    module.text = paths['module_dir'] as String;
    root.text = paths['root'] as String;
    if (name.text.trim().isEmpty) {
      name.text = mode == 'remote'
          ? (host?.name ?? context.l10n.automationRemoteOrigin)
          : (distribution ?? context.l10n.automationLocalRunner);
    }
  }

  Future<void> detect({bool install = false, bool preserveSelection = false}) async {
    final service = widget.service;
    if (busy ||
        service is! AutomationRunnerSetupService ||
        mode == 'remote' && host == null && savedHostId == null ||
        mode == 'wsl' && distribution == null) {
      return;
    }
    if (install) {
      final l = context.l10n;
      final updating = needsUpdate;
      final label = updating ? l.automationUpdateRunner : l.automationInstallRunner;
      final approved = await showGlassDialog<bool>(
        context,
        builder: (context) => AutomationScopeGuard(
          scope: widget.scope,
          builder: (_) => AlertDialog(
            title: Text(label),
            content: SingleChildScrollView(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                mainAxisSize: MainAxisSize.min,
                children: [
                  if (updating) ...[Text(l.automationRunnerUpdatePlanHint), const SizedBox(height: 12)],
                  Text(l.automationInstallRunnerPlan),
                  const SizedBox(height: 12),
                  for (final package in (detected?['required_packages'] as AutomationJson? ?? {}).entries)
                    Text('${package.key}: ${package.value}'),
                  const SizedBox(height: 12),
                  Text('${detected?['install_parent'] ?? ''}'), // l10n-ignore: detected installation path
                ],
              ),
            ),
            actions: [
              TextButton(onPressed: () => Navigator.pop(context, false), child: Text(l.commonCancel)),
              GlassButton(label: label, onPressed: () => Navigator.pop(context, true)),
            ],
          ),
        ),
      );
      if (!current || approved != true) return;
    }
    setState(() {
      busy = true;
      error = null;
      setupId = ObjectId.generate().value;
    });
    final operationId = setupId!;
    final selectedPython = python.text;
    try {
      final result = await (service as AutomationRunnerSetupService).setupRunner(
        setupTarget,
        operationId: operationId,
        install: install,
      );
      if (!current) return;
      setState(() {
        if (install) {
          applyPaths(result['paths'] as AutomationJson);
          detected = {
            ...?detected,
            'candidates': [result['paths']],
            'installed': true,
            'outdated_candidates': <String>[],
            'probe': result['probe'],
          };
        } else {
          detected = result;
          if (result['non_root'] == true) user.text = result['user'] as String;
          final candidates = result['candidates'] as List;
          final selected = candidates
              .cast<AutomationJson>()
              .where((paths) => paths['python'] == selectedPython)
              .firstOrNull;
          if ((widget.initial != null || preserveSelection) && selected != null) {
            applyPaths(selected);
          } else if (!preserveSelection && candidates.length == 1) {
            applyPaths(candidates.single as AutomationJson);
          } else {
            python.clear();
            module.clear();
            root.clear();
          }
        }
      });
    } catch (e) {
      if (current && !(e is AppException && e.code == AppErrorCode.cancelled)) setState(() => error = e);
    } finally {
      if (current) {
        setState(() {
          busy = false;
          setupId = null;
        });
      }
    }
  }

  void cancelSetup() {
    final id = setupId, service = widget.service;
    if (id != null && service is AutomationRunnerSetupService) {
      unawaited(
        (service as AutomationRunnerSetupService)
            .cancelRunnerSetup(id)
            .then<void>((_) {}, onError: (Object _, StackTrace _) {}),
      );
    }
  }

  Future<void> cleanupEnvironments({AutomationJson? candidate}) async {
    final service = widget.service;
    if (busy || service is! AutomationRunnerCleanupService) return;
    RunnerCandidateSelection? expectedCandidate;
    try {
      expectedCandidate = candidate == null ? null : RunnerCandidateSelection.fromPaths(candidate);
    } on RunnerCandidateUnavailable catch (e) {
      if (current) setState(() => error = e);
      return;
    }
    final target = setupTarget;
    final selected = python.text.trim().isEmpty ? null : python.text.trim();
    final expected = runnerCleanupContext(target, selected);
    var mutationAttempted = false;
    final targetLabel = mode == 'remote'
        ? (host?.name ?? savedHostId)
        : mode == 'wsl'
        ? distribution
        : context.l10n.automationLocalRunner;
    await showGlassDialog<void>(
      context,
      builder: (_) => AutomationRunnerCleanupDialog(
        scope: widget.scope,
        service: service as AutomationRunnerCleanupService,
        target: target,
        selectedPython: selected,
        targetLabel: targetLabel,
        contextChanges: Listenable.merge([python, user]),
        contextCurrent: () =>
            current &&
            runnerCleanupContext(setupTarget, python.text.trim().isEmpty ? null : python.text.trim()) == expected,
        onMutationAttempted: () => mutationAttempted = true,
        expectedCandidate: expectedCandidate,
      ),
    );
    if (!current || !mutationAttempted) return;
    ref.invalidate(automationRunnersProvider);
    if (service is AutomationRunnerSetupService) await detect(preserveSelection: true);
  }

  Future<void> executePrerequisites(String command) async {
    final service = widget.service;
    if (busy || service is! AutomationRunnerPrerequisiteTerminalService) return;
    final terminals = ref.read(terminalTabsProvider.notifier), router = ref.read(routerProvider);
    setState(() {
      busy = true;
      error = null;
      setupId = ObjectId.generate().value;
    });
    try {
      final id = await (service as AutomationRunnerPrerequisiteTerminalService).openPrerequisiteTerminal(
        setupTarget,
        operationId: setupId!,
        command: command,
      );
      final tab = await terminals.openRuntime(id);
      if (!mounted || !current) {
        await terminals.close(tab);
        return;
      }
      setupId = null;
      Navigator.pop(context);
      router.go(AppRoutes.terminal);
    } catch (e) {
      if (current && !(e is AppException && e.code == AppErrorCode.cancelled)) setState(() => error = e);
    } finally {
      if (current) {
        setState(() {
          busy = false;
          setupId = null;
        });
      }
    }
  }

  @override
  void dispose() {
    cancelSetup();
    for (final controller in [name, python, module, root, serviceIdentity]) {
      controller.removeListener(publishActions);
    }
    for (final controller in [name, user, python, module, root, serviceIdentity]) {
      controller.dispose();
    }
    super.dispose();
  }

  Future<void> save() async {
    if (busy || !current) return;
    if (name.text.trim().isEmpty ||
        serviceMode && (!serviceCapability || !validServiceIdentity || host == null && savedHostId == null) ||
        !serviceMode && [python, module, root].any((field) => field.text.trim().isEmpty) ||
        mode == 'wsl' && (distribution == null || user.text.trim().isEmpty) ||
        mode == 'remote' && host == null && savedHostId == null) {
      setState(() => error = const _RequiredFields());
      return;
    }
    final paths = <String, dynamic>{
      'python': python.text.trim(),
      'module_dir': module.text.trim(),
      'root': root.text.trim(),
    };
    final configuration = serviceMode
        ? <String, dynamic>{'kind': 'service_remote', 'host_id': host?.id.value ?? savedHostId, 'identity_id': serviceIdentity.text.trim()}
        : mode == 'remote'
        ? <String, dynamic>{'kind': 'remote', 'host_id': host?.id.value ?? savedHostId, 'helper': paths}
        : <String, dynamic>{
            'kind': 'local',
            'launch': {
              'kind': mode,
              ...paths,
              if (mode == 'wsl') ...{'distribution': distribution, 'user': user.text.trim()},
            },
          };
    setState(() {
      busy = true;
      error = null;
    });
    try {
      final initial = widget.initial;
      if (initial == null) {
        await widget.service.createRunner(name: name.text.trim(), configuration: configuration);
      } else {
        await widget.service.disconnectRunner(initial.id);
        if (!current) return;
        await (widget.service as AutomationRunnerEditingService).updateRunner(
          initial.id,
          revision: initial.revision!,
          name: name.text.trim(),
          configuration: configuration,
        );
      }
      if (current) widget.onCreated();
    } catch (e) {
      if (current) setState(() => error = e);
    } finally {
      if (current) setState(() => busy = false);
    }
  }

  Future<void> chooseHost() async {
    final result = await showGlassDialog<Host>(
      context,
      builder: (_) => AutomationScopeGuard(scope: widget.scope, builder: (_) => const HostPickerDialog()),
    );
    if (current && result != null) {
      setState(() {
        host = result;
        savedHostId = result.id.value;
        if (serviceMode && name.text.trim().isEmpty) name.text = result.name;
        resetDetection();
        publishActions();
      });
    }
  }

  String nativeOsFact(AutomationJson result, String key) => nativeFact(
    result['os_release'] is Map ? (result['os_release'] as Map)[key] : null);

  String nativeFact(Object? value) => value is String && value.isNotEmpty && value.runes.length <= 256 &&
    !value.runes.any((rune) => rune < 32 || rune == 127) ? value : context.l10n.automationResultMissing;

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final scan = detected;
    final kernel = nativeFact(scan?['release']);
    final osName = scan == null ? l.automationResultMissing : nativeOsFact(scan, 'pretty_name');
    final osVersion = scan == null ? l.automationResultMissing : nativeOsFact(scan, 'version_id');
    Widget field(String label, TextEditingController controller, {String? hint}) => Padding(
      padding: const EdgeInsets.only(bottom: 12),
      child: TextField(
        key: identical(controller, serviceIdentity) ? const ValueKey('automation-service-identity') : null,
        controller: controller,
        enabled: !busy,
        onChanged: (_) {
          if (!identical(controller, name) && detected != null) setState(() => detected = null);
        },
        decoration: InputDecoration(labelText: label, hintText: hint),
      ),
    );
    return Column(
      children: [
        if (error != null)
          InfoBanner(
            key: const ValueKey('automation-runner-setup-error'),
            message: error is RunnerCandidateUnavailable
                ? l.automationRunnerCandidateUnavailable
                : error is _RequiredFields
                ? l.automationRequiredFields
                : error is AutomationRunnerPrerequisiteFailed
                ? switch ((error as AutomationRunnerPrerequisiteFailed).reason) {
                    'prerequisites_changed' ||
                    'ssh_environment_changed' ||
                    'ssh_account_changed' => l.automationPrerequisiteRescan,
                    'ssh_route_unsupported' ||
                    'prerequisite_terminal_unavailable' => l.automationPrerequisiteRouteUnavailable,
                    _ => l.automationPrerequisiteCommandRejected,
                  }
                : error is AutomationRunnerSetupFailed
                ? switch ((error as AutomationRunnerSetupFailed).reason) {
                    'setup_bootstrap_failed' => l.automationSetupBootstrapFailed,
                    'setup_command_too_large' => l.automationSetupCommandTooLarge,
                    'setup_python_missing' => l.automationSetupPythonMissing,
                    'setup_macos_python_missing' => l.automationMacRunnerPython,
                    'environment_check_failed' when macNative => l.automationMacRunnerIsolationFailed,
                    _ => l.automationSetupFailed,
                  }
                : errorMessage(l, error!),
            tone: BannerTone.danger,
          ),
        Expanded(
          child: ListView(
            children: [
              if (macNative)
                Text(l.automationMacRunnerNative, key: const ValueKey('automation-macos-runner-help'))
              else if (!serviceMode)
                Text(l.automationSetupHint),
              if (widget.initial != null) Text(l.automationEditRunnerHint),
              const SizedBox(height: 12),
              field(l.commonName, name),
              GlassSelect<String>(
                key: const ValueKey('automation-runner-mode'),
                value: mode,
                expand: true,
                onChanged: busy
                    ? null
                    : (value) => setState(() {
                        mode = value;
                        resetDetection();
                        publishActions();
                      }),
                items: [
                  if (AppPlatform.isWindows || AppPlatform.isLinux || AppPlatform.isMacOS)
                    GlassSelectItem(
                      value: AppPlatform.isWindows ? 'wsl' : 'native',
                      label: AppPlatform.isWindows ? 'WSL2' : l.automationLocalRunner,
                    ),
                  GlassSelectItem(value: 'remote', label: l.automationRemoteRunner),
                  if (serviceCapability || serviceMode)
                    GlassSelectItem(value: 'service_remote', label: l.automationServiceConnectionMode),
                ],
              ),
              const SizedBox(height: 12),
              if (mode == 'remote') ...[
                Text(l.automationRemoteRunnerPurpose),
                const SizedBox(height: 8),
              ],
              if (mode == 'wsl') ...[
                FutureBuilder<LocalShellProfiles>(
                  future: profiles,
                  builder: (context, snapshot) {
                    final distros =
                        (snapshot.data?.profiles ?? <LocalShellProfile>[])
                            .map((profile) => profile.distribution)
                            .whereType<String>()
                            .toSet()
                            .toList()
                          ..sort();
                    final loading = snapshot.connectionState != ConnectionState.done;
                    final discovery = snapshot.data?.wslDiscovery;
                    final discoveryMessage = switch (discovery) {
                      WslDiscoveryStatus.executableMissing => l.automationWslExecutableMissing,
                      WslDiscoveryStatus.spawnFailed => l.automationWslSpawnFailed,
                      WslDiscoveryStatus.timedOut => l.automationWslTimedOut,
                      WslDiscoveryStatus.ioFailed => l.automationWslReadFailed,
                      WslDiscoveryStatus.outputTooLarge => l.automationWslOutputTooLarge,
                      WslDiscoveryStatus.nonzeroExit => l.automationWslNonzeroExit,
                      WslDiscoveryStatus.empty => l.automationWslDiscoveryEmpty,
                      _ => l.automationWslDiscoveryUnknown,
                    };
                    return Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
                      if (loading) const LinearProgressIndicator(key: ValueKey('automation-wsl-loading')),
                      if (snapshot.hasError)
                        InfoBanner(tone: BannerTone.danger, message: l.automationWslDiscoveryFailed),
                      if (!loading && !snapshot.hasError && distros.isEmpty)
                        InfoBanner(key: const ValueKey('automation-wsl-discovery-status'),
                          tone: discovery == null || discovery == WslDiscoveryStatus.empty ||
                            discovery == WslDiscoveryStatus.executableMissing || discovery == WslDiscoveryStatus.available
                            ? BannerTone.warning : BannerTone.danger,
                          message: discoveryMessage),
                      GlassSelect<String?>(
                      key: const ValueKey('automation-wsl-distribution'),
                      value: distribution,
                      expand: true,
                      semanticLabel: l.automationDistribution,
                      items: [
                        GlassSelectItem(value: null, label: l.automationDistribution),
                        if (distribution != null && !distros.contains(distribution))
                          GlassSelectItem(value: distribution, label: distribution!),
                        for (final distro in distros) GlassSelectItem(value: distro, label: distro),
                      ],
                      onChanged: busy || loading || snapshot.hasError
                          ? null
                          : (value) => setState(() {
                              distribution = value;
                              resetDetection();
                            }),
                      ),
                      Align(alignment: Alignment.centerLeft, child: TextButton.icon(
                        key: const ValueKey('automation-wsl-refresh'), icon: const Icon(Icons.refresh),
                        label: Text(l.automationWslRefresh), onPressed: busy || loading ? null : () => setState(() {
                          profiles = ref.read(terminalServiceProvider).localProfiles();
                        }))),
                      Text(l.automationWslDiscoveryHint),
                      Align(alignment: Alignment.centerLeft, child: TextButton.icon(
                        key: const ValueKey('automation-wsl-preparation'),
                        icon: const Icon(Icons.build_outlined, size: 16),
                        label: Text(l.automationWslPreparationTitle),
                        onPressed: busy ? null : () => showAutomationWslWizard(context: context,ref: ref,scope: widget.scope,service: widget.service,originalCurrent: ()=>current&&mode=='wsl',profiles: snapshot.data,distribution: distribution,user: user.text,onPrepared: (selection){
                          if(!current||mode!='wsl')return;setState((){distribution=selection.distribution;user.text=selection.user??'';detected=null;applyPaths(selection.paths);});
                        }),
                      )),
                    ]);
                  },
                ),
                const SizedBox(height: 12),
                if (detected case final result?)
                  if ((result['users'] as List? ?? []).length > 1 || result['non_root'] != true) ...[
                    GlassSelect<String?>(
                      value: user.text.isEmpty ? null : user.text,
                      expand: true,
                      items: [
                        GlassSelectItem(value: null, label: l.automationLinuxUser),
                        for (final value in result['users'] as List? ?? [])
                          GlassSelectItem(value: value as String, label: value),
                      ],
                      onChanged: busy
                          ? null
                          : (value) => setState(() {
                              user.text = value ?? '';
                              detected = null;
                              python.clear();
                              module.clear();
                              root.clear();
                            }),
                    ),
                    const SizedBox(height: 12),
                  ],
              ],
              if (mode == 'remote' || serviceMode) ...[
                Text(l.automationRunnerSshRouteHint),
                const SizedBox(height: 8),
                GlassButton(
                  key: const ValueKey('automation-runner-choose-host'),
                  label: host?.name ?? savedHostId ?? l.automationChooseRunnerHost,
                  icon: Icons.dns_outlined,
                  onPressed: busy ? null : chooseHost,
                ),
                const SizedBox(height: 12),
              ],
              if (serviceMode) ...[
                Text(serviceCapability ? l.automationServiceConnectionHint : l.automationNewerSchema),
                const SizedBox(height: 12),
                field(l.automationServiceConnectionIdentity, serviceIdentity),
              ],
              if (!serviceMode && widget.service is AutomationRunnerSetupService) ...[
                GlassButton(
                  key: const ValueKey('automation-detect-runner'),
                  label: l.automationDetectRunner,
                  icon: Icons.search,
                  busy: busy,
                  onPressed:
                      busy ||
                          mode == 'wsl' && distribution == null ||
                          mode == 'remote' && host == null && savedHostId == null
                      ? null
                      : () => detect(),
                ),
                const SizedBox(height: 12),
                if (detected case final result?) ...[
                  Text /* l10n-ignore: platform and Python version receipt */ (
                    '${result['system']} · ${result['machine']} · Python ${result['python']} · ${result['user']}',
                  ), // l10n-ignore: recorded platform, Python product version and user
                  const SizedBox(height: 8),
                  Text('${l.automationAiContextKernel}: $kernel',
                    key: const ValueKey('automation-runner-kernel')),
                  Text('${l.automationAiContextOsVersion}: $osName · $osVersion',
                    key: const ValueKey('automation-runner-os-version')),
                  if (mode == 'wsl')
                    if (result['wsl2'] case final bool wsl2)
                      _CheckRow(label: 'WSL2', ready: wsl2) // l10n-ignore: fixed WSL product major-version identifier
                    else
                      Text('WSL2: ${l.automationResultMissing}'), // l10n-ignore: fixed product version and localized unknown

                  if (result['installed'] == true) ...[
                    if (result['probe'] case final AutomationJson probe when probe['sandbox'] is Map && probe['packages'] is Map)
                      _RunnerProbe(result: probe)
                    else
                      InfoBanner(tone: BannerTone.warning, message: l.automationRunnerProbeUnavailable),
                  ],
                  if (mode == 'remote' && result['non_root'] != true) ...[
                    Text(l.automationRemoteNonRootHint, style: TextStyle(color: Theme.of(context).colorScheme.error)),
                    const SizedBox(height: 8),
                  ],
                  if ((result['missing'] as List? ?? []).isNotEmpty)
                    Text /* l10n-ignore: localized missing label and package identifiers */ (
                      // l10n-ignore: localized missing-component label plus package identifiers
                      '${l.automationSetupMissing}: ${(result['missing'] as List).map((key) => switch (key) {
                        'bwrap' => 'bubblewrap',
                        'macos_architecture' => l.automationMacRunnerArchitecture,
                        'supported_python' => macNative ? l.automationMacRunnerPython : l.automationSetupPython,
                        'python_venv' => 'python3-venv / ensurepip',
                        'non_root_user' => l.automationLinuxUser,
                        'wsl2' => 'WSL2',
                        'namespaces' => l.automationFilesystemIsolation,
                        'linux_home' => l.automationStatePath,
                        'executable_home' => l.automationSetupExecutableHome,
                        _ => key,
                      }).join(', ')}',
                    ),
                  if ((result['candidates'] as List? ?? []).isEmpty) Text(l.automationSetupNotFound),
                  if (result['prerequisite_command'] case final String command) ...[
                    const SizedBox(height: 8),
                    Text(l.automationSetupSystemHelp),
                    SelectableText(command),
                    if (widget.service is AutomationRunnerPrerequisiteTerminalService)
                      GlassButton(
                        key: const ValueKey('automation-runner-execute-prerequisites'),
                        label: l.automationSetupRunTerminal,
                        icon: Icons.terminal,
                        onPressed: busy ? null : () => executePrerequisites(command),
                      ),
                    TextButton.icon(
                      key: const ValueKey('automation-runner-copy-prerequisites'),
                      onPressed: busy ? null : () => copyPlainWithNotice(context, ref, command),
                      icon: const Icon(Icons.copy_outlined),
                      label: Text(l.commonCopy),
                    ),
                  ],
                  for (final paths in (result['candidates'] as List? ?? []).cast<AutomationJson>())
                    Row(
                      children: [
                        Expanded(
                          child: CheckboxListTile(
                            key: ValueKey('automation-runner-candidate-${paths['python']}'),
                            value: python.text == paths['python'],
                            title: Text(paths['python'] as String),
                            subtitle: (result['outdated_candidates'] as List? ?? []).contains(paths['python'])
                                ? Text(l.automationRunnerNeedsUpdate)
                                : null,
                            dense: true,
                            onChanged: busy
                                ? null
                                : (value) => setState(() {
                                    if (value == true) {
                                      applyPaths(paths);
                                    } else {
                                      python.clear();
                                      module.clear();
                                      root.clear();
                                    }
                                  }),
                          ),
                        ),
                        if (widget.service is AutomationRunnerCleanupService)
                          GlassIconButton(
                            key: ValueKey('automation-runner-delete-candidate-${paths['python']}'),
                            tooltip: l.automationRunnerDeleteCandidate,
                            icon: Icons.delete_outline,
                            onPressed: busy ? null : () => cleanupEnvironments(candidate: paths),
                          ),
                      ],
                    ),
                  if (widget.service is AutomationRunnerCleanupService && python.text.isNotEmpty) ...[
                    Text(l.automationRunnerSelectionProtectedHint),
                    TextButton.icon(
                      key: const ValueKey('automation-runner-clear-selection'),
                      label: Text(l.automationRunnerClearSelection),
                      icon: const Icon(Icons.clear),
                      onPressed: busy
                          ? null
                          : () => setState(() {
                              python.clear();
                              module.clear();
                              root.clear();
                            }),
                    ),
                  ],
                  if (result['install_supported'] == true &&
                      result['installed'] != true &&
                      ((result['candidates'] as List? ?? []).isEmpty || needsUpdate)) ...[
                    const SizedBox(height: 8),
                    GlassButton(
                      key: const ValueKey('automation-install-runner'),
                      label: needsUpdate ? l.automationUpdateRunner : l.automationInstallRunner,
                      icon: Icons.download_outlined,
                      onPressed: busy ? null : () => detect(install: true),
                    ),
                  ],
                  const SizedBox(height: 12),
                ],
                if (busy) Text(l.automationSetupBusy),
                if (setupId != null) TextButton(onPressed: cancelSetup, child: Text(l.commonCancel)),
              ],
              if (!serviceMode && !macNative && widget.service is AutomationRunnerCleanupService) ...[
                const SizedBox(height: 8),
                GlassButton(
                  key: const ValueKey('automation-open-cleanup'),
                  label: l.automationCleanupScan,
                  icon: Icons.cleaning_services_outlined,
                  onPressed:
                      busy ||
                          mode == 'wsl' && distribution == null ||
                          mode == 'remote' && host == null && savedHostId == null
                      ? null
                      : () => cleanupEnvironments(),
                ),
                const SizedBox(height: 12),
              ],
              if (!serviceMode) ExpansionTile(
                key: const ValueKey('automation-runner-advanced'),
                title: Text(l.automationSetupManual),
                initiallyExpanded: widget.initial != null || widget.service is! AutomationRunnerSetupService,
                children: [
                  Text(l.automationPreparedEnvironment),
                  const SizedBox(height: 12),
                  if (mode == 'wsl') field(l.automationLinuxUser, user),
                  field(l.automationPythonPath, python, hint: '/path/to/venv/bin/python'),
                  field(l.automationModulePath, module, hint: '/path/to/runner/src'),
                  field(l.automationStatePath, root, hint: '/private/path/consolecrypt-runner'),
                ],
              ),
            ],
          ),
        ),
      ],
    );
  }
}

class _RequiredFields implements Exception {
  const _RequiredFields();
}
