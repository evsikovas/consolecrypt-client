import 'dart:async';
import 'dart:convert';
import 'dart:io' show File, FileSystemEntity, FileSystemEntityType;
import 'dart:math' as math;

import 'package:consolecrypt/app/critical_notice_owner.dart';
import 'package:consolecrypt/app/critical_notice_providers.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/automation/ansible_code_editor.dart';
import 'package:consolecrypt/automation/automation_ai_dialog.dart';
import 'package:consolecrypt/automation/automation_ai_draft.dart';
import 'package:consolecrypt/automation/automation_ai_draft_errors.dart';
import 'package:consolecrypt/automation/automation_ansible_config.dart';
import 'package:consolecrypt/automation/automation_dependency_dialog.dart';
import 'package:consolecrypt/automation/automation_dependency_models.dart';
import 'package:consolecrypt/automation/automation_execution_console.dart';
import 'package:consolecrypt/automation/automation_file_tree.dart';
import 'package:consolecrypt/automation/automation_git_dialog.dart';
import 'package:consolecrypt/automation/automation_inventory_dialog.dart';
import 'package:consolecrypt/automation/automation_inventory_export.dart';
import 'package:consolecrypt/automation/automation_inventory_export_dialog.dart';
import 'package:consolecrypt/automation/automation_job_provenance.dart';
import 'package:consolecrypt/automation/automation_jobs_dialog.dart';
import 'package:consolecrypt/automation/automation_library_files.dart';
import 'package:consolecrypt/automation/automation_new_project_dialog.dart';
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
import 'package:consolecrypt/automation/automation_schedules_view.dart';
import 'package:consolecrypt/automation/automation_scope_guard.dart';
import 'package:consolecrypt/automation/automation_secret_view.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/automation/automation_vault_file_dialog.dart';
import 'package:consolecrypt/automation/automation_versions_dialog.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_yaml_editor.dart';
import 'package:consolecrypt/kubernetes/kube_yaml_syntax.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class AutomationScreen extends ConsumerWidget {
  const AutomationScreen({super.key});
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final scope = ref.watch(automationScopeProvider);
    final service = ref.watch(automationServiceProvider);
    if (!scope.unlocked) return const SizedBox.shrink();
    if (service == null) return Center(child: Text(context.l10n.automationUnavailable));
    // Replacing this subtree disposes plaintext editors and invalidates pending
    // reads on every profile/unlock generation, including a rapid lock/unlock.
    return _ProjectWorkspace(key: ObjectKey(scope), scope: scope, service: service);
  }
}

class _ProjectWorkspace extends ConsumerStatefulWidget {
  const _ProjectWorkspace({super.key, required this.scope, required this.service});
  final AutomationScope scope;
  final AutomationService service;
  @override
  ConsumerState<_ProjectWorkspace> createState() => _ProjectWorkspaceState();
}

class _ProjectWorkspaceState extends ConsumerState<_ProjectWorkspace> {
  static const _executionPage = 4;
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
  static const _fileTreeMinWidth = 184.0;
  double sidebarWidth = 200;
  bool fileTreeCollapsed = false;
  int request = 0;
  VoidCallback? _retireExternalEditor;
  int workspacePage = 0;
  String libraryQuery = '';
  bool projectsExpanded = false;
  bool _showPhoneEditor = false;
  Widget? runComposer;
  int jobsReviewEpoch = 0;
  String? executionJob;
  bool consoleCollapsed = false;
  double consoleHeight = 260;
  bool get current => mounted && identical(ref.read(automationScopeProvider), widget.scope);
  bool get _phoneWorkspace => AppPlatform.isMobile && MediaQuery.sizeOf(context).width < 600;
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

  Future<void> _run(AutomationJobService service, {AutomationJson? initialExtraVars, bool scheduleOnly = false}) async {
    if (busy || project == null) return;
    if (dirty && !await _save()) return;
    if (!current) return;
    final paths = [
      for (final entry in revision?.entries ?? <AutomationFileEntry>[])
        if (!entry.directory &&
            RegExp(r'\.(yml|yaml)$', caseSensitive: false).hasMatch(entry.path) &&
            ![
              'roles',
              'inventory',
              'templates',
              'files',
              'host_vars',
              'group_vars',
            ].contains(entry.path.split('/').first))
          entry.path,
    ];
    setState(() {
      workspacePage = 2;
      runComposer = AutomationRunDialog(
        scope: widget.scope,
        service: service,
        project: project!,
        playbooks: paths,
        initialPlaybook: !scheduleOnly && paths.contains(file?.path) ? file?.path : null,
        scheduleOnly: scheduleOnly,
        initialExtraVars: initialExtraVars,
        retryOf: scheduleOnly ? null : repairForCurrent,
        embedded: true,
        onClose: scheduleOnly ? _showSchedules : _showProjects,
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
      if (_phoneWorkspace) {
        workspacePage = _executionPage;
      } else if (workspacePage == 2 || workspacePage == 1) {
        workspacePage = 0;
      }
    });
  }

  void _openExecution(String id) => setState(() {
    executionJob = id;
    consoleCollapsed = false;
    workspacePage = _phoneWorkspace ? _executionPage : 0;
  });

  void _showProjects() => setState(() {
    workspacePage = 0;
    // Returning to editable files invalidates every prepared review, including retries.
    runComposer = null;
    jobsReviewEpoch++;
  });

  void _showSchedules() => setState(() {
    workspacePage = 3;
    runComposer = null;
  });

  Future<void> _backToPhoneFiles() async {
    FocusManager.instance.primaryFocus?.unfocus();
    if (busy || !await _leaveFile() || !current) return;
    setState(() {
      _showPhoneEditor = false;
      if (dirty) {
        newPath = null;
        aiDraft = null;
        requirementsSavedPending = false;
        editor.text = file?.text ?? '';
        dirty = false;
      }
    });
  }

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

  Future<void> _versions(AutomationProjectVersionService service) async {
    final selected = project;
    if (!current || busy || selected == null || selected.readOnly || revision == null) return;
    final source = widget.service, scope = widget.scope, app = ref.read(appServicesProvider);
    final runtime = ref.read(criticalRuntimeEventsProvider), router = ref.read(routerProvider);
    final epoch = runtime?.currentScope.epoch, uri = router.routeInformationProvider.value.uri;
    final live = ValueNotifier(true);
    var token = request, retired = false, selecting = false;
    bool matches() =>
        current &&
        (selecting || request == token) &&
        identical(widget.service, source) &&
        identical(ref.read(automationServiceProvider), source) &&
        identical(widget.scope, scope) &&
        identical(ref.read(appServicesProvider), app) &&
        identical(ref.read(criticalRuntimeEventsProvider), runtime) &&
        runtime?.currentScope.epoch == epoch &&
        runtime?.currentScope.streamUnavailable != true &&
        (selecting || project?.id == selected.id && project?.revision == selected.revision) &&
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
      if (!original() ||
          !await _leaveFile(
            originalCurrent: original,
            originalLive: live,
            discardLabel: context.l10n.automationPortableUseSaved,
            ownSaveRequest: (next) => token = next,
          ) ||
          !original()) {
        return;
      }
      final digest = revision?.digest;
      if (digest == null || !original()) return;
      final opened = await _dialog<AutomationProject>(
        (_) => DependencyOriginalGuard(
          original: live,
          builder: (_) =>
              AutomationVersionsDialog(project: selected, digest: digest, service: service, originalCurrent: original),
        ),
      );
      if (!original() || opened == null) return;
      ref.invalidate(automationProjectsProvider);
      selecting = true; // _select owns its new request token and selected-project identity.
      await _select(opened, originalCurrent: original);
    } finally {
      retired = true;
      for (final sub in subscriptions) {
        sub.close();
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
          automationApprovedJobHostIds(
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
            automationApprovedJobHostIds(
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
      projectsExpanded = false;
      _showPhoneEditor = false;
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
          _showPhoneEditor = true;
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
      final editor = ref.read(settingsServiceProvider).currentLocal.sftpDefaultEditor;
      final application =
          editor?.value ??
          await picker.chooseApplication(
            label: labels.sftpApplications,
            confirmButtonText: labels.sftpChooseApplication,
          );
      if (!original() || application == null) return;
      await service.openProjectEditor(
        selected.id,
        path,
        expectedSha256: sha,
        editor: {'kind': editor?.kind.wireName ?? 'path', 'value': application},
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

  Future<void> _create({
    bool folder = false,
    bool Function()? originalCurrent,
    ValueListenable<bool>? originalLive,
  }) async {
    if (busy ||
        originalCurrent?.call() == false ||
        !await _leaveFile(originalCurrent: originalCurrent, originalLive: originalLive) ||
        !current ||
        !mounted ||
        originalCurrent?.call() == false) {
      return;
    }
    final picker = ref.read(fileDialogServiceProvider);
    final source = widget.service;
    final scaffold = !folder && source is AutomationNewProjectService ? source as AutomationNewProjectService : null;
    final options = scaffold == null
        ? null
        : await _dialog<NewAutomationProject>(
            (_) => AutomationNewProjectDialog(chooseDirectory: picker.chooseDirectory),
          );
    if (!mounted || !current || (scaffold != null && options == null)) return;
    String? directory;
    try {
      directory = folder ? await picker.chooseDirectory() : options?.directory;
    } catch (_) {
      if (mounted && current) setState(() => error = FormatException(context.l10n.automationProjectChooseFolderError));
      return;
    }
    if (!mounted || !current || originalCurrent?.call() == false || (folder && directory == null)) return;
    final parts = directory?.replaceAll('\\', '/').split('/').where((part) => part.isNotEmpty).toList();
    final name =
        options?.name ??
        (folder && parts!.isNotEmpty
            ? String.fromCharCodes(parts.last.runes.take(120))
            : await _dialog<String>(
                (context) => DependencyOriginalGuard(
                  original: originalLive,
                  builder: (_) => _NameDialog(title: context.l10n.automationNewProject, initial: ''),
                ),
              ));
    if (!mounted || !current || originalCurrent?.call() == false || name == null) return;
    setState(() {
      busy = true;
      error = null;
    });
    AutomationProject? created;
    Object? creationError;
    final folderNotEmpty = context.l10n.automationProjectChooseFolderEmpty;
    final originalService = widget.service, originalScope = widget.scope;
    bool original() =>
        current &&
        originalCurrent?.call() != false &&
        identical(widget.service, originalService) &&
        identical(widget.scope, originalScope);
    try {
      if (!original()) return;
      created = scaffold == null
          ? await originalService.createProject(name: name, directory: directory)
          : await scaffold.scaffoldProject(name: name, directory: directory, role: options?.role);
    } catch (e) {
      creationError = switch (e) {
        AppException(:final args)
            when args['field'] == 'project_source' && args['rule'] == 'project_folder_not_empty' =>
          folderNotEmpty,
        _ => e,
      };
      if (original()) setState(() => error = creationError);
    } finally {
      if (current) setState(() => busy = false);
    }
    if (!original()) return;
    ref.invalidate(automationProjectsProvider);
    if (created != null) await _select(created, originalCurrent: original);
    if (original() && creationError != null) setState(() => error = creationError);
  }

  Future<void> _rename([AutomationProject? item]) async {
    // Project-source metadata is retained when only the display name changes.
    final selected = item ?? project;
    if (selected == null || busy || selected.readOnly || selected.revision == null) return;
    final token = request;
    final service = widget.service;
    final name = await _name(context.l10n.commonRename, initial: selected.name ?? '');
    if (name == null || !current || busy || request != token || !identical(widget.service, service)) return;
    setState(() {
      busy = true;
      error = null;
    });
    try {
      final result = await service.renameProject(selected.id, revision: selected.revision!, name: name);
      if (current) {
        if (project?.id == selected.id) setState(() => project = result);
        ref.invalidate(automationProjectsProvider);
      }
    } catch (e) {
      if (current) setState(() => error = e);
    } finally {
      if (current) setState(() => busy = false);
    }
  }

  Future<void> _projectEntryMenu(AutomationProject item, Offset position) async {
    if (busy || !current) return;
    final l = context.l10n;
    final service = widget.service;
    var token = request;
    bool valid() => current && request == token && identical(widget.service, service);
    final overlay = Overlay.of(context).context.findRenderObject()! as RenderBox;
    final offset = overlay.globalToLocal(position);
    final editable = !item.readOnly && item.revision != null;
    final action = await showMenu<String>(
      context: context,
      position: RelativeRect.fromSize(offset & Size.zero, overlay.size),
      items: [
        _automationMenuItem(value: 'open', label: l.commonOpen, icon: Icons.folder_open, enabled: !item.readOnly),
        if (item.path?.isNotEmpty == true)
          _automationMenuItem(value: 'copy', label: l.automationProjectCopyFolder, icon: Icons.copy_outlined),
        if (editable) _automationMenuItem(value: 'rename', label: l.commonRename, icon: Icons.edit_outlined),
        if (editable && service is AutomationProjectRemovalService)
          _automationMenuItem(value: 'remove', label: l.automationProjectRemove, icon: Icons.delete_outline),
      ],
    );
    if (!valid() || busy) return;
    switch (action) {
      case 'open':
        if (project?.id != item.id) await _select(item);
      case 'copy':
        if (!mounted) return;
        await copyPrivateWithNotice(context, ref, item.path!);
      case 'rename':
        await _rename(item);
      case 'remove':
        final accepted = await _dialog<bool>(
          (context) => AlertDialog(
            title: Text(l.automationProjectRemove),
            content: Text(l.automationProjectRemoveConfirm(item.name ?? item.id)),
            actions: [
              TextButton(onPressed: () => Navigator.pop(context, false), child: Text(l.commonCancel)),
              FilledButton(onPressed: () => Navigator.pop(context, true), child: Text(l.automationProjectRemove)),
            ],
          ),
        );
        if (accepted != true || !valid() || busy) return;
        if (project?.id == item.id &&
            !await _leaveFile(originalCurrent: valid, ownSaveRequest: (next) => token = next)) {
          return;
        }
        if (!valid() || busy) return;
        setState(() {
          busy = true;
          error = null;
        });
        try {
          await (service as AutomationProjectRemovalService).removeProject(item.id, revision: item.revision!);
          if (!valid()) return;
          if (project?.id == item.id) {
            setState(() {
              project = null;
              revision = null;
              file = null;
              newPath = null;
              aiDraft = null;
              repairOrigin = null;
              requirementsSavedPending = false;
              dirty = false;
              editor.clear();
            });
          }
          ref.invalidate(automationProjectsProvider);
        } catch (failure) {
          if (valid()) setState(() => error = failure);
        } finally {
          if (valid()) setState(() => busy = false);
        }
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
      (_) => AutomationAnsibleConfigDialog(text: original?.text ?? '', hasFile: original != null),
    );
    if (!current || token != request || project?.id != id || result == null) return;
    setState(() {
      file = original;
      _showPhoneEditor = true;
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

  Future<void> _vaultFile() async {
    final originalProject = project, originalPath = file?.path, service = widget.service;
    if (!current ||
        busy ||
        originalProject == null ||
        originalProject.readOnly ||
        originalProject.revision == null ||
        originalPath == null ||
        service is! AutomationVaultFileService ||
        !(originalPath.endsWith('.yml') || originalPath.endsWith('.yaml'))) {
      return;
    }
    final router = ref.read(routerProvider), route = ref.read(routerProvider).routeInformationProvider.value.uri;
    var token = request, retired = false;
    bool valid() =>
        !retired &&
        current &&
        request == token &&
        identical(service, widget.service) &&
        project?.id == originalProject.id &&
        project?.revision == originalProject.revision &&
        file?.path == originalPath &&
        router.routeInformationProvider.value.uri == route &&
        route.path == AppRoutes.automation;
    void observe() {
      if (!valid()) retired = true;
    }

    router.routeInformationProvider.addListener(observe);
    try {
      if (!await _leaveFile(originalCurrent: valid, ownSaveRequest: (next) => token = next) || !valid() || !mounted) {
        return;
      }
      final originalFile = file!;
      if (dirty) {
        setState(() {
          editor.text = originalFile.text;
          dirty = false;
        });
      }
      final transformed = await showAutomationVaultFileDialog(
        context,
        ref,
        scope: widget.scope,
        project: originalProject,
        file: originalFile,
        service: service as AutomationVaultFileService,
        originalCurrent: () => valid() && !busy && file?.sha256 == originalFile.sha256,
      );
      if (!valid() || transformed == null) return;
      if (transformed.path != originalPath) throw const AutomationFileConflict();
      token = ++request;
      setState(() {
        busy = true;
        error = null;
        file = transformed;
        newPath = null;
        repairOrigin = null;
        aiDraft = null;
        requirementsSavedPending = false;
        editor.text = transformed.text;
        dirty = false;
      });
      final refreshed = await widget.service.files(originalProject.id);
      if (valid()) setState(() => revision = refreshed);
    } catch (failure) {
      if (valid()) setState(() => error = failure);
    } finally {
      router.routeInformationProvider.removeListener(observe);
      if (current && token == request) setState(() => busy = false);
    }
  }

  Future<void> _entryMenu(AutomationFileDrag entry, Offset position) async {
    final l = context.l10n;
    final overlay = Overlay.of(context).context.findRenderObject()! as RenderBox;
    final offset = overlay.globalToLocal(position);
    final original = project;
    final token = request;
    final pathLower = entry.path.toLowerCase();
    final protected =
        pathLower.split('/').contains('.git') ||
        pathLower == '.vault/password' ||
        entry.directory && (pathLower == '.vault' || pathLower.startsWith('.vault/'));
    final canCreate = entry.directory && original?.readOnly == false && !protected;
    final editAllowed =
        original?.readOnly == false &&
        widget.service is AutomationFileActionsService &&
        entry.path.isNotEmpty &&
        !protected;
    final emptyDirectory =
        entry.directory && !(revision?.entries.any((item) => item.path.startsWith('${entry.path}/')) ?? false);
    bool valid() =>
        current &&
        !busy &&
        request == token &&
        identical(project, original) &&
        entry.digest == revision?.digest &&
        identical(widget.scope, entry.scope);
    final action = await showMenu<String>(
      context: context,
      position: RelativeRect.fromSize(offset & Size.zero, overlay.size),
      items: [
        if (entry.directory)
          _automationMenuItem(
            value: 'new-file',
            label: l.automationDirectoryNewFile,
            icon: Icons.note_add_outlined,
            enabled: canCreate,
          )
        else
          _automationMenuItem(value: 'open', label: l.commonOpen, icon: Icons.description_outlined),
        if (entry.path.isNotEmpty)
          _automationMenuItem(value: 'copy', label: l.automationFileCopyPath, icon: Icons.copy_outlined),
        if (editAllowed) ...[
          _automationMenuItem(value: 'rename', label: l.commonRename, icon: Icons.edit_outlined),
          if (!entry.directory || emptyDirectory)
            _automationMenuItem(value: 'delete', label: l.commonDelete, icon: Icons.delete_outline),
        ],
      ],
    );
    if (!valid() || action == null) return;
    switch (action) {
      case 'open':
        await _read(entry.path);
      case 'new-file':
        await _newFile(inDirectory: entry.path);
      case 'copy':
        if (!mounted) return;
        await copyPrivateWithNotice(context, ref, entry.path);
      case 'rename':
        final name = await _name(l.commonRename, initial: entry.path.split('/').last);
        if (name != null && valid()) await _changeProjectFile(entry, newName: name);
      case 'delete':
        final confirmed = await _dialog<bool>(
          (context) => AlertDialog(
            title: Text(l.commonDelete),
            content: Text(
              entry.directory
                  ? l.automationDirectoryDeleteConfirm(entry.path)
                  : l.automationFileDeleteConfirm(entry.path),
            ),
            actions: [
              TextButton(onPressed: () => Navigator.pop(context, false), child: Text(l.commonCancel)),
              FilledButton(onPressed: () => Navigator.pop(context, true), child: Text(l.commonDelete)),
            ],
          ),
        );
        if (confirmed == true && valid()) await _changeProjectFile(entry, delete: true);
    }
  }

  Future<void> _changeProjectFile(
    AutomationFileDrag source, {
    String? directory,
    String? newName,
    bool delete = false,
  }) async {
    final originalProject = project, service = widget.service, tree = revision;
    if (!current ||
        busy ||
        originalProject == null ||
        originalProject.readOnly ||
        tree == null ||
        (directory != null ? service is! AutomationMoveService : service is! AutomationFileActionsService) ||
        !identical(source.scope, widget.scope) ||
        source.projectId != originalProject.id ||
        source.digest != tree.digest ||
        !tree.entries.any((e) => e.path == source.path && e.directory == source.directory)) {
      return;
    }
    final parts = source.path.split('/');
    final parent = directory ?? parts.take(parts.length - 1).join('/');
    final basename = newName ?? parts.last;
    final target = parent.isEmpty ? basename : '$parent/$basename';
    if (!delete && (target == source.path || automationFileHierarchy(tree.entries).at(parent)?.directory != true)) {
      return;
    }
    final router = ref.read(routerProvider),
        originalRoute = ref.read(routerProvider).routeInformationProvider.value.uri;
    var token = request, retired = false;
    bool valid() =>
        !retired &&
        current &&
        request == token &&
        identical(service, widget.service) &&
        project?.id == originalProject.id &&
        router.routeInformationProvider.value.uri == originalRoute &&
        originalRoute.path == AppRoutes.automation;
    void observe() {
      if (!valid()) retired = true;
    }

    router.routeInformationProvider.addListener(observe);
    final selectedPath = file?.path ?? newPath;
    final movingSelection =
        selectedPath == source.path || (source.directory && selectedPath?.startsWith('${source.path}/') == true);
    try {
      if (!valid()) return;
      if (movingSelection && !await _leaveFile(originalCurrent: valid, ownSaveRequest: (next) => token = next) ||
          !valid()) {
        return;
      }
      token = ++request;
      setState(() {
        busy = true;
        error = null;
      });
      final originalFile = movingSelection ? file : null;
      var expectedSha = source.directory
          ? revision?.digest
          : originalFile?.sha256 ?? tree.entries.firstWhere((e) => e.path == source.path).sha256;
      if (expectedSha == null) {
        if (source.directory) throw const AutomationFileConflict();
        final snapshotFile = await service.readFile(originalProject.id, source.path);
        if (snapshotFile.path != source.path) throw const AutomationFileConflict();
        expectedSha = snapshotFile.sha256;
      }
      if (!valid()) return;
      if (directory != null) {
        await (service as AutomationMoveService).moveProjectFile(
          originalProject.id,
          sourcePath: source.path,
          destinationDirectory: directory,
          expectedSha256: expectedSha,
        );
      } else {
        await (service as AutomationFileActionsService).editProjectFileEntry(
          originalProject.id,
          path: source.path,
          expectedSha256: expectedSha,
          action: source.directory
              ? (delete ? 'delete_directory' : 'rename_directory')
              : (delete ? 'delete' : 'rename'),
          newName: newName,
        );
      }
      if (!valid()) return;
      // The Native move keeps the exact CAS-checked bytes. Keep the editor at
      // the new path even if the following read-only tree refresh fails.
      if (movingSelection) {
        setState(() {
          file = delete || originalFile == null
              ? null
              : AutomationProjectFile(
                  path: '$target${originalFile.path.substring(source.path.length)}',
                  text: originalFile.text,
                  sha256: originalFile.sha256,
                  lineEndings: originalFile.lineEndings,
                );
          newPath = null;
          aiDraft = null;
          repairOrigin = null;
          requirementsSavedPending = false;
          editor.text = file?.text ?? '';
          dirty = false;
        });
      }
      final refreshed = await service.files(originalProject.id);
      if (valid()) setState(() => revision = refreshed);
    } catch (failure) {
      if (valid()) setState(() => error = failure);
    } finally {
      router.routeInformationProvider.removeListener(observe);
      if (current && token == request) setState(() => busy = false);
    }
  }

  Future<void> _newFile({bool playbook = false, String? inDirectory}) async {
    if (busy || project == null || !await _leaveFile() || !current || !mounted) return;
    var suggestion = 'playbook.yml';
    if (playbook) {
      final existing = {for (final entry in revision?.entries ?? <AutomationFileEntry>[]) entry.path};
      for (var index = 2; existing.contains(suggestion); index++) {
        suggestion = 'playbook_$index.yml';
      }
    }
    final id = project!.id, token = request;
    final name = await _name(
      playbook
          ? context.l10n.automationNewPlaybook
          : inDirectory != null
          ? context.l10n.automationDirectoryNewFile
          : context.l10n.automationOtherFile,
      initial: playbook ? suggestion : '',
    );
    if (name == null || !current || request != token || project?.id != id) return;
    final path = (inDirectory?.isNotEmpty == true) ? '$inDirectory/$name' : name;
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
      _showPhoneEditor = true;
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

  Future<void> _importPlaybook() async {
    final selected = project;
    if (busy ||
        selected == null ||
        selected.readOnly ||
        !await _leaveFile() ||
        !mounted ||
        !current ||
        project != selected) {
      return;
    }
    final token = ++request;
    final picker = ref.read(fileDialogServiceProvider);
    final router = ref.read(routerProvider);
    final uri = router.routeInformationProvider.value.uri;
    final invalidFile = context.l10n.automationImportPlaybookInvalid;
    bool original() =>
        current &&
        token == request &&
        project == selected &&
        workspacePage == 0 &&
        ref.read(routerProvider) == router &&
        router.routeInformationProvider.value.uri == uri;
    setState(() {
      busy = true;
      error = null;
    });
    try {
      final path = await picker.chooseOpenFile(extensions: const ['yml', 'yaml']);
      if (!original() || path == null) return;
      final name = path.replaceAll('\\', '/').split('/').last;
      final suffix = RegExp(r'\.(yml|yaml)$', caseSensitive: false).firstMatch(name);
      if (suffix == null ||
          name.contains(RegExp(r'[:\x00-\x1f\x7f]')) ||
          FileSystemEntity.typeSync(path, followLinks: false) != FileSystemEntityType.file) {
        throw FormatException(invalidFile);
      }
      const maxBytes = 256 * 1024;
      final input = await File(path).open();
      String text;
      try {
        final bytes = await input.read(maxBytes + 1);
        if (bytes.isEmpty || bytes.length > maxBytes || bytes.contains(0)) throw FormatException(invalidFile);
        text = utf8.decode(bytes);
      } finally {
        await input.close();
      }
      if (!original()) return;
      final occupied = {
        for (final entry in revision?.entries ?? const <AutomationFileEntry>[]) entry.path.toLowerCase(),
      };
      final base = String.fromCharCodes(name.substring(0, suffix.start).runes.take(56));
      final stem = base.isEmpty ? 'playbook' : base;
      final ext = name.substring(suffix.start);
      var destination = '$stem$ext';
      for (var index = 2; occupied.contains(destination.toLowerCase()); index++) {
        destination = '${stem}_$index$ext';
      }
      setState(() {
        _showPhoneEditor = true;
        file = null;
        newPath = destination;
        repairOrigin = null;
        aiDraft = null;
        requirementsSavedPending = false;
        editor.text = text;
        dirty = true;
      });
    } catch (_) {
      if (original()) setState(() => error = FormatException(invalidFile));
    } finally {
      if (current && token == request) setState(() => busy = false);
    }
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
        _showPhoneEditor = true;
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
    final l = context.l10n;
    final jobs = ref.watch(automationJobServiceProvider);
    if (workspacePage != 0) _retireExternalEditor?.call();
    final fullConsole = _phoneWorkspace || workspacePage == _executionPage;
    final visiblePage = workspacePage;
    Widget execution({required bool fullPage}) => AutomationExecutionConsole(
      key: ValueKey(executionJob),
      scope: widget.scope,
      service: jobs!,
      jobId: executionJob!,
      collapsed: !fullPage && consoleCollapsed,
      onToggle: fullPage ? null : () => setState(() => consoleCollapsed = !consoleCollapsed),
      onHistory: () => setState(() => workspacePage = 1),
      onClose: () => setState(() {
        executionJob = null;
        if (fullPage) workspacePage = 1;
      }),
    );
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
        Padding(
          padding: const EdgeInsets.fromLTRB(12, 8, 12, 0),
          child: Wrap(
            spacing: 8,
            runSpacing: 8,
            children: [
              navigation(
                'automation-workspace-projects',
                0,
                Icons.folder_open,
                l.automationLibrary,
                _phoneWorkspace && visiblePage == 0 && _showPhoneEditor ? _backToPhoneFiles : _showProjects,
              ),
              if (jobs != null)
                navigation(
                  'automation-workspace-jobs',
                  1,
                  Icons.history,
                  l.automationJobs,
                  () => setState(() => workspacePage = 1),
                ),
              navigation(
                'automation-workspace-schedules',
                3,
                Icons.schedule_outlined,
                l.automationSchedulesTitle,
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
              if (executionJob != null && fullConsole && jobs != null)
                navigation(
                  'automation-mobile-execution-open',
                  _executionPage,
                  Icons.terminal,
                  l.automationExecutionOutput,
                  () => setState(() => workspacePage = _executionPage),
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
                        AutomationSchedulesView(
                          active: workspacePage == 3,
                          scope: widget.scope,
                          service: widget.service,
                          onCreate: (item) async {
                            await _select(item);
                            final service = widget.service;
                            if (current && project?.id == item.id && service is AutomationJobService) {
                              await _run(service as AutomationJobService, scheduleOnly: true);
                            }
                          },
                        ),
                        if (executionJob != null && fullConsole && jobs != null)
                          TickerMode(
                            enabled: visiblePage == _executionPage,
                            child: Padding(padding: const EdgeInsets.all(12), child: execution(fullPage: true)),
                          )
                        else
                          const SizedBox.shrink(),
                      ],
                    ),
                  ),
                  if (executionJob != null && jobs != null && !fullConsole) ...[
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
                      child: execution(fullPage: false),
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
    final catalog = ref.watch(automationProjectsProvider);
    final runnerService = ref.watch(automationRunnerServiceProvider);
    final portableAvailable =
        widget.service is AutomationPortableGatewayCapability && ref.watch(criticalRuntimeEventsProvider) != null;
    final menuProject = project;
    final menuRequest = request;
    final editingOnPhone = _phoneWorkspace && _showPhoneEditor && (file != null || newPath != null);
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
          if (!editingOnPhone)
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
                    if (AppPlatform.isMobile)
                      _automationMenuItem(
                        key: const ValueKey('automation-import-playbook'),
                        value: 'import-playbook',
                        label: l.automationImportPlaybook,
                        icon: Icons.file_open_outlined,
                        enabled: projectMenuCurrent() && revision != null,
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
                        label: l.automationLibraryImportArchive,
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
                    if (value == 'import-playbook' && projectMenuCurrent()) unawaited(_importPlaybook());
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
                      if (value == 'inventory' && widget.service is AutomationSourceInventoryService) {
                        _inventory(widget.service as AutomationSourceInventoryService);
                      }
                    },
                  ),
              ],
            ),
          if (!editingOnPhone && (widget.service is AutomationProjectVersionService || portableAvailable))
            Padding(
              padding: const EdgeInsets.only(top: 6),
              child: Wrap(
                spacing: 8,
                runSpacing: 6,
                crossAxisAlignment: WrapCrossAlignment.center,
                children: [
                  if (widget.service is AutomationProjectVersionService)
                    GlassButton(
                      key: const ValueKey('automation-library-versions'),
                      label: l.automationVersionsTitle,
                      icon: Icons.history,
                      size: _automationControlSize(context),
                      onPressed: projectMenuCurrent() && revision != null
                          ? () => _versions(widget.service as AutomationProjectVersionService)
                          : null,
                    ),
                  if (portableAvailable) ...[
                    GlassButton(
                      key: const ValueKey('automation-library-export-version'),
                      label: l.automationLibraryExportVersion,
                      icon: Icons.ios_share_outlined,
                      size: _automationControlSize(context),
                      onPressed: projectMenuCurrent() && project?.revision != null
                          ? () => _portablePackage(inspect: false)
                          : null,
                    ),
                    if (widget.service is AutomationPortableImportReviewCapability)
                      GlassButton(
                        key: const ValueKey('automation-library-import-archive'),
                        label: l.automationLibraryImportArchive,
                        tooltip: l.automationLibraryExchangeHint,
                        icon: Icons.unarchive_outlined,
                        size: _automationControlSize(context),
                        onPressed: busy || catalog.asData?.value?.readOnly == true
                            ? null
                            : () => _portablePackage(inspect: true, createProject: true),
                      ),
                  ],
                ],
              ),
            ),
          if (project?.gitSource case final Map<String, dynamic> source when !editingOnPhone)
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
                // Leave room for the editor after the file tree and its resize handle.
                final compact = constraints.maxWidth < _fileTreeMinWidth + GlassSpacing.s8 + 360;
                final showTree = !fileTreeCollapsed || file == null && newPath == null;
                final tree = ContentSurface(
                  key: const ValueKey('automation-project-tree'),
                  padding: EdgeInsets.zero,
                  child: ClipRRect(
                    borderRadius: BorderRadius.circular(GlassTokens.of(context).radii.card),
                    child: ListView(
                      padding: const EdgeInsets.symmetric(horizontal: 3, vertical: 3),
                      children: [
                        if (project != null) ...[
                          Padding(
                            key: const ValueKey('automation-project-header'),
                            padding: const EdgeInsets.fromLTRB(12, 4, 4, 4),
                            child: Row(
                              children: [
                                Expanded(
                                  child: InkWell(
                                    key: const ValueKey('automation-project-picker-toggle'),
                                    onSecondaryTapDown: busy
                                        ? null
                                        : (details) => unawaited(_projectEntryMenu(project!, details.globalPosition)),
                                    onTap: () => setState(() {
                                      projectsExpanded = !projectsExpanded;
                                      if (!projectsExpanded) libraryQuery = '';
                                    }),
                                    borderRadius: BorderRadius.circular(6),
                                    child: Row(
                                      children: [
                                        Icon(projectsExpanded ? Icons.expand_more : Icons.chevron_right, size: 16),
                                        const SizedBox(width: 4),
                                        Expanded(
                                          child: Text(
                                            project!.name ?? project!.id,
                                            maxLines: 1,
                                            overflow: TextOverflow.ellipsis,
                                          ),
                                        ),
                                      ],
                                    ),
                                  ),
                                ),
                                const SizedBox(width: 4),
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
                                const SizedBox(width: 6),
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
                                        label: l.automationLibraryExportVersion,
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
                        ],
                        if (project == null || projectsExpanded) ...[
                          Padding(padding: const EdgeInsets.fromLTRB(12, 12, 12, 4), child: Text(l.automationProjects)),
                          Padding(
                            padding: const EdgeInsets.fromLTRB(8, 0, 8, 8),
                            child: GlassField(
                              key: const ValueKey('automation-library-search'),
                              fieldKey: const ValueKey('automation-library-search-input'),
                              search: true,
                              placeholder: l.automationLibrarySearch,
                              onChanged: (value) => setState(() => libraryQuery = value),
                            ),
                          ),
                          ...catalog.when(
                            data: (value) => [
                              if (value?.readOnly == true)
                                Padding(padding: const EdgeInsets.all(12), child: Text(l.automationNewerSchema)),
                              if (value?.projects.isEmpty ?? false)
                                Padding(padding: const EdgeInsets.all(12), child: Text(l.automationNoProjects)),
                              if (value?.projects.isNotEmpty == true &&
                                  !(value!.projects.any(
                                    (item) => '${item.name ?? item.id} ${item.sourceKind ?? ''}'.toLowerCase().contains(
                                      libraryQuery.trim().toLowerCase(),
                                    ),
                                  )))
                                Padding(padding: const EdgeInsets.all(12), child: Text(l.automationLibraryNoMatches)),
                              for (final item in (value?.projects ?? <AutomationProject>[]).where(
                                (item) => '${item.name ?? item.id} ${item.sourceKind ?? ''}'.toLowerCase().contains(
                                  libraryQuery.trim().toLowerCase(),
                                ),
                              ))
                                GestureDetector(
                                  behavior: HitTestBehavior.opaque,
                                  onSecondaryTapDown: busy
                                      ? null
                                      : (details) => unawaited(_projectEntryMenu(item, details.globalPosition)),
                                  onLongPressStart: busy || !_phoneWorkspace
                                      ? null
                                      : (details) => unawaited(_projectEntryMenu(item, details.globalPosition)),
                                  child: ListTile(
                                    key: ValueKey('automation-project-${item.id}'),
                                    dense: !_phoneWorkspace,
                                    minTileHeight: _phoneWorkspace ? 48 : 34,
                                    visualDensity: _phoneWorkspace ? VisualDensity.standard : VisualDensity.compact,
                                    selected: item.id == project?.id,
                                    shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(6)),
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
                                ),
                            ],
                            error: (e, _) => [
                              Padding(padding: const EdgeInsets.all(12), child: Text(errorMessage(l, e))),
                              TextButton(
                                onPressed: () => ref.invalidate(automationProjectsProvider),
                                child: Text(l.commonRetry),
                              ),
                            ],
                            loading: () => [
                              const Padding(padding: EdgeInsets.all(12), child: LinearProgressIndicator()),
                            ],
                          ),
                        ],
                        if (project != null) ...[
                          const Divider(height: 8),
                          AutomationLibraryFiles(
                            scope: widget.scope,
                            projectId: project!.id,
                            digest: revision?.digest ?? '',
                            entries: revision?.entries ?? const [],
                            selectedPath: file?.path ?? newPath,
                            enabled: !busy,
                            onRead: (path) => unawaited(_read(path)),
                            onCreatePlaybook: project!.readOnly
                                ? null
                                : () {
                                    if (projectMenuCurrent()) _newFile(playbook: true);
                                  },
                            onCreateRole: project!.readOnly || widget.service is! AutomationScaffoldService
                                ? null
                                : () {
                                    if (projectMenuCurrent()) _newRole(widget.service as AutomationScaffoldService);
                                  },
                            onAddDependency: project!.readOnly || widget.service is! AutomationDependencyService
                                ? null
                                : () {
                                    if (projectMenuCurrent()) {
                                      _dependencies(widget.service as AutomationDependencyService);
                                    }
                                  },
                            onMove: project!.readOnly || widget.service is! AutomationMoveService || revision == null
                                ? null
                                : (source, directory) => unawaited(_changeProjectFile(source, directory: directory)),
                            onContextMenu: revision == null
                                ? null
                                : (entry, position) => unawaited(_entryMenu(entry, position)),
                          ),
                        ],
                      ],
                    ),
                  ),
                );
                final body = ContentSurface(
                  kind: ContentSurfaceKind.solid,
                  padding: const EdgeInsets.all(GlassSpacing.s8),
                  child: file == null && newPath == null
                      ? Center(child: Text(l.automationChooseFile))
                      : Column(
                          crossAxisAlignment: CrossAxisAlignment.stretch,
                          children: [
                            LayoutBuilder(
                              builder: (context, toolbar) => Row(
                                key: const ValueKey('automation-editor-toolbar'),
                                spacing: GlassSpacing.s8,
                                children: [
                                  if (editingOnPhone)
                                    GlassIconButton(
                                      key: const ValueKey('automation-phone-editor-files'),
                                      tooltip: l.automationLibraryFiles,
                                      icon: Icons.arrow_back,
                                      size: 40,
                                      onPressed: busy ? null : _backToPhoneFiles,
                                    )
                                  else if (!_phoneWorkspace) ...[
                                    GlassIconButton(
                                      key: const ValueKey('automation-toggle-file-tree'),
                                      tooltip: l.automationLibraryFiles,
                                      icon: Icons.view_sidebar_outlined,
                                      size: 28,
                                      selected: showTree,
                                      onPressed: () => setState(() => fileTreeCollapsed = !fileTreeCollapsed),
                                    ),
                                  ],
                                  Expanded(
                                    child: Text /* l10n-ignore: filename and dirty marker */ (
                                      '${file?.path ?? newPath}${dirty ? ' •' : ''}', // l10n-ignore: document path and dirty marker
                                      style: GlassTokens.of(context).typography.mono,
                                      overflow: TextOverflow.ellipsis,
                                    ),
                                  ),
                                  ConstrainedBox(
                                    constraints: BoxConstraints(maxWidth: toolbar.maxWidth * .65),
                                    child: Wrap(
                                      alignment: WrapAlignment.end,
                                      crossAxisAlignment: WrapCrossAlignment.center,
                                      spacing: GlassSpacing.s4,
                                      runSpacing: GlassSpacing.s4,
                                      children: [
                                        if (ref.watch(automationJobServiceProvider) case final jobs?
                                            when editingOnPhone)
                                          GlassIconButton(
                                            key: const ValueKey('automation-phone-editor-run'),
                                            tooltip: l.automationRun,
                                            icon: Icons.play_arrow,
                                            size: 40,
                                            onPressed: busy ? null : () => _run(jobs),
                                          ),
                                        GlassIconButton(
                                          key: const ValueKey('automation-save'),
                                          tooltip: l.commonSave,
                                          icon: Icons.save_outlined,
                                          size: AppPlatform.isMobile ? 40 : 28,
                                          iconSize: 16,
                                          onPressed: !dirty || busy ? null : _save,
                                        ),
                                        if (file != null &&
                                            project?.readOnly == false &&
                                            project?.revision != null &&
                                            widget.service is AutomationVaultFileService &&
                                            (file!.path.endsWith('.yml') || file!.path.endsWith('.yaml')))
                                          GlassIconButton(
                                            key: const ValueKey('automation-vault-file-open'),
                                            tooltip: l.automationVaultFileTitle,
                                            icon: Icons.lock_outline,
                                            size: AppPlatform.isMobile ? 40 : 28,
                                            iconSize: 16,
                                            onPressed: busy ? null : () => unawaited(_vaultFile()),
                                          ),
                                        if (!AppPlatform.isMobile &&
                                            file != null &&
                                            widget.service is AutomationEditorService) ...[
                                          GlassIconButton(
                                            key: const ValueKey('automation-open-editor'),
                                            size: AppPlatform.isMobile ? 40 : 28,
                                            iconSize: 16,
                                            tooltip: l.automationOpenEditor,
                                            onPressed: busy
                                                ? null
                                                : () => _externalEditor(widget.service as AutomationEditorService),
                                            icon: Icons.open_in_new,
                                          ),
                                          GlassIconButton(
                                            key: const ValueKey('automation-reload-file'),
                                            size: AppPlatform.isMobile ? 40 : 28,
                                            iconSize: 16,
                                            tooltip: l.automationReloadFile,
                                            onPressed: busy ? null : () => _read(file!.path),
                                            icon: Icons.refresh,
                                          ),
                                        ],
                                        if (widget.service case final AutomationAiService ai)
                                          GlassIconButton(
                                            key: const ValueKey('automation-ai-open'),
                                            tooltip: l.automationAiTitle,
                                            size: AppPlatform.isMobile ? 40 : 28,
                                            iconSize: 16,
                                            onPressed:
                                                busy ||
                                                    !((file?.path ?? newPath ?? '').endsWith('.yml') ||
                                                        (file?.path ?? newPath ?? '').endsWith('.yaml'))
                                                ? null
                                                : () => _ai(ai),
                                            icon: Icons.auto_awesome,
                                          ),
                                      ],
                                    ),
                                  ),
                                ],
                              ),
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
                if (_phoneWorkspace) return editingOnPhone ? body : tree;
                if (compact) {
                  return Column(
                    children: [
                      if (showTree) ...[SizedBox(height: 148, child: tree), const SizedBox(height: GlassSpacing.s8)],
                      Expanded(key: const ValueKey('automation-editor-pane'), child: body),
                    ],
                  );
                }
                final maximum = (constraints.maxWidth - 360 - GlassSpacing.s8).clamp(_fileTreeMinWidth, 520.0);
                final width = sidebarWidth.clamp(_fileTreeMinWidth, maximum);
                return Row(
                  children: [
                    if (showTree) ...[
                      SizedBox(width: width, child: tree),
                      MouseRegion(
                        cursor: SystemMouseCursors.resizeColumn,
                        child: GestureDetector(
                          key: const ValueKey('automation-project-resize'),
                          behavior: HitTestBehavior.opaque,
                          onHorizontalDragUpdate: (details) => setState(
                            () => sidebarWidth = (width + details.delta.dx).clamp(_fileTreeMinWidth, maximum),
                          ),
                          child: const SizedBox(
                            width: GlassSpacing.s8,
                            height: double.infinity,
                            child: VerticalDivider(width: 1),
                          ),
                        ),
                      ),
                    ],
                    Expanded(key: const ValueKey('automation-editor-pane'), child: body),
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
            size: AppPlatform.isMobile ? 40 : 24,
            iconSize: 16,
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
