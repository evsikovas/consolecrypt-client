import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/app/critical_notice_providers.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/automation/automation_dependency_dialog.dart' show DependencyOriginalGuard;
import 'package:consolecrypt/automation/automation_deployment_environment_selection.dart';
import 'package:consolecrypt/automation/automation_diff_dialog.dart';
import 'package:consolecrypt/automation/automation_event_view.dart';
import 'package:consolecrypt/automation/automation_job_result.dart';
import 'package:consolecrypt/automation/automation_launch_passwords.dart';
import 'package:consolecrypt/automation/automation_owner_confirmation.dart';
import 'package:consolecrypt/automation/automation_owner_confirmation_dialog.dart';
import 'package:consolecrypt/automation/automation_project_credentials.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_review_cards.dart';
import 'package:consolecrypt/automation/automation_runners_dialog.dart';
import 'package:consolecrypt/automation/automation_scope_guard.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/automation/automation_service_delegation.dart';
import 'package:consolecrypt/automation/automation_service_delegation_dialog.dart';
import 'package:consolecrypt/automation/automation_vault_selection.dart';
import 'package:consolecrypt/automation/automation_workspace_panel.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class AutomationRunDialog extends ConsumerStatefulWidget {
  const AutomationRunDialog({
    super.key,
    required this.scope,
    required this.service,
    required this.project,
    required this.playbooks,
    this.initialPlaybook,
    this.initialExtraVars,
    this.retryOf,
    this.embedded = false,
    this.scheduleOnly = false,
    this.onClose,
    this.onSubmitted,
  });
  final bool embedded;
  final bool scheduleOnly;
  final VoidCallback? onClose;
  final ValueChanged<String>? onSubmitted;
  final AutomationScope scope;
  final AutomationJobService service;
  final AutomationProject project;
  final List<String> playbooks;
  final String? initialPlaybook;
  final AutomationJson? initialExtraVars;
  final AutomationJson? retryOf;
  @override
  ConsumerState<AutomationRunDialog> createState() => _AutomationRunDialogState();
}

class _AutomationRunDialogState extends ConsumerState<AutomationRunDialog> {
  final variables = TextEditingController(text: '{}'),
      tags = TextEditingController(),
      skipTags = TextEditingController();
  final timeout = TextEditingController(text: '300'), forks = TextEditingController(text: '2');
  final manualVaultPassword = TextEditingController();
  final selectedHosts = <String>{};
  final formScroll = ScrollController(), targetsScroll = ScrollController();
  String? runner, playbook;
  bool apply = false, cancelOnLock = true, diff = false, busy = false, confirmed = false;
  bool scopeValid = true;
  bool runnerSettingsReturned = false;
  bool inventoryLoading = false;
  bool vaultEnabled = false;
  AutomationJson? vaultSelection;
  AutomationLaunchPasswords? launchPasswords;
  bool get hasManualVaultPassword => vaultEnabled && vaultSelection == null && manualVaultPassword.text.isNotEmpty;
  bool get needsVaultPassword => vaultEnabled && vaultSelection == null && !hasManualVaultPassword;
  bool get hasLaunchPasswords => hasManualVaultPassword || launchPasswords?.hasAny == true;
  ProjectCredentialsContext? deploymentContext;
  String? deploymentEnvironmentId;
  bool deploymentReady = false;
  bool Function()? deploymentCurrent;
  bool get deploymentAvailable =>
      widget.service is! AutomationProjectCredentialsService || deploymentReady && deploymentCurrent?.call() == true;
  DeploymentEnvironment? preparedEnvironment;

  AutomationJson? inventoryConfiguration;
  Set<String>? inventorySavedHosts;
  Object? inventoryError;
  AutomationJson? review;
  String? diffReviewedDigest;
  int _draftGeneration = 0;
  bool reviewChanged = false;
  Object? error;
  VoidCallback? _retireTargetSettings;
  VoidCallback? _retireOwnerConfirmation;
  VoidCallback? _retireServiceDelegation;
  bool get current => mounted && identical(ref.read(automationScopeProvider), widget.scope);
  bool get productionReview => review?['policy'] is Map && (review!['policy'] as Map)['production'] == true;
  @override
  void initState() {
    super.initState();
    if (widget.initialExtraVars case final values?) {
      variables.text = const JsonEncoder.withIndent('  ').convert(values);
    }
    ref.listenManual(automationScopeProvider, (_, next) {
      scopeValid = identical(next, widget.scope);
      if (!scopeValid) {
        launchPasswords?.wipe();
        launchPasswords = null;
        scheduleMicrotask(() {
          if (mounted) manualVaultPassword.clear();
        });
      }
    }, fireImmediately: true);
    for (final controller in [variables, tags, skipTags, timeout, forks]) {
      var text = controller.text;
      controller.addListener(() {
        if (controller.text != text) {
          text = controller.text;
          _draftGeneration++;
        }
      });
    }
    playbook = widget.playbooks.contains(widget.initialPlaybook)
        ? widget.initialPlaybook
        : widget.playbooks.firstOrNull;
    if (widget.service case final AutomationSourceInventoryService sources) {
      inventoryLoading = true;
      loadInventory(sources);
    }
  }

  Future<void> targetSettings(Host host) async {
    if (busy || !current || _retireTargetSettings != null) return;
    final scope = widget.scope, inventory = ref.read(inventoryServiceProvider);
    final app = ref.read(appServicesProvider), runtime = ref.read(criticalRuntimeEventsProvider);
    final epoch = runtime?.currentScope.epoch, router = ref.read(routerProvider);
    final uri = router.routeInformationProvider.value.uri;
    var retired = false, finished = false;
    final live = ValueNotifier<bool>(true);
    bool matches() =>
        current &&
        identical(widget.scope, scope) &&
        identical(ref.read(inventoryServiceProvider), inventory) &&
        identical(ref.read(appServicesProvider), app) &&
        identical(ref.read(criticalRuntimeEventsProvider), runtime) &&
        runtime?.currentScope.epoch == epoch &&
        runtime?.currentScope.streamUnavailable != true &&
        identical(ref.read(routerProvider), router) &&
        uri.path == AppRoutes.automation &&
        router.routeInformationProvider.value.uri == uri &&
        (ref.read(hostsProvider).value ?? const <Host>[]).any((value) => identical(value, host));
    void retire() {
      if (retired || finished) return;
      retired = true;
      scheduleMicrotask(() {
        if (!finished) live.value = false;
      });
    }

    final retirement = retire;
    _retireTargetSettings = retirement;
    bool original() {
      if (!matches()) retire();
      return !retired && !finished;
    }

    void observe() {
      if (!matches()) retire();
    }

    final subscriptions = <ProviderSubscription<dynamic>>[
      ref.listenManual(appServicesProvider, (_, _) => observe()),
      ref.listenManual(inventoryServiceProvider, (_, _) => observe()),
      ref.listenManual(automationScopeProvider, (_, _) => observe()),
      ref.listenManual(hostsProvider, (_, _) => observe()),
      ref.listenManual(criticalRuntimeEventsProvider, (_, _) => observe()),
    ];
    final notices = runtime?.watchRuntimeNotices().listen(
      (_) => observe(),
      onError: (Object _, StackTrace _) => retire(),
      onDone: retire,
    );
    router.routeInformationProvider.addListener(observe);
    try {
      if (!original()) return;
      final future = inventory.resolveEffective(host); // Existing read-only preview, never a connection.
      final edit = await showGlassDialog<bool>(
        context,
        builder: (dialogContext) => DependencyOriginalGuard(
          original: live,
          builder: (_) => AutomationScopeGuard(
            scope: scope,
            builder: (_) => FutureBuilder<EffectiveHostConfig>(
              future: future,
              builder: (context, snapshot) {
                final l = context.l10n, value = snapshot.data;
                void close(bool edit) {
                  if (original() && ModalRoute.of(dialogContext)?.isCurrent == true) {
                    Navigator.of(dialogContext).pop(edit);
                  }
                }

                String source(Resolved<Object?> r) => switch (r.source) {
                  ValueSource.host =>
                    r.sourceName == null ? l.hostEditorSourceHost : l.hostEditorSourceFrom(r.sourceName!),
                  ValueSource.group => l.hostEditorSourceGroup(r.sourceName ?? ''),
                  ValueSource.jumpProfile => l.hostEditorSourceJumpProfile(r.sourceName ?? ''),
                  ValueSource.appDefault => l.hostEditorSourceDefault,
                  ValueSource.unset => l.hostEditorSourceUnset,
                };
                Widget fact(String label, String text, Resolved<Object?> r) => Padding(
                  padding: const EdgeInsets.symmetric(vertical: 8),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(label, style: Theme.of(context).textTheme.labelMedium),
                      SelectableText(text),
                      Text(source(r)),
                    ],
                  ),
                );
                return AlertDialog(
                  key: const ValueKey('automation-target-settings-dialog'),
                  title: Text(l.automationTargetSettings),
                  content: SizedBox(
                    width: 560,
                    child: ConstrainedBox(
                      constraints: BoxConstraints(maxHeight: MediaQuery.sizeOf(context).height * .6),
                      child: SingleChildScrollView(
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.stretch,
                          children: [
                            Text(host.name),
                            Text(l.automationTargetSettingsHint),
                            if (!snapshot.hasData && !snapshot.hasError) const LinearProgressIndicator(),
                            if (snapshot.hasError)
                              InfoBanner(message: l.automationTargetSettingsProblem, tone: BannerTone.warning),
                            if (value != null) ...[
                              if (value.groupPath.isNotEmpty) Text(value.groupPath.join(' › ')),
                              fact(
                                l.hostEditorEffectiveUsername,
                                value.username.value ?? l.automationResultMissing,
                                value.username,
                              ),
                              fact(
                                l.hostEditorEffectivePort,
                                value.port.value?.toString() ?? l.automationResultMissing,
                                value.port,
                              ),
                              fact(
                                l.hostEditorEffectiveRoute,
                                value.route.isEmpty
                                    ? l.hostEditorSourceUnset
                                    : value.route.map((hop) => hop.label).join(' → '),
                                value.routeSource,
                              ),
                              if (value.problems.isNotEmpty || value.diagnostics.isNotEmpty)
                                InfoBanner(message: l.automationTargetSettingsProblem, tone: BannerTone.warning),
                            ],
                          ],
                        ),
                      ),
                    ),
                  ),
                  actions: [
                    TextButton(
                      key: const ValueKey('automation-target-settings-close'),
                      onPressed: () => close(false),
                      child: Text(l.commonClose),
                    ),
                    TextButton(
                      key: const ValueKey('automation-target-settings-edit'),
                      onPressed: () => close(true),
                      child: Text(l.commonEdit),
                    ),
                  ],
                );
              },
            ),
          ),
        ),
      );
      if (edit == true && original()) router.go(AppRoutes.editHost(host.id));
    } finally {
      finished = true;
      if (identical(_retireTargetSettings, retirement)) _retireTargetSettings = null;
      for (final subscription in subscriptions) {
        subscription.close();
      }
      await notices?.cancel();
      router.routeInformationProvider.removeListener(observe);
      live.dispose();
    }
  }

  Future<void> loadInventory(AutomationSourceInventoryService service) async {
    try {
      final result = await service.inventoryConfiguration(widget.project.id);
      if (!mounted || !current) return;
      final configuration = result['configuration'] as AutomationJson?;
      if (result['version'] != 1 ||
          result['project_id'] != widget.project.id ||
          configuration != null && configuration['version'] != 1) {
        throw FormatException(context.l10n.automationInventoryChanged);
      }
      final saved = result['saved_inventory'] as AutomationJson?;
      if (saved != null && ![1, 2].contains(saved['version'])) {
        throw FormatException(context.l10n.automationInventoryChanged);
      }
      setState(() {
        inventoryConfiguration = configuration;
        inventorySavedHosts = (saved?['selected_hosts'] as List?)?.cast<String>().toSet();
      });
    } catch (e) {
      if (current) setState(() => inventoryError = e);
    } finally {
      if (current) setState(() => inventoryLoading = false);
    }
  }

  @override
  void dispose() {
    launchPasswords?.wipe();
    manualVaultPassword.clear();
    manualVaultPassword.dispose();
    _retireServiceDelegation?.call();
    _retireServiceDelegation = null;
    _retireOwnerConfirmation?.call();
    _retireOwnerConfirmation = null;
    _retireTargetSettings?.call();
    _retireTargetSettings = null;
    // Riverpod ref cannot be used from dispose. The subscription records the
    // generation before unmount, including a parent guard removed during lock.
    if (!confirmed && scopeValid) {
      if (review?['review_id'] case final String id) {
        widget.service.discardReview(id).then<void>((_) {}, onError: (Object _, StackTrace _) {});
      }
    }
    formScroll.dispose();
    targetsScroll.dispose();
    for (final controller in [variables, tags, skipTags, timeout, forks]) {
      controller.dispose();
    }
    super.dispose();
  }

  List<String> splitTags(String value) => value.split(RegExp(r'[,\s]+')).where((tag) => tag.isNotEmpty).toList();
  Future<void> passwordsForLaunch() async {
    if (!current || busy || widget.service is! AutomationRunSecretService) return;
    final app = ref.read(appServicesProvider), runtime = ref.read(criticalRuntimeEventsProvider);
    final epoch = runtime?.currentScope.epoch,
        router = ref.read(routerProvider),
        uri = ref.read(routerProvider).routeInformationProvider.value.uri;
    bool original() =>
        current &&
        identical(ref.read(appServicesProvider), app) &&
        identical(ref.read(criticalRuntimeEventsProvider), runtime) &&
        runtime?.currentScope.epoch == epoch &&
        router.routeInformationProvider.value.uri == uri;
    final value = await showAutomationLaunchPasswords(context, isCurrent: original, allowVault: !vaultEnabled);
    if (!mounted || !original()) {
      value?.wipe();
      return;
    }
    if (value == null) return;
    launchPasswords?.wipe();
    setState(() {
      launchPasswords = value.hasAny ? value : null;
      if (value.hasAny) cancelOnLock = true;
      if (value.hasVault) diff = false;
      _draftGeneration++;
    });
  }

  Future<void> validate() async {
    if (!current || busy || inventoryLoading || inventoryError != null) return;
    final launchPasswordsUnavailable = context.l10n.automationRunPasswordsUnavailable;
    final selected = ref.read(automationRunnersProvider).value?.runners.where((item) => item.id == runner).firstOrNull;
    if (selected?.configuration?['kind'] == 'service_remote') {
      setState(() => error = FormatException(context.l10n.automationServiceUseDelegation));
      return;
    }
    if (needsVaultPassword || !deploymentAvailable) return;
    if (hasManualVaultPassword) {
      launchPasswords = AutomationLaunchPasswords(
        vaultPassword: SecretText(manualVaultPassword.text),
        becomePassword: launchPasswords?.becomePassword,
      );
      manualVaultPassword.clear();
    }
    setState(() {
      busy = true;
      error = null;
      runnerSettingsReturned = false;
      reviewChanged = false;
    });
    final passwords = launchPasswords;
    final nativeFrame = ref.read(criticalRuntimeEventsProvider)?.currentScope;
    try {
      final parsedValues = jsonDecode(variables.text);
      final seconds = int.tryParse(timeout.text), parallel = int.tryParse(forks.text);
      if (runner == null ||
          playbook == null ||
          selectedHosts.isEmpty ||
          parsedValues is! Map<String, dynamic> ||
          seconds == null ||
          parallel == null) {
        throw FormatException(context.l10n.automationRunFields);
      }
      final AutomationJson values = parsedValues;
      // Capture all mutable composer values before the first asynchronous step.
      final attempt = _draftGeneration;
      final environmentContext = deploymentContext, environmentId = deploymentEnvironmentId;
      final environmentSelection = environmentContext?.policy == null
          ? null
          : environmentContext!.selection(environmentId!);
      final request = <String, dynamic>{
        'project_id': widget.project.id,
        'runner_id': runner,
        'playbook': playbook,
        'host_ids': selectedHosts.toList()..sort(),
        'mode': apply ? 'apply' : 'check',
        'timeout_seconds': seconds,
        'forks': parallel,
        'cancel_on_lock': cancelOnLock,
        'deployment_environment': ?environmentSelection,
        if (widget.retryOf != null) 'retry_of': widget.retryOf!['job_id'],
        if (vaultEnabled && vaultSelection != null) ...vaultSelection!,
        'parameters': {
          'extra_vars': values,
          'tags': splitTags(tags.text),
          'skip_tags': splitTags(skipTags.text),
          'diff': diff,
        },
      };
      if (!current || attempt != _draftGeneration) return;
      final connected = await ref.read(automationRunnerServiceProvider)!.connectRunner(runner!);
      if (!current || !deploymentAvailable) return;
      if (attempt != _draftGeneration) {
        setState(() => reviewChanged = true);
        return;
      }
      final probe = connected['probe'];
      final capabilities = probe is Map ? probe['capabilities'] : null;
      if (vaultEnabled && vaultSelection != null && (capabilities is! Map || capabilities['vault_job_v1'] != true)) {
        throw const AutomationRunnerUpdateRequired();
      }
      final AutomationJson result;
      if (passwords?.hasAny == true) {
        final nativeScope = nativeFrame?.scope;
        if (widget.service is! AutomationRunSecretService ||
            nativeScope == null ||
            nativeFrame?.streamUnavailable != false ||
            nativeFrame?.unsupported != false ||
            nativeScope.profileId != widget.scope.profileId?.toString()) {
          throw FormatException(launchPasswordsUnavailable);
        }
        if (passwords!.hasVault && (capabilities is! Map || capabilities['vault_ephemeral_v1'] != true)) {
          throw const AutomationRunnerUpdateRequired();
        }
        result = await (widget.service as AutomationRunSecretService).prepareRunWithSecrets(
          request,
          profileId: nativeScope.profileId,
          notificationScopeId: nativeScope.nonce,
          vaultPassword: passwords.vaultPassword,
          becomePassword: passwords.becomePassword,
          useProjectVaultPassword: passwords.useProjectVaultPassword,
        );
      } else {
        result = await widget.service.prepareRun(request);
      }
      if (current && (attempt != _draftGeneration || !deploymentAvailable)) {
        // A late review grants no UI approval for a newer composer draft.
        setState(() => reviewChanged = true);
        if (result['review_id'] case final String id) await widget.service.discardReview(id);
        return;
      }
      if (!current) {
        if (scopeValid && result['review_id'] is String) {
          await widget.service.discardReview(result['review_id'] as String);
        }
        return;
      }
      DeploymentEnvironment? environment;
      try {
        environment = checkedDeploymentReview(
          result['deployment_environment'],
          environmentContext,
          environmentId,
          (result['policy'] is Map ? (result['policy'] as Map)['production'] : null),
        );
      } catch (_) {
        if (result['review_id'] case final String id) await widget.service.discardReview(id);
        rethrow;
      }
      if (current) {
        setState(() {
          preparedEnvironment = environment;
          // Own the accepted Native snapshot; neither service caches nor the
          // mutable composer may subsequently change this read-only review.
          review = jsonDecode(jsonEncode(result)) as AutomationJson;
          diffReviewedDigest = null;
        });
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (current && formScroll.hasClients) formScroll.jumpTo(0);
        });
      }
    } catch (e) {
      if (current) setState(() => error = e);
    } finally {
      passwords?.wipe();
      if (identical(launchPasswords, passwords)) launchPasswords = null;
      if (current) setState(() => busy = false);
    }
  }

  Future<void> openServiceDelegation() async {
    if (busy || !current || _retireServiceDelegation != null || widget.service is! AutomationServiceDelegationService) {
      return;
    }
    if (hasLaunchPasswords) {
      setState(() => error = FormatException(context.l10n.automationRunPasswordsUnavailable));
      return;
    }
    final owner = widget.service as AutomationServiceDelegationService;
    final selected = ref.read(automationRunnersProvider).value?.runners.where((item) => item.id == runner).firstOrNull;
    final runtime = ref.read(criticalRuntimeEventsProvider), app = ref.read(appServicesProvider);
    final frame = runtime?.currentScope, native = frame?.scope;
    if (selected == null ||
        selected.readOnly ||
        selected.configuration?['kind'] != 'service_remote' ||
        selected.revision == null ||
        selected.revision! < 1 ||
        selected.configuration?['identity_id'] is! String ||
        native == null ||
        frame?.streamUnavailable != false ||
        frame?.unsupported != false ||
        native.profileId != widget.scope.profileId?.toString()) {
      setState(() => error = FormatException(context.l10n.automationServiceUnavailable));
      return;
    }
    final scope = widget.scope,
        project = widget.project,
        epoch = frame!.epoch,
        attempt = _draftGeneration,
        router = ref.read(routerProvider),
        jobService = ref.read(automationJobServiceProvider),
        runnerService = ref.read(automationRunnerServiceProvider);
    final uri = router.routeInformationProvider.value.uri;
    final originalNative = ServiceDelegationScope(native.profileId, native.nonce);
    ServiceDelegationIntent? intent;
    if (!vaultEnabled &&
        deploymentAvailable &&
        widget.retryOf == null &&
        playbook != null &&
        selectedHosts.isNotEmpty &&
        project.revision != null) {
      try {
        final values = jsonDecode(variables.text),
            seconds = int.tryParse(timeout.text),
            parallel = int.tryParse(forks.text);
        if (values is Map<String, dynamic> && seconds != null && parallel != null) {
          intent = ServiceDelegationIntent(
            scope: originalNative,
            runnerId: selected.id,
            identityId: selected.configuration!['identity_id'] as String,
            runnerRevision: selected.revision!,
            projectRevision: project.revision!,
            run: {
              'project_id': project.id, 'runner_id': selected.id,
              'playbook': playbook, 'host_ids': selectedHosts.toList()..sort(), 'mode': apply ? 'apply' : 'check',
              'timeout_seconds': seconds, 'forks': parallel,
              if (deploymentContext?.policy != null)
                'deployment_environment': deploymentContext!.selection(deploymentEnvironmentId!),
              // This is a separate service purpose, explicitly disclosed in its
              // own preview. The ordinary Run checkbox is not silently changed.
              'cancel_on_lock': false,
              'parameters': {
                'extra_vars': values,
                'tags': splitTags(tags.text),
                'skip_tags': splitTags(skipTags.text),
                'diff': diff,
              },
            },
          );
        }
      } on FormatException {
        /* Status/revocation needs no editable run intent. */
      }
    }
    final ids = {...selectedHosts, if (selected.configuration?['host_id'] case final String id) id};
    final capturedHosts = {
      for (final host in ref.read(hostsProvider).value ?? <Host>[])
        if (ids.contains(host.id)) host.id: host,
    };
    var retired = false, finished = false;
    final live = ValueNotifier<bool>(true);
    bool matches() =>
        current &&
        identical(widget.scope, scope) &&
        identical(widget.project, project) &&
        identical(widget.service, owner) &&
        _draftGeneration == attempt &&
        runner == selected.id &&
        identical(ref.read(appServicesProvider), app) &&
        identical(ref.read(automationJobServiceProvider), jobService) &&
        identical(ref.read(automationRunnerServiceProvider), runnerService) &&
        identical(ref.read(criticalRuntimeEventsProvider), runtime) &&
        runtime?.currentScope.epoch == epoch &&
        runtime?.currentScope.streamUnavailable == false &&
        native.same(runtime?.currentScope.scope) &&
        identical(
          ref.read(automationRunnersProvider).value?.runners.where((item) => item.id == selected.id).firstOrNull,
          selected,
        ) &&
        capturedHosts.entries.every(
          (entry) =>
              identical(ref.read(hostsProvider).value?.where((host) => host.id == entry.key).firstOrNull, entry.value),
        ) &&
        identical(ref.read(routerProvider), router) &&
        uri.path == AppRoutes.automation &&
        router.routeInformationProvider.value.uri == uri;
    void retire() {
      if (retired || finished) return;
      retired = true;
      scheduleMicrotask(() {
        if (!finished) live.value = false;
      });
    }

    void observe() {
      if (!matches()) retire();
    }

    bool original() {
      observe();
      return !retired && !finished;
    }

    final retirement = retire;
    _retireServiceDelegation = retirement;
    final subscriptions = <ProviderSubscription<dynamic>>[
      ref.listenManual(appServicesProvider, (_, _) => observe()),
      ref.listenManual(automationJobServiceProvider, (_, _) => observe()),
      ref.listenManual(automationRunnerServiceProvider, (_, _) => observe()),
      ref.listenManual(automationScopeProvider, (_, _) => observe()),
      ref.listenManual(automationRunnersProvider, (_, _) => observe()),
      ref.listenManual(hostsProvider, (_, _) => observe()),
      ref.listenManual(criticalRuntimeEventsProvider, (_, _) => observe()),
    ];
    final notices = runtime?.watchRuntimeNotices().listen(
      (_) => observe(),
      onError: (Object _, StackTrace _) => retire(),
      onDone: retire,
    );
    router.routeInformationProvider.addListener(observe);
    for (final controller in [variables, tags, skipTags, timeout, forks]) {
      controller.addListener(observe);
    }
    setState(() => busy = true);
    try {
      if (!original()) return;
      await showGlassDialog<void>(
        context,
        builder: (_) => DependencyOriginalGuard(
          original: live,
          builder: (_) => AutomationScopeGuard(
            scope: scope,
            builder: (_) => AutomationServiceDelegationDialog(
              service: owner,
              scope: originalNative,
              uiScope: scope,
              originalLive: live,
              intent: intent,
              runnerId: selected.id,
              runnerRevision: selected.revision!,
              runnerName: selected.name ?? selected.id,
              original: original,
            ),
          ),
        ),
      );
    } finally {
      finished = true;
      if (identical(_retireServiceDelegation, retirement)) _retireServiceDelegation = null;
      for (final subscription in subscriptions) {
        subscription.close();
      }
      await notices?.cancel();
      router.routeInformationProvider.removeListener(observe);
      for (final controller in [variables, tags, skipTags, timeout, forks]) {
        controller.removeListener(observe);
      }
      live.dispose();
      if (current) setState(() => busy = false);
    }
  }

  Future<void> configureRunner() async {
    if (busy || !current) return;
    final service = ref.read(automationRunnerServiceProvider);
    if (service == null) return;
    final catalog = ref.read(automationRunnersProvider).value;
    final selected = catalog?.readOnly == false
        ? catalog?.runners.where((item) => item.id == runner).firstOrNull
        : null;
    await showGlassDialog<void>(
      context,
      builder: (_) => AutomationScopeGuard(
        scope: widget.scope,
        builder: (_) => AutomationRunnersDialog(scope: widget.scope, service: service, initialRunner: selected),
      ),
    );
    if (!current) return;
    // A new validation always reconnects natively. Do not grant a review or
    // install anything on return; keep all unsaved composer inputs untouched.
    ref.invalidate(automationRunnersProvider);
    setState(() {
      error = null;
      runnerSettingsReturned = true;
    });
  }

  Future<void> confirm() async {
    if (busy || !current || (review?['validation'] as AutomationJson?)?['valid'] != true) return;
    if (review?['git_source'] is Map && diffReviewedDigest != (review?['revision'] as AutomationJson?)?['digest']) {
      return;
    }
    setState(() {
      busy = true;
      error = null;
    });
    try {
      try {} catch (_) {
        if (review?['review_id'] case final String id) await widget.service.discardReview(id);
        rethrow;
      }
      if (!current) return;
      final AutomationJson job;
      if (productionReview) {
        final challenge = AutomationOwnerConfirmation.fromReview(review!);
        if (challenge == null || widget.service is! AutomationProductionOwnerService) {
          throw const AppException(AppErrorCode.unsupported, 'production_owner_confirmation_unavailable');
        }
        final result = await confirmProductionOwner(widget.service as AutomationProductionOwnerService, challenge);
        if (result == null) {
          if (current && challenge.expired(DateTime.now().toUtc())) {
            throw const AppException(AppErrorCode.requestExpired, 'production_owner_review_expired');
          }
          return;
        }
        final failure = result.error;
        if (failure is Exception) throw failure;
        if (failure is Error) throw failure;
        if (failure != null) {
          throw const AppException(AppErrorCode.internal, 'production_owner_confirmation_failed');
        }
        job = result.job!;
      } else {
        job = await widget.service.confirmRun(review!['review_id'] as String);
      }
      confirmed = true;
      if (!mounted) return;
      if (current) {
        if (widget.embedded) {
          widget.onSubmitted?.call(job['job_id'] as String);
        } else {
          Navigator.pop(context, job['job_id'] as String);
        }
      }
    } catch (e) {
      if (current) {
        setState(() {
          error = e;
          review = null;
          diffReviewedDigest = null;
          reviewChanged = true;
        });
      }
    } finally {
      if (current) setState(() => busy = false);
    }
  }

  Future<AutomationOwnerConfirmationResult?> confirmProductionOwner(
    AutomationProductionOwnerService service,
    AutomationOwnerConfirmation challenge,
  ) async {
    final originalReview = review, app = ref.read(appServicesProvider);
    final jobService = ref.read(automationJobServiceProvider), runtime = ref.read(criticalRuntimeEventsProvider);
    final epoch = runtime?.currentScope.epoch, router = ref.read(routerProvider);
    final uri = router.routeInformationProvider.value.uri;
    final live = ValueNotifier<bool>(true);
    var retired = false, finished = false;
    bool matches() =>
        current &&
        identical(review, originalReview) &&
        identical(ref.read(appServicesProvider), app) &&
        identical(ref.read(automationJobServiceProvider), jobService) &&
        identical(ref.read(criticalRuntimeEventsProvider), runtime) &&
        runtime?.currentScope.epoch == epoch &&
        runtime?.currentScope.streamUnavailable != true &&
        identical(ref.read(routerProvider), router) &&
        uri.path == AppRoutes.automation &&
        router.routeInformationProvider.value.uri == uri;
    void retire() {
      if (retired || finished) return;
      retired = true;
      scheduleMicrotask(() {
        if (!finished) live.value = false;
      });
    }

    final retirement = retire;
    _retireOwnerConfirmation = retirement;
    bool original() {
      if (!matches()) retire();
      return !retired && !finished;
    }

    void observe() {
      if (!matches()) retire();
    }

    final subscriptions = <ProviderSubscription<dynamic>>[
      ref.listenManual(appServicesProvider, (_, _) => observe()),
      ref.listenManual(automationJobServiceProvider, (_, _) => observe()),
      ref.listenManual(automationScopeProvider, (_, _) => observe()),
      ref.listenManual(criticalRuntimeEventsProvider, (_, _) => observe()),
    ];
    final notices = runtime?.watchRuntimeNotices().listen(
      (_) => observe(),
      onError: (Object _, StackTrace _) => retire(),
      onDone: retire,
    );
    router.routeInformationProvider.addListener(observe);
    try {
      if (!original()) return null;
      return await showGlassDialog<AutomationOwnerConfirmationResult>(
        context,
        builder: (_) => DependencyOriginalGuard(
          original: live,
          builder: (_) => AutomationScopeGuard(
            scope: widget.scope,
            builder: (_) =>
                AutomationOwnerConfirmationDialog(service: service, challenge: challenge, original: original),
          ),
        ),
      );
    } finally {
      finished = true;
      if (identical(_retireOwnerConfirmation, retirement)) _retireOwnerConfirmation = null;
      for (final subscription in subscriptions) {
        subscription.close();
      }
      await notices?.cancel();
      router.routeInformationProvider.removeListener(observe);
      live.dispose();
    }
  }

  Future<void> editReview() async {
    if (busy || !current || review == null) return;
    final id = review!['review_id'] as String;
    setState(() {
      review = null;
      diffReviewedDigest = null;
      error = null;
      reviewChanged = true;
      busy = true;
    });
    try {
      await widget.service.discardReview(id);
    } catch (e) {
      if (current) setState(() => error = e);
    } finally {
      if (current) setState(() => busy = false);
    }
  }

  Future<void> reviewDiff() async {
    if (busy || widget.service is! AutomationDiffService) return;
    final digest = (review?['revision'] as AutomationJson?)?['digest'] as String?;
    if (digest == null) return;
    final accepted = await showDialog<bool>(
      context: context,
      builder: (_) => AutomationScopeGuard(
        scope: widget.scope,
        builder: (_) => AutomationDiffDialog(
          scope: widget.scope,
          service: widget.service as AutomationDiffService,
          projectId: widget.project.id,
          digest: digest,
        ),
      ),
    );
    if (current && accepted == true) setState(() => diffReviewedDigest = digest);
  }

  Widget field(String label, TextEditingController controller, {bool multiline = false}) => Padding(
    padding: const EdgeInsets.only(bottom: 12),
    child: TextField(
      controller: controller,
      minLines: multiline ? 3 : 1,
      maxLines: multiline ? 6 : 1,
      decoration: InputDecoration(labelText: label),
      autocorrect: false,
      enableSuggestions: false,
    ),
  );
  Widget fact(String label, Object? value) => Padding(
    padding: const EdgeInsets.symmetric(vertical: 5),
    child: Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(label, style: Theme.of(context).textTheme.labelMedium),
        SelectableText('$value'),
      ],
    ),
  );
  Widget validationCheck(AutomationJson check) {
    final l = context.l10n;
    final output = check['output'] as String? ?? '';
    final failed =
        check['exit_code'] != 0 ||
        check['cancelled'] == true ||
        check['timed_out'] == true ||
        check['truncated'] == true;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 8),
      child: ContentSurface(
        key: ValueKey('automation-validation-check-${check['name']}'),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text /* l10n-ignore: validation check identity and exit code */ (
              // l10n-ignore: native validation check identity and exit code
              '${check['name']} · ${check['exit_code']}',
              style: TextStyle(color: failed ? Theme.of(context).colorScheme.error : null),
            ),
            if (check['timed_out'] == true) Text(l.automationValidationTimedOut),
            if (check['cancelled'] == true) Text(l.automationStateCancelled),
            if (check['truncated'] == true) Text(l.automationOutputLimited),
            const SizedBox(height: 8),
            if (output.isEmpty)
              Text(l.automationValidationNoOutput)
            else
              ConstrainedBox(
                constraints: const BoxConstraints(maxHeight: 220),
                child: SingleChildScrollView(child: SelectableText(output)),
              ),
          ],
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final runners = ref.watch(automationRunnersProvider);
    final hosts = ref.watch(hostsProvider);
    final serviceRunner = runners.value?.runners.where((item) => item.id == runner).firstOrNull;
    final serviceSelected = serviceRunner?.configuration?['kind'] == 'service_remote';
    final visibleRunners = (runners.value?.runners ?? <AutomationRunnerProfile>[]).where(
      (item) => !item.readOnly && (!widget.scheduleOnly || item.configuration?['kind'] == 'service_remote'),
    );
    final mobileRunnerMissing = AppPlatform.isMobile && runners.hasValue && visibleRunners.isEmpty;
    final checked = review;
    final validation = checked?['validation'] as AutomationJson?;
    final capturedRevision = checked?['revision'] as AutomationJson?;
    final boundAliases = <String, String>{
      for (final entry in (inventoryConfiguration?['bindings'] as AutomationJson? ?? {}).entries)
        (entry.value as AutomationJson)['host_id'] as String: entry.key,
    };
    final externalOnly = inventoryConfiguration?['mode'] == 'external_only';
    return PopScope(
      canPop: widget.embedded || !busy,
      child: AutomationWorkspacePanel(
        embedded: widget.embedded,
        title: Text(checked == null ? l.automationRunSetup : l.automationReview),
        content: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            if (busy) const LinearProgressIndicator(),
            if (error != null)
              Padding(
                padding: const EdgeInsets.symmetric(vertical: 12),
                child: InfoBanner(
                  tone: BannerTone.danger,
                  message: error is AutomationRunnerUpdateRequired
                      ? l.automationRunnerUpdateForInputs
                      : error is FormatException
                      ? (error as FormatException).message
                      : errorMessage(l, error!),
                ),
              ),
            if (error is AutomationRunnerUpdateRequired && ref.watch(automationRunnerServiceProvider) != null)
              Align(
                alignment: Alignment.centerLeft,
                child: TextButton.icon(
                  key: const ValueKey('automation-configure-runner'),
                  onPressed: busy ? null : configureRunner,
                  icon: const Icon(Icons.settings_ethernet),
                  label: Text(l.automationConfigureRunner),
                ),
              ),
            if (reviewChanged)
              Padding(
                padding: const EdgeInsets.only(bottom: 12),
                child: InfoBanner(tone: BannerTone.warning, message: l.automationReviewChanged),
              ),
            if (runnerSettingsReturned)
              Padding(padding: const EdgeInsets.only(bottom: 12), child: Text(l.automationRunnerRevalidate)),
            Expanded(
              child: ListView(
                key: const ValueKey('automation-run-scroll'),
                controller: formScroll,
                children: [
                  if (widget.retryOf case final previous?) ...[
                    if (checked != null) InfoBanner(tone: BannerTone.warning, message: l.automationRepairPriorChanges),
                    ExpansionTile(
                      key: const ValueKey('automation-retry-prior-results'),
                      initiallyExpanded: false,
                      title: Text(l.automationRepairPriorResults),
                      children: [
                        AutomationJobResult(job: previous),
                        if (widget.service is AutomationEventService)
                          AutomationTaskResults(
                            service: widget.service as AutomationEventService,
                            job: previous,
                            scopeIdentity: widget.scope,
                            isCurrent: () => current,
                            active: !busy,
                          ),
                      ],
                    ),
                  ],
                  if (checked == null) ...[
                    Text(widget.project.name ?? widget.project.id),
                    if (widget.retryOf case final previous?) ...[
                      const SizedBox(height: 8),
                      Text(l.automationRetryWarning),
                      fact(l.automationRetryOrigin, previous['job_id']),
                    ],
                    const SizedBox(height: 12),
                    GlassSelect<String?>(
                      key: const ValueKey('automation-run-runner'),
                      value: runner,
                      expand: true,
                      semanticLabel: l.automationChooseRunner,
                      onChanged: busy ? null : (value) => setState(() => runner = value),
                      items: [
                        GlassSelectItem(value: null, label: l.automationChooseRunner),
                        for (final item in visibleRunners) GlassSelectItem(value: item.id, label: item.name ?? item.id),
                      ],
                    ),
                    if (runners.hasError) Text(errorMessage(l, runners.error!)),
                    if (widget.scheduleOnly && !runners.isLoading && visibleRunners.isEmpty)
                      InfoBanner(tone: BannerTone.info, message: l.automationSchedulesRunnerRequired),
                    if (mobileRunnerMissing) ...[
                      if (!widget.scheduleOnly)
                        InfoBanner(tone: BannerTone.info, message: l.automationMobileRunnerRequired),
                      if (ref.watch(automationRunnerServiceProvider) != null)
                        Align(
                          alignment: AlignmentDirectional.centerStart,
                          child: TextButton.icon(
                            key: const ValueKey('automation-add-mobile-runner'),
                            onPressed: busy ? null : configureRunner,
                            icon: const Icon(Icons.settings_ethernet),
                            label: Text(l.automationConfigureRunner),
                          ),
                        ),
                    ],
                    if (serviceSelected)
                      InfoBanner(
                        tone: BannerTone.info,
                        message: widget.service is AutomationServiceDelegationService
                            ? l.automationServiceUseDelegation
                            : l.automationServiceUnavailable,
                      ),
                    const SizedBox(height: 12),
                    GlassSelect<String?>(
                      value: playbook,
                      expand: true,
                      semanticLabel: l.automationPlaybook,
                      onChanged: busy ? null : (value) => setState(() => playbook = value),
                      items: [
                        GlassSelectItem(value: null, label: l.automationPlaybook),
                        for (final path in widget.playbooks) GlassSelectItem(value: path, label: path),
                      ],
                    ),
                    const SizedBox(height: 12),
                    if (widget.service case final AutomationProjectCredentialsService credentials)
                      AutomationDeploymentEnvironmentSelection(
                        scope: widget.scope,
                        project: widget.project,
                        service: credentials,
                        busy: busy,
                        onChanged: (ready, context, id, original) => setState(() {
                          deploymentReady = ready;
                          deploymentContext = context;
                          deploymentEnvironmentId = id;
                          deploymentCurrent = original;
                          _draftGeneration++;
                        }),
                      ),
                    const SizedBox(height: 10),
                    ContentSurface(
                      kind: ContentSurfaceKind.inset,
                      padding: const EdgeInsets.all(10),
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.stretch,
                        children: [
                          Text(l.automationTargets, style: Theme.of(context).textTheme.titleSmall),
                          if (inventoryLoading) const LinearProgressIndicator(),
                          if (inventoryError != null)
                            Text(
                              inventoryError is FormatException
                                  ? (inventoryError as FormatException).message
                                  : errorMessage(l, inventoryError!),
                            ),
                          if (inventoryConfiguration != null)
                            Text(externalOnly ? l.automationInventoryExternalOnly : l.automationInventoryMixed),
                          if (hosts.isLoading) const LinearProgressIndicator(),
                          if (hosts.hasError) Text(errorMessage(l, hosts.error!)),
                          const SizedBox(height: 8),
                          ConstrainedBox(
                            constraints: const BoxConstraints(maxHeight: 216),
                            child: Scrollbar(
                              controller: targetsScroll,
                              thumbVisibility: true,
                              trackVisibility: true,
                              child: ListView(
                                controller: targetsScroll,
                                key: const ValueKey('automation-run-targets'),
                                primary: false,
                                shrinkWrap: true,
                                padding: const EdgeInsetsDirectional.only(end: 12),
                                children: [
                                  for (final host in hosts.value ?? <Host>[])
                                    if (host.protocol == HostProtocol.ssh &&
                                        !inventoryLoading &&
                                        inventoryError == null &&
                                        (!externalOnly || boundAliases.containsKey(host.id.value)) &&
                                        (externalOnly ||
                                            inventorySavedHosts == null ||
                                            inventorySavedHosts!.contains(host.id.value) ||
                                            boundAliases.containsKey(host.id.value)) &&
                                        (widget.retryOf == null ||
                                            (widget.retryOf!['host_ids'] as List).contains(host.id.value)))
                                      ConstrainedBox(
                                        constraints: const BoxConstraints(minHeight: 52),
                                        child: CheckboxListTile(
                                          key: ValueKey('automation-target-${host.id}'),
                                          dense: true,
                                          value: selectedHosts.contains(host.id.value),
                                          title: Text(
                                            boundAliases.containsKey(host.id.value)
                                                ? '${boundAliases[host.id.value]} · ${host.name}'
                                                : host.name,
                                          ),
                                          subtitle: Text(host.address),
                                          secondary: IconButton(
                                            key: ValueKey('automation-target-settings-${host.id.value}'),
                                            tooltip: l.automationTargetSettings,
                                            icon: const Icon(Icons.tune_outlined),
                                            onPressed: busy ? null : () => targetSettings(host),
                                          ),
                                          onChanged: busy
                                              ? null
                                              : (value) => setState(() {
                                                  if (value == true) {
                                                    selectedHosts.add(host.id.value);
                                                  } else {
                                                    selectedHosts.remove(host.id.value);
                                                  }
                                                }),
                                        ),
                                      ),
                                ],
                              ),
                            ),
                          ),
                        ],
                      ),
                    ),
                    const SizedBox(height: 16),
                    const Divider(),
                    GlassSelect<bool>(
                      value: apply,
                      expand: true,
                      semanticLabel: l.automationRunSetup,
                      onChanged: busy ? null : (value) => setState(() => apply = value),
                      items: [
                        GlassSelectItem(value: false, label: l.automationCheckMode),
                        GlassSelectItem(value: true, label: l.automationApplyMode),
                      ],
                    ),
                    const SizedBox(height: 8),
                    Text(l.automationCheckWarning),
                    const SizedBox(height: 12),
                    field(l.automationVariables, variables, multiline: true),
                    field(l.automationTags, tags),
                    field(l.automationSkipTags, skipTags),
                    field(l.automationTimeout, timeout),
                    field(l.automationForks, forks),
                    if (!widget.scheduleOnly && widget.service is AutomationVaultSelectionService)
                      AutomationVaultSelection(
                        scope: widget.scope,
                        projectId: widget.project.id,
                        service: widget.service as AutomationVaultSelectionService,
                        busy: busy || launchPasswords?.hasVault == true,
                        manualPassword: widget.service is AutomationRunSecretService ? manualVaultPassword : null,
                        onManualChanged: () => setState(() {
                          if (hasManualVaultPassword) cancelOnLock = true;
                          _draftGeneration++;
                        }),
                        onChanged: (enabled, selected) => setState(() {
                          vaultEnabled = enabled;
                          vaultSelection = selected;
                          _draftGeneration++;
                          if (enabled) diff = false;
                        }),
                      ),
                    if (!widget.scheduleOnly && widget.service is AutomationRunSecretService) ...[
                      const SizedBox(height: 8),
                      OutlinedButton.icon(
                        key: const ValueKey('automation-run-passwords'),
                        onPressed: busy || serviceSelected ? null : passwordsForLaunch,
                        icon: const Icon(Icons.password),
                        label: Text(l.automationRunPasswords),
                      ),
                      if (launchPasswords?.hasAny == true) Text(l.automationRunPasswordsConfigured),
                    ],
                    if (!widget.scheduleOnly)
                      CheckboxListTile(
                        value: cancelOnLock,
                        title: Text(l.automationCancelOnLock),
                        onChanged: busy || hasLaunchPasswords
                            ? null
                            : (value) => setState(() => cancelOnLock = value ?? true),
                      ),
                    CheckboxListTile(
                      value: diff,
                      title: Text(l.automationDiff),
                      onChanged: busy || vaultEnabled || launchPasswords?.hasVault == true
                          ? null
                          : (value) => setState(() => diff = value ?? false),
                    ),
                  ] else ...[
                    Text(l.automationReviewHint),
                    if (validation?['valid'] != true)
                      Text(l.automationValidationFailed, style: TextStyle(color: Theme.of(context).colorScheme.error)),
                    if (validation?['valid'] != true)
                      for (final check in (validation?['checks'] as List? ?? []).cast<AutomationJson>())
                        validationCheck(check),
                    if (preparedEnvironment case final DeploymentEnvironment environment) ...[
                      Text('${l.automationDeploymentTitle}: ${environment.name}'),
                      Text(
                        environment.production
                            ? l.automationDeploymentProduction
                            : l.automationDeploymentProductionHint,
                      ),
                      ExpansionTile(
                        title: Text(l.automationDeploymentFrozenReferences),
                        children: [
                          for (final reference in environment.references)
                            SelectableText('${reference.hostId} → ${reference.credentialId}'),
                        ],
                      ),
                    ],
                    const SizedBox(height: 12),
                    AutomationReviewCards(
                      review: checked,
                      runnerName:
                          runners.value?.runners.where((r) => r.id == checked['runner_id']).firstOrNull?.name ??
                          '${checked['runner_id']}',
                      diffReviewed: diffReviewedDigest == capturedRevision?['digest'],
                      onDiff: busy || widget.service is! AutomationDiffService ? null : reviewDiff,
                    ),
                    const SizedBox(height: 12),
                    ExpansionTile(
                      key: const ValueKey('automation-review-technical'),
                      title: Text(l.automationReviewDetails),
                      children: [
                        fact(l.automationEnvironment, validation?['environment_digest']),
                        if (checked['git_source'] case final AutomationJson source)
                          fact(l.automationGitReference, '${source['reference']} · ${source['commit']}'),
                        if (checked['retry_origin'] case final AutomationJson previous)
                          fact(l.automationRetryOrigin, previous['job_id']),
                      ],
                    ),
                    ExpansionTile(
                      key: const ValueKey('automation-review-variables'),
                      title: Text(l.automationShowVariables),
                      children: [AutomationReviewVariables(parameters: checked['parameters'])],
                    ),
                    if (validation?['valid'] == true)
                      for (final check in (validation?['checks'] as List? ?? []).cast<AutomationJson>())
                        validationCheck(check),
                  ],
                ],
              ),
            ),
          ],
        ),
        actions: [
          TextButton(
            onPressed: busy
                ? null
                : () {
                    if (widget.embedded) {
                      widget.onClose?.call();
                    } else {
                      Navigator.pop(context);
                    }
                  },
            child: Text(l.commonCancel),
          ),
          if (checked != null)
            TextButton(
              key: const ValueKey('automation-review-edit'),
              onPressed: busy ? null : editReview,
              child: Text(l.commonBack),
            ),
          if (checked == null && (serviceSelected || widget.scheduleOnly))
            FilledButton(
              key: const ValueKey('automation-service-open'),
              onPressed: busy || !serviceSelected || widget.service is! AutomationServiceDelegationService
                  ? null
                  : openServiceDelegation,
              child: Text(widget.scheduleOnly ? l.automationSchedulesConfigure : l.automationServiceDelegate),
            ),
          if (checked == null && !widget.scheduleOnly)
            FilledButton(
              key: const ValueKey('automation-validate-run'),
              onPressed:
                  busy ||
                      !deploymentAvailable ||
                      inventoryLoading ||
                      inventoryError != null ||
                      needsVaultPassword ||
                      mobileRunnerMissing
                  ? null
                  : serviceSelected
                  ? null
                  : validate,
              child: Text(l.automationValidate),
            ),
          if (checked != null && validation?['valid'] == true)
            FilledButton(
              key: const ValueKey('automation-confirm-run'),
              onPressed: busy || checked['git_source'] is Map && diffReviewedDigest != capturedRevision?['digest']
                  ? null
                  : confirm,
              child: Text(productionReview ? l.automationOwnerReviewConfirm : l.automationConfirm),
            ),
        ],
      ),
    );
  }
}
