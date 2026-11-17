import 'dart:async';
import 'dart:math' as math;

import 'package:consolecrypt/app/critical_notice_owner.dart';
import 'package:consolecrypt/app/critical_notice_providers.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/automation/agent_installer_project_focus.dart';
import 'package:consolecrypt/automation/ansible_code_editor.dart';
import 'package:consolecrypt/automation/automation_ai_dialog.dart';
import 'package:consolecrypt/automation/automation_ai_draft.dart';
import 'package:consolecrypt/automation/automation_ai_draft_errors.dart';
import 'package:consolecrypt/automation/automation_ansible_config.dart';
import 'package:consolecrypt/automation/automation_awx_entry.dart';
import 'package:consolecrypt/automation/automation_cadvisor_review.dart';
import 'package:consolecrypt/automation/automation_dependency_dialog.dart';
import 'package:consolecrypt/automation/automation_dependency_models.dart';
import 'package:consolecrypt/automation/automation_execution_console.dart';
import 'package:consolecrypt/automation/automation_exporter_template.dart';
import 'package:consolecrypt/automation/automation_git_dialog.dart';
import 'package:consolecrypt/automation/automation_groups_dialog.dart';
import 'package:consolecrypt/automation/automation_inventory_dialog.dart';
import 'package:consolecrypt/automation/automation_inventory_export.dart';
import 'package:consolecrypt/automation/automation_inventory_export_dialog.dart';
import 'package:consolecrypt/automation/automation_jobs_dialog.dart';
import 'package:consolecrypt/automation/automation_portable_host.dart';
import 'package:consolecrypt/automation/automation_portable_import_host.dart';
import 'package:consolecrypt/automation/automation_portable_import_l10n.dart';
import 'package:consolecrypt/automation/automation_portable_import_session.dart';
import 'package:consolecrypt/automation/automation_portable_l10n.dart';
import 'package:consolecrypt/automation/automation_portable_service.dart';
import 'package:consolecrypt/automation/automation_project_credentials.dart';
import 'package:consolecrypt/automation/automation_project_credentials_dialog.dart';
import 'package:consolecrypt/automation/automation_project_metadata.dart';
import 'package:consolecrypt/automation/automation_project_metadata_dialog.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_run_dialog.dart';
import 'package:consolecrypt/automation/automation_runners_dialog.dart';
import 'package:consolecrypt/automation/automation_scope_guard.dart';
import 'package:consolecrypt/automation/automation_secret_view.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_yaml_editor.dart';
import 'package:consolecrypt/kubernetes/kube_yaml_syntax.dart';
import 'package:consolecrypt/monitoring/job_context_focus.dart';
import 'package:consolecrypt/monitoring/job_context_navigation.dart';
import 'package:consolecrypt/monitoring/job_context_project_guard.dart';
import 'package:consolecrypt/monitoring/job_context_receiver.dart';
import 'package:consolecrypt/monitoring/monitoring_host_context.dart';
import 'package:consolecrypt/monitoring/monitoring_providers.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class AutomationScreen extends ConsumerWidget {
  const AutomationScreen({super.key, this.contextFocus, this.agentProjectFocus, this.cadvisorFocus});
  final AgentCadvisorComposerFocus? cadvisorFocus;
  final AgentInstallerProjectFocus? agentProjectFocus;
  final JobContextFocus? contextFocus;
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final scope = ref.watch(automationScopeProvider);
    final service = ref.watch(automationServiceProvider);
    if (!scope.unlocked) return const SizedBox.shrink();
    if (service == null) return Center(child: Text(context.l10n.automationUnavailable));
    // Replacing this subtree disposes plaintext editors and invalidates pending
    // reads on every profile/unlock generation, including a rapid lock/unlock.
    return _ProjectWorkspace(
      key: ObjectKey(scope),
      scope: scope,
      service: service,
      contextFocus: contextFocus,
      agentProjectFocus: agentProjectFocus,
      cadvisorFocus: cadvisorFocus,
    );
  }
}

class _ProjectWorkspace extends ConsumerStatefulWidget {
  const _ProjectWorkspace({
    super.key,
    required this.scope,
    required this.service,
    this.contextFocus,
    this.agentProjectFocus,
    this.cadvisorFocus,
  });
  final AgentCadvisorComposerFocus? cadvisorFocus;
  final AgentInstallerProjectFocus? agentProjectFocus;
  final JobContextFocus? contextFocus;
  final AutomationScope scope;
  final AutomationService service;
  @override
  ConsumerState<_ProjectWorkspace> createState() => _ProjectWorkspaceState();
}

class _ProjectWorkspaceState extends ConsumerState<_ProjectWorkspace> {
  final editor = KubeYamlController();
  AutomationProject? project;
  AutomationProjectRevision? revision;
  AutomationProjectFile? file;
  AutomationAiDraft? aiDraft;
  AutomationJson? repairOrigin;
  Object? repairApp;
  AutomationService? repairService;
  AutomationJson? get repairForCurrent {
    final row = repairOrigin, manifest = repairOrigin?['manifest'];
    return row != null &&
            identical(repairApp, ref.read(appServicesProvider)) &&
            identical(repairService, widget.service) &&
            row['project_id'] == project?.id &&
            manifest is Map &&
            manifest['playbook'] == (file?.path ?? newPath)
        ? row
        : null;
  }

  String? newPath;
  Object? error;
  bool busy = false, dirty = false;
  bool requirementsSavedPending = false;
  double sidebarWidth = 250;
  int request = 0;
  VoidCallback? _retireExternalEditor;
  int workspacePage = 0;
  Widget? runComposer;
  int jobsReviewEpoch = 0;
  String? executionJob;
  bool consoleCollapsed = false;
  double consoleHeight = 260;
  bool get current => mounted && identical(ref.read(automationScopeProvider), widget.scope);
  AgentCadvisorComposerFocus? _handledCadvisorFocus;
  void _scheduleCadvisorFocus() => WidgetsBinding.instance.addPostFrameCallback((_) {
    if (mounted) unawaited(_consumeCadvisorFocus());
  });
  bool _cadvisorCurrent(AgentCadvisorComposerFocus focus) =>
      current &&
      identical(widget.cadvisorFocus, focus) &&
      focus.current(widget.scope, widget.service) &&
      ref.read(routerProvider).routeInformationProvider.value.uri.path == AppRoutes.automation;
  Future<void> _consumeCadvisorFocus() async {
    final focus = widget.cadvisorFocus;
    if (focus == null || identical(focus, _handledCadvisorFocus)) return;
    _handledCadvisorFocus = focus;
    if (!focus.take(widget.scope, widget.service) || !_cadvisorCurrent(focus)) {
      if (current) setState(() => error = context.l10n.automationCadvisorInstallInvalid);
      return;
    }
    if (busy || widget.service is! AutomationJobService) {
      setState(() => error = context.l10n.automationCadvisorInstallInvalid);
      return;
    }
    await _createCadvisorTemplate(install: true, cadvisorFocus: focus);
  }

  AgentInstallerProjectFocus? _handledAgentFocus, _acceptedAgentFocus;
  int _agentFocusGeneration = 0;
  bool _agentProjectUnavailable = false;
  void _scheduleAgentFocus() => WidgetsBinding.instance.addPostFrameCallback((_) {
    if (mounted) unawaited(_consumeAgentFocus());
  });
  bool _agentCurrent(AgentInstallerProjectFocus focus) =>
      current &&
      identical(widget.agentProjectFocus, focus) &&
      focus.current(widget.scope, widget.service) &&
      ref.read(routerProvider).routeInformationProvider.value.uri.path == AppRoutes.automation;
  Future<void> _consumeAgentFocus() async {
    final focus = widget.agentProjectFocus;
    if (focus == null || identical(focus, _handledAgentFocus)) return;
    _handledAgentFocus = focus;
    if (!focus.take(widget.scope, widget.service)) return;
    final generation = ++_agentFocusGeneration;
    bool valid() => generation == _agentFocusGeneration && _agentCurrent(focus);
    if (!valid()) return;
    setState(() {
      _acceptedAgentFocus = focus;
      _agentProjectUnavailable = false;
    });
    if (busy) {
      setState(() => _agentProjectUnavailable = true);
      return;
    }
    try {
      final catalog = await widget.service.projects();
      if (!valid()) return;
      final selected = catalog.projects.where((p) => p.id == focus.projectId).firstOrNull;
      if (catalog.readOnly || selected == null || selected.readOnly || selected.revision == null) {
        setState(() => _agentProjectUnavailable = true);
        return;
      }
      await _select(selected, originalCurrent: valid);
      if (valid() && project?.id == focus.projectId) _showProjects();
    } catch (e) {
      if (valid()) setState(() => error = e);
    }
  }

  Future<void> _returnToAgentHost(AgentInstallerProjectFocus focus) async {
    bool original() => _agentCurrent(focus) && identical(_acceptedAgentFocus, focus);
    if (busy || !original() || !await _leaveFile(originalCurrent: original) || !original()) return;
    ref.read(routerProvider).go(focus.returnUri.toString());
  }

  JobContextFocus? _handledContextFocus, _acceptedProjectFocus;
  AutomationProject? _focusedProject;
  bool _contextError = false;
  int _contextGeneration = 0;
  bool _contextCurrent(JobContextFocus focus, int epoch) =>
      current &&
      epoch == _contextGeneration &&
      identical(widget.contextFocus, focus) &&
      ref.read(routerProvider).routeInformationProvider.value.uri.path == AppRoutes.automation &&
      ref.read(monitoringScopeProvider).current &&
      identical(ref.read(monitoringSessionProvider)?.correlationScope, focus.scope);
  void _scheduleContextFocus() => WidgetsBinding.instance.addPostFrameCallback((_) {
    if (mounted) unawaited(_consumeContextFocus());
  });
  @override
  void didUpdateWidget(_ProjectWorkspace oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!identical(widget.cadvisorFocus, oldWidget.cadvisorFocus)) _scheduleCadvisorFocus();
    if (!identical(widget.agentProjectFocus, oldWidget.agentProjectFocus)) {
      _agentFocusGeneration++;
      _acceptedAgentFocus = null;
      _scheduleAgentFocus();
    }
    if (!identical(widget.contextFocus, oldWidget.contextFocus)) {
      _contextGeneration++;
      _scheduleContextFocus();
    }
  }

  Future<AutomationProject?> _currentContextProject(
    JobContextFocus focus,
    bool Function() valid, {
    AutomationProjectRevision? files,
  }) async {
    final service = widget.service;
    if (!valid() || service is! AutomationJobService) return null;
    final jobs = await (service as AutomationJobService).jobs();
    if (!valid() || !identical(service, widget.service)) return null;
    final catalog = await service.projects();
    if (!valid() || !identical(service, widget.service)) return null;
    return jobContextProjectFromCurrentMetadata(focus, catalog, jobs, DateTime.now().toUtc(), files: files);
  }

  Future<bool> _recheckContext(JobContextFocus focus, bool Function() valid) async {
    final session = ref.read(monitoringSessionProvider), service = ref.read(monitoringSessionProvider)?.service;
    if (!valid() || !identical(session?.correlationScope, focus.scope) || service is! MonitoringHostContextReader) {
      return false;
    }
    final reader = service as MonitoringHostContextReader;
    if (!reader.hostContextSupported) return false;
    final checked = await reader.recheckHostContext(focus.metadata);
    return valid() &&
        identical(ref.read(monitoringSessionProvider), session) &&
        sameJobContextMetadata(focus.metadata, checked);
  }

  Future<void> _consumeContextFocus() async {
    final focus = widget.contextFocus;
    if (focus == null || identical(focus, _handledContextFocus)) return;
    _handledContextFocus = focus;
    final epoch = ++_contextGeneration, before = request, service = widget.service;
    bool valid() => _contextCurrent(focus, epoch) && request == before && identical(widget.service, service);
    if (!valid()) return;
    if (busy) {
      setState(() => _contextError = true);
      return;
    }
    try {
      final ready = await receiveJobContextFocus(
        ref,
        focus,
        destination: JobContextDestination.ansibleProject,
        receiverCurrent: valid,
      );
      if (ready == null || !valid()) return;
      final selected = await _currentContextProject(ready, valid);
      if (!valid()) return;
      if (selected == null || !await _recheckContext(ready, valid) || !valid()) {
        if (valid()) setState(() => _contextError = true);
        return;
      }
      setState(() {
        _acceptedProjectFocus = ready;
        _focusedProject = selected;
        _contextError = false;
        // IndexedStack retains editor and composer. Do not call _showProjects.
        workspacePage = 0;
      });
    } catch (_) {
      if (valid()) setState(() => _contextError = true);
    }
  }

  Future<void> _openContextProject(JobContextFocus focus) async {
    final epoch = _contextGeneration, service = widget.service;
    bool original() =>
        _contextCurrent(focus, epoch) && identical(_acceptedProjectFocus, focus) && identical(widget.service, service);
    if (busy || !original() || !await _leaveFile() || !original()) return;
    final token = ++request;
    bool valid() => original() && token == request;
    setState(() {
      busy = true;
      _contextError = false;
    });
    try {
      final selected = await _currentContextProject(focus, valid);
      if (selected == null || !valid() || !await _recheckContext(focus, valid) || !valid()) {
        if (valid()) setState(() => _contextError = true);
        return;
      }
      final files = await service.files(selected.id);
      if (!valid()) return;
      final finalProject = await _currentContextProject(focus, valid, files: files);
      if (finalProject == null || !await _recheckContext(focus, valid) || !valid()) {
        if (valid()) setState(() => _contextError = true);
        return;
      }
      setState(() {
        project = finalProject;
        revision = files;
        file = null;
        aiDraft = null;
        repairOrigin = null;
        requirementsSavedPending = false;
        newPath = null;
        editor.clear();
        dirty = false;
        runComposer = null;
        jobsReviewEpoch++;
        workspacePage = 0;
        _acceptedProjectFocus = null;
        _focusedProject = null;
        busy = false;
      });
    } catch (_) {
      if (valid()) setState(() => _contextError = true);
    } finally {
      if (current && token == request) setState(() => busy = false);
    }
  }

  // Project source may contain secrets not recognized by the display mask.
  // Keep every editor selection on the protected, expiring clipboard path.
  Future<bool> _copyEditorSelection(String text) async {
    final epoch = request;
    bool isCurrent() => current && request == epoch;
    if (!isCurrent()) return false;
    try {
      await ref.read(secureClipboardProvider).copySecret(text, isCurrent: isCurrent);
      return isCurrent();
    } catch (_) {
      if (mounted && isCurrent()) showSnack(context, context.l10n.clipboardSecureCopyFailed, error: true);
      return false;
    }
  }

  @override
  void initState() {
    super.initState();
    editor.addListener(_edited);
    _scheduleContextFocus();
    _scheduleAgentFocus();
    _scheduleCadvisorFocus();
    ref.listenManual(criticalNoticeOpenIntentProvider, (_, intent) {
      if (intent?.target is! CriticalJobTarget) return;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!current || ref.read(automationJobServiceProvider) == null) return;
        final target = ref.read(criticalNoticeOpenIntentProvider.notifier).take(intent!);
        if (target is CriticalJobTarget) _openExecution(target.jobId);
      });
    }, fireImmediately: true);
  }

  void _edited() {
    if (mounted) setState(() => dirty = editor.text != (file?.text ?? '') || aiDraft?.hasRequirements == true);
  }

  @override
  void dispose() {
    _retireExternalEditor?.call();
    _retireExternalEditor = null;
    request++;
    _contextGeneration++;
    editor.removeListener(_edited);
    editor.dispose();
    super.dispose();
  }

  Future<T?> _dialog<T>(WidgetBuilder builder) => showGlassDialog<T>(
    context,
    builder: (_) => AutomationScopeGuard(scope: widget.scope, builder: builder),
  );

  Future<String?> _name(String title, {String initial = ''}) async {
    return _dialog<String>((context) => _NameDialog(title: title, initial: initial));
  }

  Future<bool> _leaveFile({
    bool Function()? originalCurrent,
    ValueListenable<bool>? originalLive,
    void Function(int)? ownSaveRequest,
    String? discardLabel,
  }) async {
    if (!dirty) return originalCurrent?.call() ?? true;
    final choice = await _dialog<String>(
      (context) => DependencyOriginalGuard(
        original: originalLive,
        builder: (context) => AlertDialog(
          title: Text(context.l10n.automationUnsaved),
          content: Text(file?.path ?? newPath ?? ''),
          actions: [
            TextButton(onPressed: () => Navigator.pop(context, 'cancel'), child: Text(context.l10n.commonCancel)),
            TextButton(
              onPressed: () => Navigator.pop(context, 'discard'),
              child: Text(discardLabel ?? context.l10n.automationDiscard),
            ),
            TextButton(onPressed: () => Navigator.pop(context, 'save'), child: Text(context.l10n.commonSave)),
          ],
        ),
      ),
    );
    if (!current || originalCurrent?.call() == false) return false;
    if (choice == 'discard') return true;
    if (choice != 'save') return false;
    ownSaveRequest?.call(request + 1); // Only this immediately invoked _save owns the counter transition.
    final saved = await _save();
    return originalCurrent == null ? saved : saved && current && originalCurrent();
  }

  Future<void> _run(
    AutomationJobService service, {
    AutomationJson? initialExtraVars,
    AutomationCadvisorTemplateBinding? cadvisorTemplate,
    AgentCadvisorComposerFocus? cadvisorFocus,
  }) async {
    if (busy || project == null) return;
    if (dirty && !await _save()) return;
    if (!current || cadvisorFocus != null && !_cadvisorCurrent(cadvisorFocus)) return;
    final paths = [
      for (final entry in revision?.entries ?? <AutomationFileEntry>[])
        if (!entry.directory &&
            RegExp(r'\.(yml|yaml)$', caseSensitive: false).hasMatch(entry.path) &&
            !['roles', 'templates', 'files', 'host_vars', 'group_vars'].contains(entry.path.split('/').first))
          entry.path,
    ];
    setState(() {
      workspacePage = 2;
      runComposer = AutomationRunDialog(
        scope: widget.scope,
        service: service,
        project: project!,
        playbooks: paths,
        initialPlaybook: cadvisorTemplate == null ? file?.path : 'playbook.yml',
        initialExtraVars: initialExtraVars,
        cadvisorTemplate: cadvisorTemplate,
        cadvisorFocus: cadvisorFocus,
        cadvisorOriginal: cadvisorFocus == null ? null : () => _cadvisorCurrent(cadvisorFocus),
        retryOf: repairForCurrent,
        embedded: true,
        onClose: _showProjects,
        onSubmitted: _acceptedJob,
      );
    });
  }

  void _acceptedJob(String id) {
    if (!current) return;
    setState(() {
      runComposer = null;
      repairOrigin = null;
      executionJob = id;
      consoleCollapsed = false;
      if (workspacePage == 2 || workspacePage == 1) workspacePage = 0;
    });
  }

  void _openExecution(String id) => setState(() {
    executionJob = id;
    consoleCollapsed = false;
    workspacePage = 0;
  });

  void _showProjects() => setState(() {
    workspacePage = 0;
    // Returning to editable files invalidates every prepared review, including retries.
    runComposer = null;
    jobsReviewEpoch++;
  });

  Future<void> _portablePackage({required bool inspect, bool createProject = false}) async {
    final service = widget.service;
    if (!current ||
        busy ||
        service is! AutomationPortableGatewayCapability ||
        createProject && service is! AutomationPortableImportReviewCapability) {
      return;
    }
    final originalScope = widget.scope, originalServices = ref.read(appServicesProvider);
    final originalFiles = ref.read(fileDialogServiceProvider), runtime = ref.read(criticalRuntimeEventsProvider);
    final originalProfile = ref.read(activeProfileProvider)?.id.value;
    final router = ref.read(routerProvider), route = router.routeInformationProvider.value.uri;
    final epoch = runtime?.currentScope.epoch, intended = project, intendedPath = file?.path ?? newPath;
    if (runtime == null ||
        originalProfile == null ||
        route.path != AppRoutes.automation ||
        !inspect && (intended == null || intended.readOnly || intended.revision == null)) {
      return;
    }
    var expectedRequest = request, retired = false, finished = false;
    String? editorSnapshot;
    final live = ValueNotifier<bool>(true);
    // Metadata-only UI lifetime, captured BEFORE dirty prompt's first await.
    // Native alone supplies original scope/issued registration and file authority.
    bool matches() =>
        mounted &&
        current &&
        identical(widget.scope, originalScope) &&
        identical(widget.service, service) &&
        identical(ref.read(automationServiceProvider), service) &&
        identical(ref.read(appServicesProvider), originalServices) &&
        identical(ref.read(fileDialogServiceProvider), originalFiles) &&
        identical(ref.read(criticalRuntimeEventsProvider), runtime) &&
        runtime.currentScope.epoch == epoch &&
        !runtime.currentScope.streamUnavailable &&
        ref.read(activeProfileProvider)?.id.value == originalProfile &&
        identical(ref.read(routerProvider), router) &&
        router.routeInformationProvider.value.uri == route &&
        request == expectedRequest &&
        (inspect && !createProject ||
            (identical(project, intended) &&
                (file?.path ?? newPath) == intendedPath &&
                (editorSnapshot == null || editor.text == editorSnapshot)));
    void retire() {
      if (retired || finished) return;
      retired = true;
      scheduleMicrotask(() {
        if (!finished) live.value = false;
      });
    }

    bool originalCurrent() {
      if (retired || finished) return false;
      if (!matches()) {
        retire();
        return false;
      }
      return true;
    }

    void observe() {
      if (!matches()) retire();
    }

    final subscriptions = <ProviderSubscription<dynamic>>[
      ref.listenManual(appServicesProvider, (_, _) => observe()),
      ref.listenManual(automationServiceProvider, (_, _) => observe()),
      ref.listenManual(automationScopeProvider, (_, _) => observe()),
      ref.listenManual(fileDialogServiceProvider, (_, _) => observe()),
      ref.listenManual(activeProfileProvider, (_, _) => observe()),
      ref.listenManual(criticalRuntimeEventsProvider, (_, _) => observe()),
      ref.listenManual(routerProvider, (_, _) => observe()),
    ];
    final notices = runtime.watchRuntimeNotices().listen(
      (_) => observe(),
      onError: (Object _, StackTrace _) => retire(),
      onDone: retire,
    );
    router.routeInformationProvider.addListener(observe);
    editor.addListener(observe);
    try {
      if (!originalCurrent() ||
          !inspect &&
              !await _leaveFile(
                originalCurrent: originalCurrent,
                originalLive: live,
                ownSaveRequest: (next) {
                  expectedRequest = next;
                },
                discardLabel: context.l10n.automationPortableUseSaved,
              ) ||
          !originalCurrent()) {
        return;
      }
      editorSnapshot = editor.text;
      if (!mounted || !originalCurrent()) return;
      if (createProject) {
        // Creating a separate project preserves the current dirty editor.
        final installed = await showAutomationPortableImportReview(
          context: context,
          ref: ref,
          originalScope: originalScope,
          originalService: service,
          expectedUiEpoch: epoch,
          sourceCurrent: originalCurrent,
          sourceChanges: editor,
          labels: portableImportLabels(context.l10n),
          scopeGateway: (service as AutomationPortableGatewayCapability).createPortableGateway(),
          reviewGateway: (service as AutomationPortableImportReviewCapability).createPortableImportReviewGateway(),
        );
        if (installed != null && originalCurrent()) {
          ref.invalidate(automationProjectsProvider); // refresh only, no select/open/run
        }
      } else {
        await showAutomationPortableOperation(
          context,
          ref,
          originalScope: originalScope,
          originalService: service,
          expectedUiEpoch: epoch,
          sourceCurrent: originalCurrent,
          sourceChanges: editor,
          labels: portableHostLabels(context.l10n, importMode: inspect),
          importMode: inspect,
          gateway: (service as AutomationPortableGatewayCapability).createPortableGateway(),
          projectId: inspect ? null : intended?.id,
          projectRevision: inspect ? null : intended?.revision,
        );
      }
    } finally {
      finished = true;
      editor.removeListener(observe);
      router.routeInformationProvider.removeListener(observe);
      for (final subscription in subscriptions) {
        subscription.close();
      }
      unawaited(notices.cancel());
      live.dispose();
    }
  }

  Future<void> _exportInventory(AutomationInventoryExportService service) async {
    final selectedProject = project;
    if (!current || busy || selectedProject == null || selectedProject.readOnly || selectedProject.revision == null) {
      return;
    }
    final selectedRequest = request;
    await _dialog<bool>(
      (_) => AutomationInventoryExportDialog(
        scope: widget.scope,
        project: selectedProject,
        service: service,
        isProjectCurrent: () =>
            current &&
            request == selectedRequest &&
            project?.id == selectedProject.id &&
            project?.revision == selectedProject.revision,
      ),
    );
  }

  Future<void> _dependencies(AutomationDependencyService service) async {
    if (busy || project == null || project!.readOnly || project!.revision == null || !current) return;
    // Capture EVERY original UI identity synchronously, before the dirty prompt.
    final selected = project!, originalScope = widget.scope, originalService = widget.service;
    final originalServices = ref.read(appServicesProvider), originalAutomation = ref.read(automationServiceProvider);
    final originalProfile = ref.read(activeProfileProvider)?.id.value;
    final router = ref.read(routerProvider), runtime = ref.read(criticalRuntimeEventsProvider);
    final runtimeEpoch = runtime?.currentScope.epoch;
    final route = router.routeInformationProvider.value.uri;
    if (originalProfile == null ||
        route.path != AppRoutes.automation ||
        !identical(originalAutomation, originalService) ||
        !identical(service, originalService)) {
      return;
    }
    var expectedRequest = request, retired = false, finished = false, refreshing = false;
    final live = ValueNotifier<bool>(true);
    bool matches() =>
        mounted &&
        current &&
        identical(widget.scope, originalScope) &&
        identical(widget.service, originalService) &&
        identical(ref.read(appServicesProvider), originalServices) &&
        identical(ref.read(automationServiceProvider), originalAutomation) &&
        ref.read(activeProfileProvider)?.id.value == originalProfile &&
        (refreshing || (identical(project, selected) && request == expectedRequest)) &&
        identical(ref.read(criticalRuntimeEventsProvider), runtime) &&
        runtime?.currentScope.epoch == runtimeEpoch &&
        runtime?.currentScope.streamUnavailable != true &&
        identical(ref.read(routerProvider), router) &&
        router.routeInformationProvider.value.uri == route;
    void retire() {
      if (retired || finished) return;
      retired = true;
      scheduleMicrotask(() {
        if (!finished) live.value = false;
      });
    }

    bool originalCurrent() {
      if (retired || finished) return false;
      if (!matches()) {
        retire();
        return false;
      }
      return true;
    }

    void observe() {
      if (!matches()) retire();
    }

    final subscriptions = <ProviderSubscription<dynamic>>[
      ref.listenManual(appServicesProvider, (_, _) => observe()),
      ref.listenManual(automationServiceProvider, (_, _) => observe()),
      ref.listenManual(automationScopeProvider, (_, _) => observe()),
      ref.listenManual(activeProfileProvider, (_, _) => observe()),
      ref.listenManual(criticalRuntimeEventsProvider, (_, _) => observe()),
      ref.listenManual(routerProvider, (_, _) => observe()),
    ];
    final notices = runtime?.watchRuntimeNotices().listen(
      (_) {
        observe();
      },
      onError: (Object _, StackTrace _) {
        retire();
      },
      onDone: retire,
    );
    router.routeInformationProvider.addListener(observe);
    try {
      if (!originalCurrent() ||
          !await _leaveFile(
            originalCurrent: originalCurrent,
            originalLive: live,
            ownSaveRequest: (next) {
              expectedRequest = next;
            },
          ) ||
          !originalCurrent()) {
        return;
      }
      if (dirty) {
        // _leaveFile returned true while still dirty ONLY after explicit Discard.
        setState(() {
          dirty = false;
          file = null;
          newPath = null;
          editor.clear();
          aiDraft = null;
          requirementsSavedPending = false;
        });
      }
      _showProjects(); // Retire an old prepared run composer; accepted jobs keep their signed snapshot policy.
      expectedRequest = ++request; // This operation's own synchronous invalidation of older file reads.
      if (!originalCurrent()) return;
      if (!mounted) return;
      final l = context.l10n;
      final result = await showGlassDialog<DependencyPublished>(
        context,
        barrierDismissible: false,
        builder: (_) => DependencyOriginalGuard(
          original: live,
          builder: (_) => AutomationScopeGuard(
            scope: originalScope,
            builder: (_) => AutomationDependencyDialog(
              scope: originalScope,
              service: service,
              project: selected,
              sourceCurrent: originalCurrent,
              originalLive: live,
              labels: DependencyInstallerLabels(
                title: l.automationDependencyTitle,
                kind: l.automationDependencyKind,
                role: l.automationDependencyRole,
                collection: l.automationDependencyCollection,
                name: l.automationDependencyName,
                version: l.automationDependencyVersion,
                url: l.automationDependencyUrl,
                sha256: l.automationDependencySha256,
                runner: l.automationDependencyRunner,
                review: l.automationDependencyReviewSource,
                download: l.automationDependencyDownload,
                diff: l.automationDependencyReviewChanges,
                install: l.automationDependencyInstall,
                cancel: l.commonCancel,
                close: l.commonClose,
                firstInstall: l.automationDependencyFirstOnly,
                managedOnly: l.automationDependencyManagedOnly,
                profileUnsupported: l.automationDependencyProfileUnsupported,
                excludedEntries: l.automationDependencyExcludedEntries,
                platformUnsupported: l.automationDependencyPlatformUnsupported,
                networkNotice: l.automationDependencyNetworkNotice,
                preparing: l.automationDependencyPreparing,
                published: l.automationDependencyPublished,
                uncertain: l.automationDependencyUncertain,
                cancelRequested: l.automationDependencyCancelRequested,
                failureText: (code) => switch (code) {
                  DependencyFailureCode.invalidInput => l.automationDependencyErrorInvalidInput,
                  DependencyFailureCode.changed => l.automationDependencyErrorChanged,
                  DependencyFailureCode.busy => l.automationDependencyErrorBusy,
                  DependencyFailureCode.unsupported => l.automationDependencyProfileUnsupported,
                  DependencyFailureCode.cancelled => l.automationDependencyCancelRequested,
                  DependencyFailureCode.recoveryRequired => l.automationDependencyUncertain,
                  _ => l.automationDependencyErrorUnavailable,
                },
              ),
            ),
          ),
        ),
      );
      if (!originalCurrent()) return;
      ref.invalidate(automationProjectsProvider);
      try {
        // Always recapture ACTUAL Native catalog, including an uncertain outcome.
        // Never derive/select a path or source kind from a dialog result.
        final catalog = await widget.service.projects();
        if (!originalCurrent()) return;
        AutomationProject? fresh;
        for (final row in catalog.projects) {
          if (row.id == selected.id) {
            fresh = row;
            break;
          }
        }
        if (fresh == null ||
            catalog.readOnly ||
            fresh.readOnly ||
            fresh.sourceKind != selected.sourceKind ||
            fresh.revision == null ||
            (result != null && fresh.revision! < result.revision)) {
          throw const DependencyInstallerFailure(DependencyFailureCode.changed);
        }
        if (result != null || fresh.revision != selected.revision) {
          // The installer is CLOSED; only a same-ID actual Native catalog reload
          // follows. Its own _select token fences project changes. Preserve the
          // original profile/service/route lifetime through the file-list await.
          refreshing = true;
          await _select(fresh, originalCurrent: originalCurrent);
        }
      } catch (_) {
        if (originalCurrent()) setState(() => error = FormatException(l.automationDependencyErrorChanged));
      }
    } finally {
      finished = true;
      router.routeInformationProvider.removeListener(observe);
      for (final subscription in subscriptions) {
        subscription.close();
      }
      unawaited(notices?.cancel());
      live.dispose();
    }
  }

  Future<void> _groups(AutomationInventoryService service) async {
    if (busy || project == null || project!.readOnly) return;
    final originalId = project!.id;
    final result = await _dialog<AutomationProject>(
      (_) => AutomationGroupsDialog(scope: widget.scope, projectId: originalId, service: service),
    );
    if (current && result != null && project?.id == originalId) {
      setState(() => project = result);
      ref.invalidate(automationProjectsProvider);
    }
  }

  Future<void> _credentials(AutomationProjectCredentialsService service) async {
    if (busy || project == null || project!.readOnly || !current) return;
    final selected = project!, source = widget.service, scope = widget.scope, token = request;
    final app = ref.read(appServicesProvider), runtime = ref.read(criticalRuntimeEventsProvider);
    final epoch = runtime?.currentScope.epoch, router = ref.read(routerProvider);
    final uri = router.routeInformationProvider.value.uri;
    var retired = false;
    final live = ValueNotifier(true);
    bool matches() =>
        current &&
        identical(widget.scope, scope) &&
        identical(widget.service, source) &&
        identical(ref.read(appServicesProvider), app) &&
        identical(ref.read(automationServiceProvider), source) &&
        identical(project, selected) &&
        request == token &&
        identical(ref.read(criticalRuntimeEventsProvider), runtime) &&
        runtime?.currentScope.epoch == epoch &&
        runtime?.currentScope.streamUnavailable != true &&
        uri.path == AppRoutes.automation &&
        identical(ref.read(routerProvider), router) &&
        router.routeInformationProvider.value.uri == uri;
    void observe() {
      if (!retired && !matches()) {
        retired = true;
        live.value = false;
      }
    }

    bool original() {
      observe();
      return !retired;
    }

    final subscriptions = <ProviderSubscription<dynamic>>[
      ref.listenManual(appServicesProvider, (_, _) => observe()),
      ref.listenManual(automationServiceProvider, (_, _) => observe()),
      ref.listenManual(automationScopeProvider, (_, _) => observe()),
      ref.listenManual(criticalRuntimeEventsProvider, (_, _) => observe()),
    ];
    final notices = runtime?.watchRuntimeNotices().listen(
      (_) => observe(),
      onError: (Object _, StackTrace _) {
        retired = true;
        live.value = false;
      },
      onDone: () {
        retired = true;
        live.value = false;
      },
    );
    router.routeInformationProvider.addListener(observe);
    try {
      if (!original()) return;
      final result = await _dialog<AutomationProject>(
        (_) => DependencyOriginalGuard(
          original: live,
          builder: (_) => AutomationProjectCredentialsDialog(
            scope: scope,
            project: selected,
            service: service,
            originalCurrent: original,
            originalLive: live,
          ),
        ),
      );
      if (original() && result != null) {
        setState(() => project = result);
        ref.invalidate(automationProjectsProvider);
      }
    } finally {
      retired = true;
      for (final subscription in subscriptions) {
        subscription.close();
      }
      await notices?.cancel();
      router.routeInformationProvider.removeListener(observe);
      live.dispose();
    }
  }

  Future<void> _metadata(AutomationProjectMetadataService service) async {
    if (busy || project == null || project!.readOnly || !current) return;
    final selected = project!, source = widget.service, scope = widget.scope, token = request;
    final app = ref.read(appServicesProvider), runtime = ref.read(criticalRuntimeEventsProvider);
    final epoch = runtime?.currentScope.epoch, router = ref.read(routerProvider);
    final uri = router.routeInformationProvider.value.uri;
    var retired = false;
    final live = ValueNotifier(true);
    bool matches() =>
        current &&
        identical(widget.scope, scope) &&
        identical(widget.service, source) &&
        identical(ref.read(appServicesProvider), app) &&
        identical(ref.read(automationServiceProvider), source) &&
        identical(project, selected) &&
        request == token &&
        identical(ref.read(criticalRuntimeEventsProvider), runtime) &&
        runtime?.currentScope.epoch == epoch &&
        runtime?.currentScope.streamUnavailable != true &&
        uri.path == AppRoutes.automation &&
        identical(ref.read(routerProvider), router) &&
        router.routeInformationProvider.value.uri == uri;
    void observe() {
      if (!retired && !matches()) {
        retired = true;
        live.value = false;
      }
    }

    bool original() {
      observe();
      return !retired;
    }

    final subscriptions = <ProviderSubscription<dynamic>>[
      ref.listenManual(appServicesProvider, (_, _) => observe()),
      ref.listenManual(automationServiceProvider, (_, _) => observe()),
      ref.listenManual(automationScopeProvider, (_, _) => observe()),
      ref.listenManual(criticalRuntimeEventsProvider, (_, _) => observe()),
    ];
    final notices = runtime?.watchRuntimeNotices().listen(
      (_) => observe(),
      onError: (Object _, StackTrace _) {
        retired = true;
        live.value = false;
      },
      onDone: () {
        retired = true;
        live.value = false;
      },
    );
    router.routeInformationProvider.addListener(observe);
    try {
      if (!original()) return;
      final subject = runtime?.currentScope.scope;
      if (subject == null ||
          subject.profileId != scope.profileId?.toString() ||
          runtime?.currentScope.unsupported != false) {
        return;
      }
      await _dialog<void>(
        (_) => DependencyOriginalGuard(
          original: live,
          builder: (_) => AutomationProjectMetadataDialog(
            service: service,
            project: selected,
            profileId: subject.profileId,
            nonce: subject.nonce,
            originalCurrent: original,
            originalLive: live,
          ),
        ),
      );
    } finally {
      retired = true;
      for (final subscription in subscriptions) {
        subscription.close();
      }
      await notices?.cancel();
      router.routeInformationProvider.removeListener(observe);
      live.dispose();
    }
  }

  Future<void> _inventory(AutomationSourceInventoryService service) async {
    if (busy || project == null || project!.readOnly || !current) return;
    final selected = project!, source = widget.service, scope = widget.scope, token = request;
    final app = ref.read(appServicesProvider), runtime = ref.read(criticalRuntimeEventsProvider);
    final epoch = runtime?.currentScope.epoch, router = ref.read(routerProvider);
    final uri = router.routeInformationProvider.value.uri;
    var retired = false;
    bool matches() =>
        current &&
        identical(widget.scope, scope) &&
        identical(widget.service, source) &&
        identical(ref.read(appServicesProvider), app) &&
        identical(ref.read(automationServiceProvider), source) &&
        identical(project, selected) &&
        request == token &&
        identical(ref.read(criticalRuntimeEventsProvider), runtime) &&
        runtime?.currentScope.epoch == epoch &&
        runtime?.currentScope.streamUnavailable != true &&
        uri.path == AppRoutes.automation &&
        identical(ref.read(routerProvider), router) &&
        router.routeInformationProvider.value.uri == uri;
    void observe() {
      if (!matches()) retired = true;
    }

    bool original() {
      observe();
      return !retired;
    }

    final subscriptions = <ProviderSubscription<dynamic>>[
      ref.listenManual(appServicesProvider, (_, _) => observe()),
      ref.listenManual(automationServiceProvider, (_, _) => observe()),
      ref.listenManual(automationScopeProvider, (_, _) => observe()),
      ref.listenManual(criticalRuntimeEventsProvider, (_, _) => observe()),
    ];
    final notices = runtime?.watchRuntimeNotices().listen(
      (_) => observe(),
      onError: (Object _, StackTrace _) => retired = true,
      onDone: () => retired = true,
    );
    router.routeInformationProvider.addListener(observe);
    try {
      if (!original()) return;
      final result = await _dialog<AutomationProject>(
        (_) => AutomationInventoryDialog(
          scope: scope,
          projectId: selected.id,
          files: source,
          service: service,
          originalCurrent: original,
        ),
      );
      if (original() && result != null) {
        setState(() => project = result);
        ref.invalidate(automationProjectsProvider);
      }
    } finally {
      retired = true;
      for (final subscription in subscriptions) {
        subscription.close();
      }
      await notices?.cancel();
      router.routeInformationProvider.removeListener(observe);
    }
  }

  Future<void> _repairJob(AutomationJson selectedJob) async {
    final unavailable = context.l10n.automationRetryUnavailable;
    final service = widget.service, jobs = ref.read(automationJobServiceProvider);
    final app = ref.read(appServicesProvider), scope = widget.scope;
    final events = ref.read(criticalRuntimeEventsProvider);
    final epoch = events?.currentScope.epoch, router = ref.read(routerProvider);
    final id = selectedJob['job_id'], digest = selectedJob['manifest_digest'];
    final rev = selectedJob['notification_revision'], projectId = selectedJob['project_id'];
    if (busy ||
        jobs == null ||
        service is! AutomationAiService ||
        scope.profileId is! ObjectId ||
        id is! String ||
        digest is! String ||
        rev is! int ||
        projectId is! String ||
        selectedJob['state'] != 'failed') {
      return;
    }
    var token = request, retired = false, finished = false;
    final live = ValueNotifier<bool>(true);
    bool matches() =>
        current &&
        request == token &&
        identical(widget.service, service) &&
        identical(ref.read(automationServiceProvider), service) &&
        identical(ref.read(automationJobServiceProvider), jobs) &&
        identical(ref.read(appServicesProvider), app) &&
        identical(ref.read(criticalRuntimeEventsProvider), events) &&
        events?.currentScope.epoch == epoch &&
        events?.currentScope.streamUnavailable != true &&
        router.routeInformationProvider.value.uri.path == AppRoutes.automation;
    void retire() {
      if (retired || finished) return;
      retired = true;
      scheduleMicrotask(() {
        if (!finished) live.value = false;
      });
    }

    bool original() {
      if (retired || finished) return false;
      if (!matches()) {
        retire();
        return false;
      }
      return true;
    }

    void observe() {
      if (!matches()) retire();
    }

    final subscriptions = <ProviderSubscription<dynamic>>[
      ref.listenManual(appServicesProvider, (_, _) => observe()),
      ref.listenManual(automationScopeProvider, (_, _) => observe()),
      ref.listenManual(automationServiceProvider, (_, _) => observe()),
      ref.listenManual(automationJobServiceProvider, (_, _) => observe()),
      ref.listenManual(criticalRuntimeEventsProvider, (_, _) => observe()),
    ];
    final notices = events?.watchRuntimeNotices().listen(
      (_) => observe(),
      onError: (Object _, StackTrace _) => retire(),
      onDone: retire,
    );
    router.routeInformationProvider.addListener(observe);
    try {
      if (!original() ||
          !await _leaveFile(originalCurrent: original, originalLive: live, ownSaveRequest: (next) => token = next) ||
          !original()) {
        return;
      }
      setState(() {
        busy = true;
        error = null;
      });
      final catalog = await jobs.jobs();
      if (!original()) return;
      final rows = catalog['jobs'];
      if (catalog['schema_version'] != 1 || catalog['read_only'] != false || rows is! List || rows.length > 2048) {
        throw FormatException(unavailable);
      }
      final row = rows.whereType<AutomationJson>().where((row) => row['job_id'] == id).singleOrNull;
      if (row == null ||
          row['state'] != 'failed' ||
          jobContextApprovedHostIds(
                row,
                profileId: (scope.profileId as ObjectId).value,
                jobId: id,
                revision: rev,
                projectId: projectId,
                manifestDigest: digest,
              ) ==
              null) {
        throw FormatException(unavailable);
      }
      final projects = await service.projects();
      if (!original()) return;
      final selected = projects.projects.where((p) => p.id == projectId && !p.readOnly).singleOrNull;
      final manifest = row['manifest'];
      final path = manifest is Map ? manifest['playbook'] : null;
      if (projects.readOnly || selected == null || path is! String) {
        throw FormatException(unavailable);
      }
      final tree = await service.files(projectId);
      if (!original()) return;
      if (!tree.entries.any((entry) => !entry.directory && entry.path == path)) {
        throw FormatException(unavailable);
      }
      final loaded = await service.readFile(projectId, path);
      if (!original()) return;
      final verifiedTree = await service.files(projectId);
      if (!original()) return;
      if (verifiedTree.digest != tree.digest || loaded.path != path) {
        throw FormatException(unavailable);
      }
      setState(() {
        project = selected;
        revision = verifiedTree;
        file = loaded;
        aiDraft = null;
        newPath = null;
        requirementsSavedPending = false;
        editor.text = loaded.text;
        dirty = false;
        workspacePage = 0;
        repairOrigin = row;
        repairApp = app;
        repairService = service;
        busy = false;
      });
      await _ai(service as AutomationAiService, parentCurrent: original, parentLive: live);
    } catch (e) {
      if (original()) setState(() => error = e);
    } finally {
      if (mounted && identical(widget.scope, scope) && request == token) setState(() => busy = false);
      finished = true;
      for (final subscription in subscriptions) {
        subscription.close();
      }
      await notices?.cancel();
      router.routeInformationProvider.removeListener(observe);
      live.dispose();
    }
  }

  Future<void> _ai(
    AutomationAiService service, {
    bool Function()? parentCurrent,
    ValueListenable<bool>? parentLive,
  }) async {
    final path = file?.path ?? newPath;
    if (busy || path == null) return;
    final before = editor.text;
    final projectId = project?.id, projectRevision = project?.revision;
    final origin = repairForCurrent;
    final app = ref.read(appServicesProvider), jobs = ref.read(automationJobServiceProvider);
    final epoch = request, originalService = widget.service;
    final events = ref.read(criticalRuntimeEventsProvider), router = ref.read(routerProvider);
    final contextFrame = events?.currentScope;
    final hubScope = contextFrame?.epoch;
    final nativeContextScope = contextFrame?.unsupported == false && contextFrame?.streamUnavailable == false
        ? contextFrame?.scope
        : null;
    bool original() =>
        current &&
        parentCurrent?.call() != false &&
        request == epoch &&
        identical(ref.read(appServicesProvider), app) &&
        identical(widget.service, originalService) &&
        identical(ref.read(automationServiceProvider), originalService) &&
        identical(ref.read(automationJobServiceProvider), jobs) &&
        identical(ref.read(criticalRuntimeEventsProvider), events) &&
        events?.currentScope.epoch == hubScope &&
        events?.currentScope.streamUnavailable != true &&
        router.routeInformationProvider.value.uri.path == AppRoutes.automation &&
        project?.id == projectId &&
        (file?.path ?? newPath) == path;
    Future<bool> recheck() async {
      if (!original() || origin == null || jobs == null) return false;
      final catalog = await jobs.jobs();
      if (!original()) return false;
      final rows = catalog['jobs'];
      if (catalog['schema_version'] != 1 || catalog['read_only'] != false || rows is! List || rows.length > 2048) {
        return false;
      }
      return rows.whereType<AutomationJson>().any(
        (row) =>
            row['job_id'] == origin['job_id'] &&
            row['manifest_digest'] == origin['manifest_digest'] &&
            row['notification_revision'] == origin['notification_revision'] &&
            row['project_id'] == origin['project_id'] &&
            row['state'] == 'failed' &&
            widget.scope.profileId is ObjectId &&
            jobContextApprovedHostIds(
                  row,
                  profileId: (widget.scope.profileId as ObjectId).value,
                  jobId: origin['job_id'] as String,
                  revision: origin['notification_revision'] as int,
                  projectId: origin['project_id'] as String,
                  manifestDigest: origin['manifest_digest'] as String,
                ) !=
                null,
      );
    }

    final result = await _dialog<AutomationAiDraft>(
      (_) => DependencyOriginalGuard(
        original: parentLive,
        builder: (_) => AutomationAiDialog(
          scope: widget.scope,
          service: service,
          path: path,
          before: before,
          projectId: projectId,
          projectRevision: projectRevision,
          nativeContextScope: nativeContextScope,
          originalCurrent: original,
          repairOrigin: origin,
          repairEvents: jobs is AutomationEventService ? jobs as AutomationEventService : null,
          recheckRepair: origin == null ? null : recheck,
        ),
      ),
    );
    // A proposal is only a local edit, never an implicit file write or run.
    // Preserve the existing file SHA so a concurrent disk edit is still caught.
    if (current &&
        result != null &&
        (editor.text != before || project?.id != projectId || (file?.path ?? newPath) != path)) {
      setState(() => error = const _AiDraftStale());
      return;
    }
    if (original() && result != null) {
      try {
        if (aiDraft case final previous?) result.retainRequirements(previous);
        setState(() => aiDraft = result);
        editor.text = result.text;
        _edited();
      } catch (e) {
        setState(() => error = e);
      }
    }
  }

  Future<void> _select(AutomationProject selected, {bool Function()? originalCurrent}) async {
    if (busy ||
        originalCurrent?.call() == false ||
        !await _leaveFile(originalCurrent: originalCurrent) ||
        !current ||
        originalCurrent?.call() == false) {
      return;
    }
    final token = ++request;
    setState(() {
      busy = true;
      error = null;
      project = selected;
      repairOrigin = null;
      revision = null;
      file = null;
      aiDraft = null;
      requirementsSavedPending = false;
      newPath = null;
      editor.clear();
    });
    try {
      final result = await widget.service.files(selected.id);
      if (current && token == request && originalCurrent?.call() != false) setState(() => revision = result);
    } catch (e) {
      if (current && token == request && originalCurrent?.call() != false) setState(() => error = e);
    } finally {
      if (current && token == request) setState(() => busy = false);
    }
  }

  Future<void> _read(String path) async {
    if (busy || project == null || !await _leaveFile() || !current) return;
    final token = ++request;
    setState(() {
      busy = true;
      error = null;
    });
    try {
      final result = await widget.service.readFile(project!.id, path);
      if (current && token == request) {
        setState(() {
          file = result;
          repairOrigin = null;
          aiDraft = null;
          requirementsSavedPending = false;
          newPath = null;
          editor.text = result.text;
          dirty = false;
        });
      }
    } catch (e) {
      if (current && token == request) setState(() => error = e);
    } finally {
      if (current && token == request) setState(() => busy = false);
    }
  }

  Future<bool> _save({bool Function()? originalCurrent, ValueListenable<bool>? originalLive}) async {
    if (busy || project == null || (file == null && newPath == null)) return false;
    final token = ++request;
    bool valid() => current && token == request && originalCurrent?.call() != false;
    if (!valid()) return false;
    setState(() {
      busy = true;
      error = null;
    });
    try {
      if (aiDraft?.hasRequirements == true) {
        final tree = await widget.service.files(project!.id);
        if (!valid()) return false;
        // Reject noncanonical variants rather than create a second contract
        // which could behave differently on a case-sensitive Runner.
        if (tree.entries.any(
          (entry) =>
              entry.path.toLowerCase() == automationRequirementsFile &&
              (entry.path != automationRequirementsFile || entry.directory),
        )) {
          throw const AutomationAiDraftException(AutomationAiDraftError.canonicalFilename);
        }
        final exists = tree.entries.any((entry) => entry.path == automationRequirementsFile);
        final old = exists ? await widget.service.readFile(project!.id, automationRequirementsFile) : null;
        if (!valid()) return false;
        final after = aiDraft!.requirements(file?.path ?? newPath!, before: old?.text);
        final accepted = await _dialog<bool>(
          (context) => DependencyOriginalGuard(
            original: originalLive,
            builder: (context) => AlertDialog(
              title: Text(context.l10n.automationAiRequirementsTitle),
              content: SizedBox(
                width: 820,
                height: MediaQuery.sizeOf(context).height * .6,
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    Text(context.l10n.automationAiRequirementsHint),
                    const SizedBox(height: 8),
                    const Text(automationRequirementsFile),
                    Expanded(
                      child: KubeYamlDiff(
                        before: old?.text ?? '',
                        after: after,
                        explanation: context.l10n.automationAiRequirementsHint,
                      ),
                    ),
                  ],
                ),
              ),
              actions: [
                TextButton(onPressed: () => Navigator.pop(context, false), child: Text(context.l10n.commonCancel)),
                FilledButton(
                  key: const ValueKey('automation-ai-save-requirements'),
                  onPressed: () => Navigator.pop(context, true),
                  child: Text(context.l10n.commonSave),
                ),
              ],
            ),
          ),
        );
        if (accepted != true || !valid()) return false;
        // Constraints FIRST. If the second write conflicts/fails, execution is
        // still constrained; no interval admits new YAML without requirements.
        // Both are ordinary CAS writes, never rollback over another editor.
        await widget.service.saveFile(
          project!.id,
          path: automationRequirementsFile,
          text: after,
          expectedSha256: old?.sha256,
        );
        if (!valid()) return false;
        requirementsSavedPending = true;
      }
      final result = await widget.service.saveFile(
        project!.id,
        path: file?.path ?? newPath!,
        text: editor.text,
        expectedSha256: file?.sha256,
      );
      if (!valid()) return false;
      setState(() {
        file = result;
        aiDraft = null;
        requirementsSavedPending = false;
        newPath = null;
        dirty = editor.text != result.text;
      });
      final tree = await widget.service.files(project!.id);
      if (valid()) setState(() => revision = tree);
      return valid();
    } catch (e) {
      if (valid()) setState(() => error = e);
      return false;
    } finally {
      if (current && token == request) setState(() => busy = false);
    }
  }

  Future<void> _externalEditor(AutomationEditorService service) async {
    if (busy ||
        project == null ||
        file == null ||
        project!.readOnly ||
        !current ||
        !identical(service, widget.service) ||
        _retireExternalEditor != null) {
      return;
    }
    final labels = context.l10n, selected = project!, path = file!.path, text = editor.text;
    final source = widget.service, scope = widget.scope, app = ref.read(appServicesProvider);
    final picker = ref.read(fileDialogServiceProvider), runtime = ref.read(criticalRuntimeEventsProvider);
    final epoch = runtime?.currentScope.epoch, router = ref.read(routerProvider);
    final uri = router.routeInformationProvider.value.uri;
    var token = request, retired = false, finished = false, ownsBusy = false;
    final live = ValueNotifier<bool>(true);
    bool matches() =>
        current &&
        identical(widget.scope, scope) &&
        identical(widget.service, source) &&
        identical(ref.read(automationServiceProvider), source) &&
        identical(ref.read(appServicesProvider), app) &&
        identical(ref.read(fileDialogServiceProvider), picker) &&
        identical(project, selected) &&
        file?.path == path &&
        editor.text == text &&
        request == token &&
        workspacePage == 0 &&
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

    void observe() {
      if (!matches()) retire();
    }

    bool original() {
      observe();
      return !retired && !finished;
    }

    final retirement = retire;
    _retireExternalEditor = retirement;
    final subscriptions = <ProviderSubscription<dynamic>>[
      ref.listenManual(appServicesProvider, (_, _) => observe()),
      ref.listenManual(automationServiceProvider, (_, _) => observe()),
      ref.listenManual(automationScopeProvider, (_, _) => observe()),
      ref.listenManual(fileDialogServiceProvider, (_, _) => observe()),
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
      if (dirty) {
        final save = await _dialog<bool>(
          (context) => DependencyOriginalGuard(
            original: live,
            builder: (context) => AlertDialog(
              title: Text(context.l10n.automationUnsaved),
              content: Text(context.l10n.automationEditorSaveHint),
              actions: [
                TextButton(onPressed: () => Navigator.pop(context, false), child: Text(context.l10n.commonCancel)),
                FilledButton(onPressed: () => Navigator.pop(context, true), child: Text(context.l10n.commonSave)),
              ],
            ),
          ),
        );
        if (!original() || save != true) return;
        token = request + 1; // Only the immediately invoked save owns this request transition.
        if (!await _save(originalCurrent: () => matches() && !retired, originalLive: live) || !original()) return;
      }
      if (!original()) return;
      final sha = file!.sha256;
      ownsBusy = true;
      setState(() {
        busy = true;
        error = null;
      });
      final application = await picker.chooseApplication(
        label: labels.sftpApplications,
        confirmButtonText: labels.sftpChooseApplication,
      );
      if (!original() || application == null) return;
      await service.openProjectEditor(
        selected.id,
        path,
        expectedSha256: sha,
        editor: {'kind': 'path', 'value': application},
      );
      // Disk changes are read only by explicit Reload; the saved SHA still
      // protects a later ordinary save from overwriting concurrent edits.
    } catch (e) {
      if (original()) setState(() => error = e);
    } finally {
      if (ownsBusy && current && request == token) setState(() => busy = false);
      finished = true;
      retired = true;
      if (identical(_retireExternalEditor, retirement)) _retireExternalEditor = null;
      for (final subscription in subscriptions) {
        subscription.close();
      }
      await notices?.cancel();
      router.routeInformationProvider.removeListener(observe);
      live.dispose();
    }
  }

  Future<void> _createCadvisorTemplate({bool install = false, AgentCadvisorComposerFocus? cadvisorFocus}) async {
    if (busy || !current || install && widget.service is! AutomationJobService) return;
    final source = widget.service, scope = widget.scope;
    final app = ref.read(appServicesProvider), runtime = ref.read(criticalRuntimeEventsProvider);
    final epoch = runtime?.currentScope.epoch, router = ref.read(routerProvider);
    final uri = router.routeInformationProvider.value.uri;
    final live = ValueNotifier(true);
    var retired = false;
    bool matches() =>
        current &&
        (cadvisorFocus == null || _cadvisorCurrent(cadvisorFocus)) &&
        identical(widget.scope, scope) &&
        identical(widget.service, source) &&
        identical(ref.read(appServicesProvider), app) &&
        identical(ref.read(automationServiceProvider), source) &&
        identical(ref.read(criticalRuntimeEventsProvider), runtime) &&
        runtime?.currentScope.epoch == epoch &&
        runtime?.currentScope.streamUnavailable != true &&
        identical(ref.read(routerProvider), router) &&
        uri.path == AppRoutes.automation &&
        router.routeInformationProvider.value.uri == uri;
    void observe() {
      if (!retired && !matches()) {
        retired = true;
        live.value = false;
      }
    }

    bool original() {
      observe();
      return !retired;
    }

    final subscriptions = <ProviderSubscription<dynamic>>[
      ref.listenManual(appServicesProvider, (_, _) => observe()),
      ref.listenManual(automationServiceProvider, (_, _) => observe()),
      ref.listenManual(automationScopeProvider, (_, _) => observe()),
      ref.listenManual(criticalRuntimeEventsProvider, (_, _) => observe()),
      if (cadvisorFocus != null) ref.listenManual(hostsProvider, (_, _) => observe()),
    ];
    final notices = runtime?.watchRuntimeNotices().listen(
      (_) => observe(),
      onError: (Object _, StackTrace _) {
        retired = true;
        live.value = false;
      },
      onDone: () {
        retired = true;
        live.value = false;
      },
    );
    router.routeInformationProvider.addListener(observe);
    try {
      if (!original()) return;
      final accepted = await _dialog<bool>(
        (_) => DependencyOriginalGuard(original: live, builder: (_) => const AutomationCadvisorReview()),
      );
      if (accepted != true || !original()) return;
      AutomationProject? created;
      await _create(
        exporterTemplate: AutomationExporterTemplate.cadvisor,
        originalCurrent: original,
        originalLive: live,
        onTemplateCreated: (value) => created = value,
      );
      final fresh = created;
      if (!install || fresh == null || !original() || project?.id != fresh.id) return;
      final token = request;
      bool originalProject() => original() && request == token && project?.id == fresh.id && !dirty;
      final binding = await AutomationCadvisorTemplateBinding.capture(source, fresh.id, isCurrent: originalProject);
      if (!originalProject()) return;
      setState(() => project = binding.project);
      final options = await _dialog<AutomationCadvisorInstallOptions>(
        (_) => DependencyOriginalGuard(
          original: live,
          builder: (_) => _CadvisorInstallDialog(owner: fresh.id),
        ),
      );
      if (options == null || !originalProject()) return;
      await binding.recheck(source, isCurrent: originalProject);
      if (!originalProject()) return;
      await _run(
        source as AutomationJobService,
        initialExtraVars: options.extraVars,
        cadvisorTemplate: binding,
        cadvisorFocus: cadvisorFocus,
      );
    } catch (e) {
      if (original()) setState(() => error = context.l10n.automationCadvisorInstallInvalid);
    } finally {
      retired = true;
      for (final subscription in subscriptions) {
        subscription.close();
      }
      await notices?.cancel();
      router.routeInformationProvider.removeListener(observe);
      live.dispose();
    }
  }

  Future<void> _create({
    bool folder = false,
    AutomationExporterTemplate? exporterTemplate,
    bool Function()? originalCurrent,
    ValueListenable<bool>? originalLive,
    ValueChanged<AutomationProject>? onTemplateCreated,
  }) async {
    if (busy ||
        originalCurrent?.call() == false ||
        !await _leaveFile(originalCurrent: originalCurrent, originalLive: originalLive) ||
        !current ||
        !mounted ||
        originalCurrent?.call() == false) {
      return;
    }
    final directory = folder ? await ref.read(fileDialogServiceProvider).chooseDirectory() : null;
    if (!mounted || !current || originalCurrent?.call() == false || (folder && directory == null)) return;
    final parts = directory?.replaceAll('\\', '/').split('/').where((part) => part.isNotEmpty).toList();
    final name = folder && parts!.isNotEmpty
        ? String.fromCharCodes(parts.last.runes.take(120))
        : await _dialog<String>(
            (context) => DependencyOriginalGuard(
              original: originalLive,
              builder: (_) => _NameDialog(title: context.l10n.automationNewProject, initial: ''),
            ),
          );
    if (!mounted || !current || originalCurrent?.call() == false || name == null) return;
    setState(() {
      busy = true;
      error = null;
    });
    AutomationProject? created;
    Object? creationError;
    final templateFailureMessage = context.l10n.monitoringExporterTemplateIncomplete;
    final originalService = widget.service, originalScope = widget.scope;
    bool original() =>
        current &&
        originalCurrent?.call() != false &&
        identical(widget.service, originalService) &&
        identical(widget.scope, originalScope);
    try {
      final assets = exporterTemplate == null
          ? null
          : await loadExporterTemplate(exporterTemplate, rootBundle.loadString);
      if (!original()) return;
      created = await originalService.createProject(name: name, directory: directory);
      if (assets != null && original()) {
        await populateExporterTemplate(originalService, created, assets, isCurrent: original);
      }
    } catch (e) {
      creationError = exporterTemplate == null ? e : templateFailureMessage;
      if (original()) setState(() => error = creationError);
    } finally {
      if (current) setState(() => busy = false);
    }
    if (!original()) return;
    ref.invalidate(automationProjectsProvider);
    if (created != null) await _select(created, originalCurrent: original);
    if (original() &&
        creationError == null &&
        created != null &&
        exporterTemplate != null &&
        project?.id == created.id) {
      onTemplateCreated?.call(created);
    }
    if (original() && creationError != null) setState(() => error = creationError);
  }

  Future<void> _rename() async {
    // Project-source metadata is retained when only the display name changes.
    final selected = project;
    if (selected == null || busy || selected.readOnly) return;
    final name = await _name(context.l10n.commonRename, initial: selected.name ?? '');
    if (name == null || !current) return;
    setState(() {
      busy = true;
      error = null;
    });
    try {
      final result = await widget.service.renameProject(selected.id, revision: selected.revision!, name: name);
      if (current) {
        setState(() => project = result);
        ref.invalidate(automationProjectsProvider);
      }
    } catch (e) {
      if (current) setState(() => error = e);
    } finally {
      if (current) setState(() => busy = false);
    }
  }

  Future<void> _ansibleConfig() async {
    if (busy || project == null || project!.readOnly || !await _leaveFile() || !current) return;
    final id = project!.id;
    final token = ++request;
    AutomationProjectFile? original;
    setState(() {
      busy = true;
      error = null;
    });
    try {
      final tree = await widget.service.files(id);
      if (!current || token != request || project?.id != id) return;
      if (tree.entries.any((entry) => entry.path == 'ansible.cfg' && !entry.directory)) {
        original = await widget.service.readFile(id, 'ansible.cfg');
      }
      if (!current || token != request || project?.id != id) return;
      setState(() => revision = tree);
    } catch (e) {
      if (current && token == request) setState(() => error = e);
      return;
    } finally {
      if (current && token == request) setState(() => busy = false);
    }
    if (!current || token != request) return;
    final result = await _dialog<AutomationAnsibleConfigResult>(
      (_) => AutomationAnsibleConfigDialog(text: original?.text ?? ''),
    );
    if (!current || token != request || project?.id != id || result == null) return;
    setState(() {
      file = original;
      repairOrigin = null;
      newPath = original == null ? 'ansible.cfg' : null;
      aiDraft = null;
      requirementsSavedPending = false;
      editor.text = result.proposedText ?? original?.text ?? '';
      dirty = editor.text != (original?.text ?? '');
    });
    // The explicit form Save uses the ordinary file CAS, including conflicts.
    if (result.action == AutomationAnsibleConfigAction.apply && dirty) await _save();
  }

  Future<void> _git(AutomationGitService service) async {
    if (busy || !await _leaveFile() || !current) return;
    final created = await _dialog<AutomationProject>(
      (_) => AutomationGitDialog(scope: widget.scope, service: widget.service, git: service),
    );
    if (!current) return;
    ref.invalidate(automationProjectsProvider);
    if (created != null) await _select(created);
  }

  Future<void> _newFile({bool playbook = false}) async {
    if (busy || project == null || !await _leaveFile() || !current || !mounted) return;
    var suggestion = 'playbook.yml';
    if (playbook) {
      final existing = {for (final entry in revision?.entries ?? <AutomationFileEntry>[]) entry.path};
      for (var index = 2; existing.contains(suggestion); index++) {
        suggestion = 'playbook_$index.yml';
      }
    }
    final path = await _name(
      playbook ? context.l10n.automationNewPlaybook : context.l10n.automationOtherFile,
      initial: playbook ? suggestion : '',
    );
    if (path == null || !current) return;
    if (playbook &&
        (!RegExp(r'\.(yml|yaml)$', caseSensitive: false).hasMatch(path) ||
            path.startsWith('/') ||
            path.contains('\\') ||
            path.split('/').any((part) => part.isEmpty || part == '..' || part == '.'))) {
      setState(() => error = FormatException(context.l10n.automationPlaybookPathHint));
      return;
    }
    setState(() {
      file = null;
      repairOrigin = null;
      newPath = path;
      aiDraft = null;
      requirementsSavedPending = false;
      editor.text = playbook
          ? '---\n- name: Describe the task\n  hosts: all\n  gather_facts: false\n  tasks:\n    - name: Show a message\n      ansible.builtin.debug:\n        msg: ConsoleCrypt playbook scaffold\n' // l10n-ignore: editable YAML source scaffold, not interface copy
          : '';
      dirty = true;
      error = null;
    });
  }

  Future<void> _newRole(AutomationScaffoldService service) async {
    if (busy || project == null || project!.readOnly || !await _leaveFile() || !mounted || !current) return;
    final name = await _name(context.l10n.automationCreateRole);
    if (name == null || !current) return;
    if (!RegExp(r'^[a-z][a-z0-9_]{0,63}$').hasMatch(name)) {
      setState(() => error = FormatException(context.l10n.automationRoleNameHint));
      return;
    }
    setState(() {
      busy = true;
      error = null;
    });
    try {
      final result = await service.createRole(project!.id, name);
      if (!current) return;
      setState(() {
        file = result;
        newPath = null;
        aiDraft = null;
        requirementsSavedPending = false;
        editor.text = result.text;
        dirty = false;
      });
      final tree = await widget.service.files(project!.id);
      if (current) setState(() => revision = tree);
    } catch (e) {
      if (current) setState(() => error = e);
    } finally {
      if (current) setState(() => busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(monitoringSessionProvider, (previous, next) {
      if (!identical(previous, next)) {
        setState(() {
          _contextGeneration++;
          _acceptedProjectFocus = null;
          _focusedProject = null;
        });
      }
    });
    final l = context.l10n;
    final jobs = ref.watch(automationJobServiceProvider);
    final awxFactory = ref.watch(automationAwxPortFactoryProvider);
    final awxEvents = ref.watch(criticalRuntimeEventsProvider);
    final awxAvailable = awxFactory != null && awxEvents != null;
    if (workspacePage != 0) _retireExternalEditor?.call();
    final visiblePage = workspacePage == 3 && !awxAvailable ? 0 : workspacePage;
    Widget navigation(String key, int page, IconData icon, String label, VoidCallback select) => Semantics(
      selected: visiblePage == page,
      child: GlassButton(
        key: ValueKey(key),
        label: label,
        icon: icon,
        style: visiblePage == page ? GlassButtonStyle.prominent : GlassButtonStyle.glass,
        size: _automationControlSize(context),
        onPressed: select,
      ),
    );
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (_agentProjectUnavailable)
          Padding(
            padding: const EdgeInsets.all(12),
            child: InfoBanner(tone: BannerTone.warning, message: l.hostAgentProjectUnavailable),
          ),
        Padding(
          padding: const EdgeInsets.fromLTRB(12, 8, 12, 0),
          child: Wrap(
            spacing: 8,
            runSpacing: 8,
            children: [
              navigation('automation-workspace-projects', 0, Icons.folder_open, l.automationProjects, _showProjects),
              if (_acceptedAgentFocus case final focus?)
                GlassButton(
                  key: const ValueKey('automation-return-agent-host'),
                  label: l.hostAgentReturn(focus.hostName),
                  icon: Icons.arrow_back,
                  onPressed: _agentCurrent(focus) ? () => _returnToAgentHost(focus) : null,
                ),
              if (jobs != null)
                navigation(
                  'automation-workspace-jobs',
                  1,
                  Icons.history,
                  l.automationJobs,
                  () => setState(() => workspacePage = 1),
                ),
              if (awxAvailable)
                navigation(
                  'automation-workspace-awx',
                  3,
                  Icons.cloud_outlined,
                  l.awxTab,
                  () => setState(() => workspacePage = 3),
                ),
              if (runComposer != null)
                navigation(
                  'automation-workspace-run',
                  2,
                  Icons.play_arrow,
                  l.automationRunSetup,
                  () => setState(() => workspacePage = 2),
                ),
            ],
          ),
        ),
        Expanded(
          child: LayoutBuilder(
            builder: (context, box) {
              final maxHeight = math.max(120.0, math.min(420.0, box.maxHeight * .55));
              final panelHeight = consoleCollapsed
                  ? AutomationExecutionConsole.collapsedHeight
                  : consoleHeight.clamp(120.0, maxHeight);
              return Column(
                children: [
                  Expanded(
                    child: IndexedStack(
                      index: visiblePage,
                      children: [
                        _buildProjects(context),
                        if (jobs != null)
                          AutomationJobsDialog(
                            key: const ValueKey('automation-history'),
                            scope: widget.scope,
                            service: jobs,
                            embedded: true,
                            active: workspacePage == 1,
                            reviewEpoch: jobsReviewEpoch,
                            onClose: _showProjects,
                            onJobSelected: _openExecution,
                            onJobSubmitted: _acceptedJob,
                            onRepairRequested: widget.service is AutomationAiService ? _repairJob : null,
                          )
                        else
                          const SizedBox.shrink(),
                        runComposer ?? const SizedBox.shrink(),
                        if (awxAvailable)
                          ValueListenableBuilder(
                            valueListenable: ref.watch(routerProvider).routeInformationProvider,
                            builder: (context, route, _) => AutomationAwxEntry(
                              scope: widget.scope,
                              service: widget.service,
                              events: awxEvents,
                              portFactory: awxFactory,
                              active: visiblePage == 3 && route.uri.path == AppRoutes.automation,
                            ),
                          )
                        else
                          const SizedBox.shrink(),
                      ],
                    ),
                  ),
                  if (executionJob != null && jobs != null) ...[
                    if (!consoleCollapsed)
                      Semantics(
                        label: l.automationExecutionResize,
                        child: MouseRegion(
                          cursor: SystemMouseCursors.resizeRow,
                          child: GestureDetector(
                            key: const ValueKey('automation-console-resize'),
                            behavior: HitTestBehavior.opaque,
                            onVerticalDragUpdate: (details) =>
                                setState(() => consoleHeight = (panelHeight - details.delta.dy).clamp(120, maxHeight)),
                            child: SizedBox(
                              height: 8,
                              child: Center(
                                child: Container(
                                  width: 36,
                                  height: 3,
                                  decoration: BoxDecoration(
                                    color: Theme.of(context).colorScheme.outline,
                                    borderRadius: BorderRadius.circular(2),
                                  ),
                                ),
                              ),
                            ),
                          ),
                        ),
                      ),
                    SizedBox(
                      key: const ValueKey('automation-execution-panel'),
                      height: panelHeight,
                      child: AutomationExecutionConsole(
                        key: ValueKey(executionJob),
                        scope: widget.scope,
                        service: jobs,
                        jobId: executionJob!,
                        collapsed: consoleCollapsed,
                        onToggle: () => setState(() => consoleCollapsed = !consoleCollapsed),
                        onHistory: () => setState(() => workspacePage = 1),
                        onClose: () => setState(() => executionJob = null),
                      ),
                    ),
                  ],
                ],
              );
            },
          ),
        ),
      ],
    );
  }

  Widget _buildProjects(BuildContext context) {
    final l = context.l10n;
    final focusedContext = _acceptedProjectFocus, focusedProject = _focusedProject;
    final catalog = ref.watch(automationProjectsProvider);
    final runnerService = ref.watch(automationRunnerServiceProvider);
    final portableAvailable =
        widget.service is AutomationPortableGatewayCapability && ref.watch(criticalRuntimeEventsProvider) != null;
    final menuProject = project;
    final menuRequest = request;
    bool projectMenuCurrent() =>
        current &&
        !busy &&
        project?.readOnly == false &&
        project?.id == menuProject?.id &&
        project?.revision == menuProject?.revision &&
        request == menuRequest;
    return Padding(
      padding: const EdgeInsets.all(12),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Wrap(
            key: const ValueKey('automation-project-toolbar'),
            spacing: 8,
            runSpacing: 8,
            crossAxisAlignment: WrapCrossAlignment.center,
            children: [
              Text(l.automationTitle, style: Theme.of(context).textTheme.titleLarge),
              if (ref.watch(automationJobServiceProvider) case final jobService?) ...[
                GlassButton.prominent(
                  key: const ValueKey('automation-open-run'),
                  label: l.automationRun,
                  icon: Icons.play_arrow,
                  size: _automationControlSize(context),
                  onPressed: busy || project == null ? null : () => _run(jobService),
                ),
              ],
              _AutomationActionMenu(
                triggerKey: const ValueKey('automation-project-actions'),
                label: l.automationProjectMenu,
                icon: Icons.expand_more,
                enabled: !busy && catalog.asData?.value?.readOnly != true,
                entries: [
                  _automationMenuItem(
                    key: const ValueKey('automation-new-project'),
                    value: 'new',
                    label: l.automationNewProject,
                    icon: Icons.add,
                  ),
                  _automationMenuItem(
                    key: const ValueKey('automation-open-folder'),
                    value: 'folder',
                    label: l.automationOpenFolder,
                    icon: Icons.folder_open,
                  ),
                  if (portableAvailable)
                    _automationMenuItem(
                      key: const ValueKey('automation-portable-inspect-open'),
                      value: 'portable-inspect',
                      label: l.automationPortableInspectAction,
                      icon: Icons.inventory_2_outlined,
                    ),
                  if (portableAvailable && widget.service is AutomationPortableImportReviewCapability)
                    _automationMenuItem(
                      key: const ValueKey('automation-portable-import-open'),
                      value: 'portable-import',
                      label: l.automationPortableImportTitle,
                      icon: Icons.create_new_folder_outlined,
                    ),
                  if (widget.service is AutomationGitService)
                    _automationMenuItem(
                      key: const ValueKey('automation-git-open'),
                      value: 'git',
                      label: l.automationGitImport,
                      icon: Icons.download_outlined,
                    ),
                ],
                onSelected: (value) {
                  if (!current || busy || ref.read(automationProjectsProvider).value?.readOnly == true) return;
                  if (value == 'new') _create();
                  if (value == 'folder') _create(folder: true);
                  if (value == 'portable-inspect') _portablePackage(inspect: true);
                  if (value == 'portable-import') _portablePackage(inspect: true, createProject: true);
                  if (value == 'git' && widget.service is AutomationGitService) {
                    _git(widget.service as AutomationGitService);
                  }
                },
              ),
              if (runnerService != null ||
                  widget.service is AutomationInventoryService ||
                  widget.service is AutomationDependencyService ||
                  widget.service is AutomationSourceInventoryService)
                _AutomationActionMenu(
                  triggerKey: const ValueKey('automation-settings-menu'),
                  label: l.settingsTitle,
                  icon: Icons.expand_more,
                  entries: [
                    if (runnerService != null)
                      _automationMenuItem(
                        key: const ValueKey('automation-runners-open'),
                        value: 'runners',
                        label: l.automationRunners,
                        icon: Icons.settings_ethernet,
                      ),
                    if (widget.service is AutomationDependencyService)
                      _automationMenuItem(
                        key: const ValueKey('automation-dependencies-open'),
                        value: 'dependencies',
                        label: l.automationDependencyTitle,
                        icon: Icons.extension_outlined,
                        enabled: projectMenuCurrent(),
                      ),
                    if (widget.service is AutomationProjectCredentialsService)
                      _automationMenuItem(
                        key: const ValueKey('automation-project-credentials-open'),
                        value: 'credentials',
                        label: l.automationDeploymentTitle,
                        icon: Icons.key_outlined,
                        enabled: projectMenuCurrent(),
                      ),
                    if (widget.service is AutomationProjectMetadataService)
                      _automationMenuItem(
                        key: const ValueKey('automation-project-metadata-open'),
                        value: 'metadata',
                        label: l.automationMetadataTitle,
                        icon: Icons.sync_outlined,
                        enabled: projectMenuCurrent(),
                      ),
                    if (widget.service is AutomationInventoryService)
                      _automationMenuItem(
                        key: const ValueKey('automation-groups-open'),
                        value: 'groups',
                        label: l.automationGroups,
                        icon: Icons.account_tree_outlined,
                        enabled: projectMenuCurrent(),
                      ),
                    if (widget.service is AutomationSourceInventoryService)
                      _automationMenuItem(
                        key: const ValueKey('automation-inventory-open'),
                        value: 'inventory',
                        label: l.automationInventorySources,
                        icon: Icons.playlist_add_check,
                        enabled: projectMenuCurrent(),
                      ),
                  ],
                  onSelected: (value) {
                    if (!current) return;
                    if (value == 'runners' && runnerService != null) {
                      _dialog<void>((_) => AutomationRunnersDialog(scope: widget.scope, service: runnerService));
                    }
                    if (!projectMenuCurrent()) return;
                    if (value == 'dependencies' && widget.service is AutomationDependencyService) {
                      _dependencies(widget.service as AutomationDependencyService);
                    }
                    if (value == 'credentials' && widget.service is AutomationProjectCredentialsService) {
                      _credentials(widget.service as AutomationProjectCredentialsService);
                    }
                    if (value == 'metadata' && widget.service is AutomationProjectMetadataService) {
                      _metadata(widget.service as AutomationProjectMetadataService);
                    }
                    if (value == 'groups' && widget.service is AutomationInventoryService) {
                      _groups(widget.service as AutomationInventoryService);
                    }
                    if (value == 'inventory' && widget.service is AutomationSourceInventoryService) {
                      _inventory(widget.service as AutomationSourceInventoryService);
                    }
                  },
                ),
            ],
          ),
          const SizedBox(height: 8),
          Text(l.automationWorkflowHint),
          if (project?.gitSource case final Map<String, dynamic> source)
            Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                SelectableText /* l10n-ignore: Git reference and commit identity */ (
                  'Git · ${source['reference']} · ${source['commit']}',
                ), // l10n-ignore: Git reference and commit data
                if (dirty ||
                    (source['snapshot_digest'] != null &&
                        revision != null &&
                        source['snapshot_digest'] != revision!.digest))
                  Text(l.automationGitLocalChanges),
              ],
            ),
          if (error != null)
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 8),
              child: SelectableText(
                error is _AiDraftStale
                    ? l.automationAiDraftStale
                    : error is AutomationFileConflict
                    ? l.automationFileChanged
                    : error is AutomationAiDraftException
                    ? automationAiDraftErrorMessage(l, error as AutomationAiDraftException)
                    : error is FormatException
                    ? (error as FormatException).message
                    : errorMessage(l, error!),
                style: TextStyle(color: Theme.of(context).colorScheme.error),
              ),
            ),
          if (requirementsSavedPending)
            Padding(padding: const EdgeInsets.all(8), child: Text(l.automationAiRequirementsPartial)),
          if (busy) const LinearProgressIndicator(minHeight: 2),
          const SizedBox(height: 8),
          Expanded(
            child: LayoutBuilder(
              builder: (context, constraints) {
                // 210px navigation + 10px divider + 360px editor fit side by side.
                final compact = constraints.maxWidth < 580;
                final tree = ContentSurface(
                  key: const ValueKey('automation-project-tree'),
                  padding: EdgeInsets.zero,
                  child: ListView(
                    children: [
                      Padding(padding: const EdgeInsets.all(12), child: Text(l.automationProjects)),
                      if (_contextError)
                        InfoBanner(
                          key: const ValueKey('automation-context-error'),
                          message: l.correlationProjectChanged,
                          tone: BannerTone.danger,
                        ),
                      if (focusedContext != null && focusedProject != null) ...[
                        InfoBanner(message: l.correlationProjectFocusHint),
                        ListTile(
                          key: ValueKey('automation-context-project-${focusedProject.id}'),
                          dense: true,
                          selected: true,
                          leading: const Icon(Icons.folder_outlined, size: 18),
                          title: Text(focusedProject.name ?? focusedProject.id),
                          trailing: GlassButton(
                            key: const ValueKey('automation-context-open'),
                            label: l.commonOpen,
                            onPressed: busy ? null : () => _openContextProject(focusedContext),
                          ),
                        ),
                        TextButton(
                          key: const ValueKey('automation-context-clear'),
                          onPressed: () => setState(() {
                            if (identical(_acceptedProjectFocus, focusedContext)) {
                              _contextGeneration++;
                              _acceptedProjectFocus = null;
                              _focusedProject = null;
                            }
                          }),
                          child: Text(l.commonClose),
                        ),
                      ] else
                        ...catalog.when(
                          data: (value) => [
                            if (value?.readOnly == true)
                              Padding(padding: const EdgeInsets.all(12), child: Text(l.automationNewerSchema)),
                            if (value?.projects.isEmpty ?? false)
                              Padding(padding: const EdgeInsets.all(12), child: Text(l.automationNoProjects)),
                            for (final item in value?.projects ?? <AutomationProject>[])
                              ListTile(
                                key: ValueKey('automation-project-${item.id}'),
                                dense: true,
                                selected: item.id == project?.id,
                                leading: const Icon(Icons.folder_outlined, size: 18),
                                title: Text(item.name ?? item.id, maxLines: 1, overflow: TextOverflow.ellipsis),
                                subtitle: item.importState == null
                                    ? null
                                    : Text(switch (item.importState) {
                                        'importing' => l.automationGitReceiving,
                                        'ready' => l.automationGitReady,
                                        'failed' => l.automationStateFailed,
                                        'cancelled' => l.automationStateCancelled,
                                        'interrupted' => l.automationStateInterrupted,
                                        _ => l.automationNewerSchema,
                                      }),
                                onTap: busy || item.readOnly ? null : () => _select(item),
                              ),
                          ],
                          error: (e, _) => [
                            Padding(padding: const EdgeInsets.all(12), child: Text(errorMessage(l, e))),
                            TextButton(
                              onPressed: () => ref.invalidate(automationProjectsProvider),
                              child: Text(l.commonRetry),
                            ),
                          ],
                          loading: () => [const Padding(padding: EdgeInsets.all(12), child: LinearProgressIndicator())],
                        ),
                      if (project != null) ...[
                        const Divider(),
                        Padding(
                          key: const ValueKey('automation-project-header'),
                          padding: const EdgeInsets.fromLTRB(12, 4, 4, 4),
                          child: Row(
                            children: [
                              Expanded(
                                child: Text(project!.name ?? project!.id, maxLines: 1, overflow: TextOverflow.ellipsis),
                              ),
                              _AutomationActionMenu(
                                triggerKey: const ValueKey('automation-create-menu'),
                                enabled: !busy && !project!.readOnly,
                                tooltip: l.commonCreate,
                                onSelected: (value) {
                                  if (!projectMenuCurrent()) return;
                                  if (value == 'playbook') {
                                    _newFile(playbook: true);
                                  } else if (value == 'role' && widget.service is AutomationScaffoldService) {
                                    _newRole(widget.service as AutomationScaffoldService);
                                  } else if (value == 'file') {
                                    _newFile();
                                  }
                                },
                                entries: [
                                  _automationMenuItem(
                                    key: const ValueKey('automation-create-playbook'),
                                    value: 'playbook',
                                    label: l.automationNewPlaybook,
                                  ),
                                  if (widget.service is AutomationScaffoldService)
                                    _automationMenuItem(
                                      key: const ValueKey('automation-create-role'),
                                      value: 'role',
                                      label: l.automationCreateRole,
                                    ),
                                  _automationMenuItem(
                                    key: const ValueKey('automation-create-file'),
                                    value: 'file',
                                    label: l.automationOtherFile,
                                  ),
                                ],
                                icon: Icons.add,
                              ),
                              _AutomationActionMenu(
                                triggerKey: const ValueKey('automation-project-menu'),
                                tooltip: l.glassMoreActions,
                                enabled: !busy && !project!.readOnly,
                                icon: Icons.more_horiz,
                                onSelected: (value) {
                                  if (!projectMenuCurrent()) return;
                                  if (value == 'rename') _rename();
                                  if (value == 'config') _ansibleConfig();
                                  if (value == 'reload') _select(project!);
                                  if (value == 'portable-export') _portablePackage(inspect: false);
                                  if (value == 'inventory-export' &&
                                      widget.service is AutomationInventoryExportService) {
                                    _exportInventory(widget.service as AutomationInventoryExportService);
                                  }
                                },
                                entries: [
                                  _automationMenuItem(
                                    key: const ValueKey('automation-rename-project'),
                                    value: 'rename',
                                    label: l.commonRename,
                                  ),
                                  _automationMenuItem(
                                    key: const ValueKey('automation-ansible-settings'),
                                    value: 'config',
                                    label: l.automationConfigTitle,
                                  ),
                                  _automationMenuItem(
                                    key: const ValueKey('automation-reload-project'),
                                    value: 'reload',
                                    label: l.commonRetry,
                                  ),
                                  if (portableAvailable)
                                    _automationMenuItem(
                                      key: const ValueKey('automation-portable-export-open'),
                                      value: 'portable-export',
                                      enabled: project!.revision != null,
                                      label: l.automationPortableExportAction,
                                    ),
                                  if (widget.service is AutomationInventoryExportService)
                                    _automationMenuItem(
                                      key: const ValueKey('automation-inventory-export-open'),
                                      value: 'inventory-export',
                                      enabled: project!.revision != null,
                                      label: l.automationInventoryExportAction,
                                    ),
                                ],
                              ),
                            ],
                          ),
                        ),
                        for (final entry in revision?.entries ?? <AutomationFileEntry>[])
                          if (entry.path != automationGitSourceFile)
                            ListTile(
                              key: ValueKey('automation-file-${entry.path}'),
                              dense: true,
                              selected: entry.path == (file?.path ?? newPath),
                              leading: Icon(
                                entry.directory ? Icons.folder_outlined : Icons.description_outlined,
                                size: 16,
                              ),
                              title: Text(entry.path, maxLines: 1, overflow: TextOverflow.ellipsis),
                              onTap: busy || entry.directory ? null : () => _read(entry.path),
                            ),
                      ],
                    ],
                  ),
                );
                final body = ContentSurface(
                  padding: const EdgeInsets.all(10),
                  child: file == null && newPath == null
                      ? Center(child: Text(l.automationChooseFile))
                      : Column(
                          crossAxisAlignment: CrossAxisAlignment.stretch,
                          children: [
                            Row(
                              children: [
                                Expanded(
                                  child: Text /* l10n-ignore: filename and dirty marker */ (
                                    '${file?.path ?? newPath}${dirty ? ' •' : ''}', // l10n-ignore: document path and dirty marker
                                    overflow: TextOverflow.ellipsis,
                                  ),
                                ),
                                GlassButton(
                                  key: const ValueKey('automation-save'),
                                  label: l.commonSave,
                                  icon: Icons.save_outlined,
                                  size: _automationControlSize(context),
                                  onPressed: !dirty || busy ? null : _save,
                                ),
                                if (!AppPlatform.isMobile &&
                                    file != null &&
                                    widget.service is AutomationEditorService) ...[
                                  IconButton(
                                    key: const ValueKey('automation-open-editor'),
                                    tooltip: l.automationOpenEditor,
                                    onPressed: busy
                                        ? null
                                        : () => _externalEditor(widget.service as AutomationEditorService),
                                    icon: const Icon(Icons.open_in_new, size: 18),
                                  ),
                                  IconButton(
                                    key: const ValueKey('automation-reload-file'),
                                    tooltip: l.automationReloadFile,
                                    onPressed: busy ? null : () => _read(file!.path),
                                    icon: const Icon(Icons.refresh, size: 18),
                                  ),
                                ],
                                if (widget.service case final AutomationAiService ai)
                                  Flexible(
                                    child: GlassButton(
                                      key: const ValueKey('automation-ai-open'),
                                      label: l.automationAiTitle,
                                      tooltip: l.automationAiTitle,
                                      size: _automationControlSize(context),
                                      onPressed:
                                          busy ||
                                              !((file?.path ?? newPath ?? '').endsWith('.yml') ||
                                                  (file?.path ?? newPath ?? '').endsWith('.yaml'))
                                          ? null
                                          : () => _ai(ai),
                                      icon: Icons.auto_awesome,
                                    ),
                                  ),
                              ],
                            ),
                            const SizedBox(height: 8),
                            Expanded(
                              child: Shortcuts(
                                shortcuts: const {
                                  SingleActivator(LogicalKeyboardKey.keyS, control: true): _SaveFileIntent(),
                                  SingleActivator(LogicalKeyboardKey.keyS, meta: true): _SaveFileIntent(),
                                },
                                child: Actions(
                                  actions: {
                                    _SaveFileIntent: CallbackAction<_SaveFileIntent>(
                                      onInvoke: (_) {
                                        if (!busy && dirty) _save();
                                        return null;
                                      },
                                    ),
                                  },
                                  child: AutomationSecretView(
                                    key: ValueKey('secrets-${project?.id}-${file?.path ?? newPath}'),
                                    controller: editor,
                                    builder: (display, masked) => AnsibleCodeEditor(
                                      key: ObjectKey(display),
                                      controller: display,
                                      onCopySelection: _copyEditorSelection,
                                      readOnly: busy || masked,
                                      path: file?.path ?? newPath,
                                      foldTooltip: (collapsed, count) => collapsed
                                          ? l.automationEditorExpandLines(count)
                                          : l.automationEditorCollapseLines(count),
                                      limitMessage: l.automationEditorLimit,
                                    ),
                                  ),
                                ),
                              ),
                            ),
                          ],
                        ),
                );
                if (compact) {
                  return Column(
                    children: [
                      SizedBox(height: 180, child: tree),
                      const SizedBox(height: 8),
                      Expanded(child: body),
                    ],
                  );
                }
                final width = sidebarWidth.clamp(210.0, (constraints.maxWidth - 360).clamp(210.0, 520.0));
                return Row(
                  children: [
                    SizedBox(width: width, child: tree),
                    MouseRegion(
                      cursor: SystemMouseCursors.resizeColumn,
                      child: GestureDetector(
                        key: const ValueKey('automation-project-resize'),
                        behavior: HitTestBehavior.opaque,
                        onHorizontalDragUpdate: (details) =>
                            setState(() => sidebarWidth = (width + details.delta.dx).clamp(210, 520)),
                        child: const SizedBox(width: 10, height: double.infinity, child: VerticalDivider(width: 1)),
                      ),
                    ),
                    Expanded(child: body),
                  ],
                );
              },
            ),
          ),
        ],
      ),
    );
  }
}

GlassControlSize _automationControlSize(BuildContext context) {
  final style = GlassTokens.of(context).typography.button;
  final line = MediaQuery.textScalerOf(context).scale(style.fontSize!) * (style.height ?? 1);
  return GlassControlSize.values.firstWhere((size) => size.height >= line + 4, orElse: () => GlassControlSize.xl);
}

PopupMenuItem<String> _automationMenuItem({
  required String value,
  required String label,
  Key? key,
  IconData? icon,
  bool enabled = true,
}) => PopupMenuItem<String>(
  key: key,
  value: value,
  enabled: enabled,
  height: AppPlatform.isMobile ? 48 : 32,
  padding: const EdgeInsets.symmetric(horizontal: 12),
  child: Row(
    mainAxisSize: MainAxisSize.min,
    children: [
      if (icon != null) ...[Icon(icon, size: 16), const SizedBox(width: 8)],
      Flexible(child: Text(label, maxLines: 2, overflow: TextOverflow.ellipsis)),
    ],
  ),
);

/// Compact desktop menu rows retain Material keyboard navigation and can grow
/// for larger accessibility text, while triggers use the existing glass kit.
class _AutomationActionMenu extends StatelessWidget {
  const _AutomationActionMenu({
    required this.triggerKey,
    required this.entries,
    required this.onSelected,
    required this.icon,
    this.label,
    this.tooltip,
    this.enabled = true,
  });
  final Key triggerKey;
  final List<PopupMenuEntry<String>> entries;
  final ValueChanged<String> onSelected;
  final IconData icon;
  final String? label, tooltip;
  final bool enabled;
  Future<void> open(BuildContext context) async {
    final overlay = Overlay.of(context).context.findRenderObject()! as RenderBox;
    final button = context.findRenderObject()! as RenderBox;
    final offset = button.localToGlobal(Offset(0, button.size.height), ancestor: overlay);
    final maxWidth = math.min(420.0, MediaQuery.sizeOf(context).width - 32);
    final tokens = GlassTokens.of(context);
    final choice = await showMenu<String>(
      context: context,
      position: RelativeRect.fromRect(offset & Size(button.size.width, 0), Offset.zero & overlay.size),
      items: entries,
      constraints: BoxConstraints(minWidth: math.min(220, maxWidth), maxWidth: maxWidth),
      color: tokens.surfaces.contentSolid,
      surfaceTintColor: Colors.transparent,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(tokens.radii.menu),
        side: BorderSide(color: tokens.surfaces.hairlineCard),
      ),
      menuPadding: const EdgeInsets.symmetric(vertical: 6),
      requestFocus: true,
    );
    if (context.mounted && choice != null) onSelected(choice);
  }

  @override
  Widget build(BuildContext context) => Builder(
    builder: (context) => label == null
        ? GlassIconButton(
            key: triggerKey,
            icon: icon,
            size: 32,
            iconSize: 18,
            tooltip: tooltip!,
            onPressed: enabled ? () => open(context) : null,
          )
        : GlassButton(
            key: triggerKey,
            label: label!,
            icon: icon,
            size: _automationControlSize(context),
            tooltip: tooltip ?? label,
            onPressed: enabled ? () => open(context) : null,
          ),
  );
}

class _AiDraftStale implements Exception {
  const _AiDraftStale();
}

class _SaveFileIntent extends Intent {
  const _SaveFileIntent();
}

class _CadvisorInstallDialog extends StatefulWidget {
  const _CadvisorInstallDialog({required this.owner});
  final String owner;
  @override
  State<_CadvisorInstallDialog> createState() => _CadvisorInstallDialogState();
}

class _CadvisorInstallDialogState extends State<_CadvisorInstallDialog> {
  final image = TextEditingController(), imageId = TextEditingController();
  final port = TextEditingController(text: '19101');
  late final owner = TextEditingController(text: widget.owner);
  final containerId = TextEditingController();
  AutomationCadvisorOperation operation = AutomationCadvisorOperation.install;
  bool reviewed = false, invalid = false;
  @override
  void dispose() {
    image.dispose();
    imageId.dispose();
    port.dispose();
    owner.dispose();
    containerId.dispose();
    super.dispose();
  }

  void submit() {
    try {
      final options = AutomationCadvisorInstallOptions(
        imageSha256: image.text.trim(),
        imageId: imageId.text.trim(),
        owner: owner.text.trim(),
        port: int.tryParse(port.text) ?? 0,
        hostAccessReviewed: reviewed,
        operation: operation,
        containerId: operation == AutomationCadvisorOperation.install ? null : containerId.text.trim(),
      );
      Navigator.pop(context, options);
    } on FormatException {
      setState(() => invalid = true);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return AlertDialog(
      key: const ValueKey('automation-cadvisor-install-options'),
      title: Text(l.automationCadvisorLifecycleTitle),
      content: SizedBox(
        width: 620,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(l.automationCadvisorLifecycleDraft),
              GlassSelect<AutomationCadvisorOperation>(
                key: const ValueKey('cadvisor-lifecycle-operation'),
                value: operation,
                expand: true,
                semanticLabel: l.automationCadvisorLifecycleOperation,
                items: [
                  for (final value in AutomationCadvisorOperation.values)
                    GlassSelectItem(
                      value: value,
                      label: switch (value) {
                        AutomationCadvisorOperation.inspect => l.automationCadvisorLifecycleInspect,
                        AutomationCadvisorOperation.install => l.automationCadvisorLifecycleInstall,
                        AutomationCadvisorOperation.start => l.automationCadvisorLifecycleStart,
                        AutomationCadvisorOperation.stop => l.automationCadvisorLifecycleStop,
                        AutomationCadvisorOperation.remove => l.automationCadvisorLifecycleRemove,
                      },
                    ),
                ],
                onChanged: (value) => setState(() {
                  operation = value;
                  reviewed = false;
                  invalid = false;
                }),
              ),
              const SizedBox(height: 12),
              InfoBanner(tone: BannerTone.warning, message: l.automationCadvisorHostAccess),
              Text(l.automationCadvisorDockerAccess),
              const SizedBox(height: 12),
              TextField(
                key: const ValueKey('cadvisor-lifecycle-owner'),
                controller: owner,
                maxLength: 36,
                decoration: InputDecoration(labelText: l.automationCadvisorLifecycleOwner),
              ),
              if (operation != AutomationCadvisorOperation.install)
                TextField(
                  key: const ValueKey('cadvisor-lifecycle-container'),
                  controller: containerId,
                  maxLength: 64,
                  decoration: InputDecoration(labelText: l.automationCadvisorLifecycleContainer),
                ),
              TextField(
                key: const ValueKey('cadvisor-install-image-digest'),
                controller: image,
                maxLength: 64,
                decoration: InputDecoration(labelText: l.automationCadvisorInstallImageDigest),
              ),
              TextField(
                key: const ValueKey('cadvisor-install-image-id'),
                controller: imageId,
                maxLength: 71,
                decoration: InputDecoration(labelText: l.automationCadvisorInstallImageId),
              ),
              TextField(
                key: const ValueKey('cadvisor-install-port'),
                controller: port,
                maxLength: 5,
                keyboardType: TextInputType.number,
                decoration: InputDecoration(labelText: l.automationCadvisorInstallPort),
              ),
              CheckboxListTile(
                key: const ValueKey('cadvisor-install-consent'),
                value: reviewed,
                title: Text(l.automationCadvisorInstallConsent),
                onChanged: (value) => setState(() => reviewed = value == true),
              ),
              if (invalid) Text(l.automationCadvisorInstallInvalid),
            ],
          ),
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context), child: Text(l.commonCancel)),
        TextButton(
          key: const ValueKey('cadvisor-install-compose'),
          onPressed: reviewed ? submit : null,
          child: Text(l.automationCadvisorInstallContinue),
        ),
      ],
    );
  }
}

class _NameDialog extends StatefulWidget {
  const _NameDialog({required this.title, required this.initial});
  final String title, initial;
  @override
  State<_NameDialog> createState() => _NameDialogState();
}

class _NameDialogState extends State<_NameDialog> {
  late final controller = TextEditingController(text: widget.initial);
  @override
  void dispose() {
    controller.dispose();
    super.dispose();
  }

  void submit() {
    if (controller.text.trim().isNotEmpty) Navigator.pop(context, controller.text.trim());
  }

  @override
  Widget build(BuildContext context) => AlertDialog(
    title: Text(widget.title),
    content: SizedBox(
      width: 380,
      child: TextField(controller: controller, autofocus: true, maxLength: 120, onSubmitted: (_) => submit()),
    ),
    actions: [
      TextButton(onPressed: () => Navigator.pop(context), child: Text(context.l10n.commonCancel)),
      TextButton(onPressed: submit, child: Text(context.l10n.commonSave)),
    ],
  );
}
