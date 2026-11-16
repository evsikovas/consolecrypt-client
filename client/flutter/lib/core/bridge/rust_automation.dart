import 'dart:convert';

import 'package:consolecrypt/automation/automation_agent_installer.dart';
import 'package:consolecrypt/automation/automation_awx_handle_gateway.dart';
import 'package:consolecrypt/automation/automation_dependency_models.dart';
import 'package:consolecrypt/automation/automation_inventory_export.dart';
import 'package:consolecrypt/automation/automation_monitoring_node.dart';
import 'package:consolecrypt/automation/automation_portable_handle_gateway.dart';
import 'package:consolecrypt/automation/automation_portable_import_gateway.dart';
import 'package:consolecrypt/automation/automation_portable_import_session.dart';
import 'package:consolecrypt/automation/automation_portable_native_adapter.dart';
import 'package:consolecrypt/automation/automation_portable_service.dart';
import 'package:consolecrypt/automation/automation_project_credentials.dart';
import 'package:consolecrypt/automation/automation_project_metadata.dart';
import 'package:consolecrypt/automation/automation_runner_cleanup.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/automation/automation_service_delegation_service.dart';
import 'package:consolecrypt/automation/automation_windows_prerequisites.dart';
import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/bridge/rust_account.dart' show withStepUp;
import 'package:consolecrypt/core/bridge/rust_automation_awx.dart';
import 'package:consolecrypt/core/bridge/rust_automation_portable.dart';
import 'package:consolecrypt/core/bridge/rust_automation_project_metadata.dart';
import 'package:consolecrypt/core/models/step_up.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/src/rust/api/agent_installer.dart' as installer_native;
import 'package:consolecrypt/src/rust/api/automation.dart' as native;
import 'package:consolecrypt/src/rust/api/automation_service.dart' as service_native;

final class RustAutomationService
    implements
        AutomationService,
        AutomationProjectCredentialsService,
        AutomationProjectMetadataService,
        AutomationAgentInstallerService,
        AutomationMonitoringNodeService,
        AutomationServiceDelegationService,
        AutomationServiceResultsService,
        AutomationDependencyService,
        AutomationAwxPortCapability,
        AutomationPortableGatewayCapability,
        AutomationPortableImportReviewCapability,
        AutomationRunnerService,
        AutomationRunnerEditingService,
        AutomationRunnerSetupService,
        AutomationWindowsPrerequisiteService,
        AutomationRunnerCleanupService,
        AutomationInventoryExportService,
        AutomationRunnerPrerequisiteTerminalService,
        AutomationJobService,
        AutomationProductionOwnerService,
        AutomationVaultSelectionService,
        AutomationEventService,
        AutomationAiService,
        AutomationAiContextService,
        AutomationGitService,
        AutomationDiffService,
        AutomationEditorService,
        AutomationSourceInventoryService,
        AutomationInventoryService,
        AutomationFolderBindingService,
        AutomationInventorySelectionService,
        AutomationScaffoldService {
  late final _projectMetadata = RustAutomationProjectMetadataPort(
    contextJson: (id) => _automationGuard(() => native.automationProjectMetadataContext(id: id)),
    catalogJson: () => _automationGuard(native.automationProjectMetadataCatalog),
    publishJson: (request) =>
        _automationGuard(() => native.automationProjectMetadataPublish(requestJson: request)),
  );

  @override
  Future<AutomationJson> projectMetadataContext(String projectId) =>
      _projectMetadata.projectMetadataContext(projectId);

  @override
  Future<AutomationJson> projectMetadataCatalog() => _projectMetadata.projectMetadataCatalog();

  @override
  Future<AutomationJson> publishProjectMetadata(AutomationJson originalFrame) =>
      _projectMetadata.publishProjectMetadata(originalFrame);

  @override
  Future<AutomationJson> agentInstallerContext(AutomationJson frame) async =>
      jsonDecode(await _automationGuard(
        () => installer_native.automationAgentInstallerContextJson(requestJson: jsonEncode(frame)),
      )) as AutomationJson;

  @override
  Future<AutomationJson> createAgentInstallerProject(AutomationJson request) async =>
      jsonDecode(await _automationGuard(
        () => installer_native.automationAgentInstallerCreateJson(requestJson: jsonEncode(request)),
      )) as AutomationJson;

  @override
  Future<AutomationJson> createMonitoringNodeProject(AutomationJson request) async =>
      jsonDecode(await _automationGuard(
        () => installer_native.automationMonitoringNodeCreateJson(requestJson: jsonEncode(request)),
      )) as AutomationJson;

  @override
  Future<AutomationJson> prepareServiceDelegation(AutomationJson request) async =>
      jsonDecode(await _automationGuard(() => service_native.automationServicePrepareJson(requestJson: jsonEncode(request)))) as AutomationJson;

  @override
  Future<AutomationJson> confirmServiceDelegation(AutomationJson request) async =>
      jsonDecode(await _automationGuard(() => service_native.automationServiceConfirmJson(requestJson: jsonEncode(request)))) as AutomationJson;

  @override
  Future<AutomationJson> cancelServiceDelegation(AutomationJson request) async =>
      jsonDecode(await _automationGuard(() => service_native.automationServiceCancelJson(requestJson: jsonEncode(request)))) as AutomationJson;

  @override
  Future<AutomationJson> serviceDelegationStatus(AutomationJson request) async =>
      jsonDecode(await _automationGuard(() => service_native.automationServiceStatusJson(requestJson: jsonEncode(request)))) as AutomationJson;

  @override
  Future<AutomationJson> serviceDelegationResults(AutomationJson request) async =>
      jsonDecode(
        await _automationGuard(
          () => service_native.automationServiceResultsJson(requestJson: jsonEncode(request)),
        ),
      ) as AutomationJson;

  @override
  Future<AutomationJson> revokeServiceDelegation(AutomationJson request) async =>
      jsonDecode(await _automationGuard(() => service_native.automationServiceRevokeJson(requestJson: jsonEncode(request)))) as AutomationJson;

  @override
  AwxOpaqueNativePort createAwxPort() => const RustAwxOpaqueNativePort();

  @override
  Future<AutomationJson> vaultContext(String projectId) async =>
      jsonDecode(await _automationGuard(() => native.automationProjectVaultContextJson(projectId: projectId)))
          as AutomationJson;

  @override
  Future<AutomationJson> projectCredentialsContext(String projectId) async =>
      jsonDecode(await _automationGuard(() => native.automationProjectCredentialsContext(id: projectId)))
          as AutomationJson;

  @override
  Future<AutomationJson> saveProjectCredentials(AutomationJson request) async =>
      jsonDecode(await _automationGuard(() => native.automationProjectCredentialsSave(requestJson: jsonEncode(request))))
          as AutomationJson;

  @override
  Future<AutomationJson> selectVault(AutomationJson selection) async => jsonDecode(
    await _automationGuard(() => native.automationProjectVaultSelectJson(requestJson: jsonEncode(selection))),
  ) as AutomationJson;

  @override
  PortableNativeGateway createPortableGateway() => PortableHandleGateway(port: const RustPortableOpaqueNativePort());

  @override
  PortableImportReviewGateway createPortableImportReviewGateway() =>
      const PortableImportHandleGateway(port: RustPortableImportOpaqueNativePort());

  @override
  Future<String> dependencyAction(String requestJson) =>
      _automationGuard(() => native.automationDependencyActionJson(requestJson: requestJson));

  late final _inventoryExport = AutomationInventoryExportClient(
    configuration: inventoryConfiguration,
    issue: (requestJson) => _automationGuard(() => native.automationInventoryExportIssue(requestJson: requestJson)),
  );
  @override
  Future<InventoryExportContext> inventoryExportContext(String projectId) =>
      _inventoryExport.inventoryExportContext(projectId);
  @override
  Future<IssuedInventorySnapshot> issueInventoryExport(InventoryExportRequest request) =>
      _inventoryExport.issueInventoryExport(request);

  late final _cleanup = AutomationRunnerCleanupClient(
    rpc: (method, request) async => jsonDecode(
      await _automationGuard(
        () => switch (method) {
          'scan' => native.automationRunnerCleanupScan(requestJson: jsonEncode(request)),
          'plan' => native.automationRunnerCleanupPlan(requestJson: jsonEncode(request)),
          'commit' => native.automationRunnerCleanupCommit(requestJson: jsonEncode(request)),
          _ => throw const AutomationRunnerCleanupFailed(RunnerCleanupReason.invalidRequest),
        },
      ),
    ) as AutomationJson,
    cancel: cancelRunnerSetup,
  );
  @override
  Future<RunnerCleanupScan> scanRunnerCleanup(
    AutomationJson target, {
    required String operationId,
    String? selectedPython,
  }) => _cleanup.scanRunnerCleanup(target, operationId: operationId, selectedPython: selectedPython);
  @override
  Future<RunnerCleanupPlan> planRunnerCleanup(
    AutomationJson target,
    List<String> candidateIds, {
    required String operationId,
    String? selectedPython,
  }) => _cleanup.planRunnerCleanup(target, candidateIds, operationId: operationId, selectedPython: selectedPython);
  @override
  Future<RunnerCleanupResult> commitRunnerCleanup(
    String planToken, {
    required String operationId,
    String? selectedPython,
  }) => _cleanup.commitRunnerCleanup(planToken, operationId: operationId, selectedPython: selectedPython);
  @override
  Future<void> cancelRunnerCleanup(String operationId) => _cleanup.cancelRunnerCleanup(operationId);
  @override
  Future<AutomationRunnerProfile> updateRunner(
    String id, {
    required int revision,
    required String name,
    required AutomationJson configuration,
  }) async => AutomationRunnerProfile.fromJson(
    jsonDecode(
      await _automationGuard(
        () => native.automationRunnerUpdate(
          id: id,
          expectedRevision: revision,
          name: name,
          configurationJson: jsonEncode(configuration),
        ),
      ),
    ) as AutomationJson,
  );
  @override
  Future<AutomationProjectFile> createRole(String projectId, String name) async => AutomationProjectFile.fromJson(
    jsonDecode(await _automationGuard(() => native.automationProjectCreateRole(id: projectId, name: name)))
        as AutomationJson,
  );
  @override
  Future<String> openPrerequisiteTerminal(
    AutomationJson target, {
    required String operationId,
    required String command,
  }) => _automationGuard(
    () => native.automationRunnerPrerequisiteTerminal(
      operationId: operationId,
      targetJson: jsonEncode(target),
      reviewedCommand: command,
    ),
  );
  @override
  Future<AutomationProject> selectInventory(AutomationJson selection) async => AutomationProject.fromJson(
    jsonDecode(await _automationGuard(() => native.automationInventorySelect(selectionJson: jsonEncode(selection))))
        as AutomationJson,
  );
  @override
  Future<AutomationJson> inspectWindowsPrerequisites({
    required String operationId,
    required String profileId,
    required String notificationScopeId,
  }) async => jsonDecode(await _automationGuard(() => native.automationWindowsPrerequisitesJson(
    requestJson: jsonEncode({'version':1,'operationId':operationId,'profileId':profileId,'notificationScopeId':notificationScopeId}),
  ))) as AutomationJson;

  @override
  Future<AutomationJson> setupRunner(
    AutomationJson target, {
    required String operationId,
    bool install = false,
  }) async => jsonDecode(
    await _automationGuard(
      () => native.automationRunnerSetup(operationId: operationId, targetJson: jsonEncode(target), install: install),
    ),
  ) as AutomationJson;
  @override
  Future<void> cancelRunnerSetup(String operationId) =>
      _automationGuard(() => native.automationRunnerCancelSetup(operationId: operationId));
  @override
  Future<void> openProjectEditor(
    String projectId,
    String path, {
    required String expectedSha256,
    required AutomationJson editor,
  }) => _automationGuard(
    () => native.automationProjectOpenEditor(
      id: projectId,
      path: path,
      expectedSha256: expectedSha256,
      editorJson: jsonEncode(editor),
    ),
  );
  @override
  Future<AutomationJson> projectDiff(String projectId, {required String currentDigest, int offset = 0}) async =>
      jsonDecode(
        await _automationGuard(
          () => native.automationProjectDiff(id: projectId, currentDigest: currentDigest, offset: offset, limit: 50),
        ),
      ) as AutomationJson;
  @override
  Future<AutomationJson> projectFileDiff(String projectId, String path, {required String currentDigest}) async =>
      jsonDecode(
        await _automationGuard(
          () => native.automationProjectFileDiff(id: projectId, path: path, currentDigest: currentDigest),
        ),
      ) as AutomationJson;
  @override
  Future<AutomationJson> inventoryConfiguration(String projectId) async =>
      jsonDecode(await _automationGuard(() => native.automationInventoryConfiguration(id: projectId)))
          as AutomationJson;
  @override
  Future<AutomationJson> startInventory(String projectId, String runnerId, List<AutomationJson> sources) async =>
      jsonDecode(
        await _automationGuard(
          () => native.automationInventoryStart(
            projectId: projectId,
            runnerId: runnerId,
            sourcesJson: jsonEncode(sources),
          ),
        ),
      ) as AutomationJson;
  @override
  Future<AutomationJson> inventoryPage(String id, {String query = '', int offset = 0}) async => jsonDecode(
    await _automationGuard(() => native.automationInventoryPage(id: id, query: query, offset: offset, limit: 50)),
  ) as AutomationJson;
  @override
  Future<void> discardInventory(String id) => _automationGuard(() => native.automationInventoryDiscard(id: id));
  @override
  Future<AutomationJson> checkInventory(String id, AutomationJson configuration) async => jsonDecode(
    await _automationGuard(() => native.automationInventoryCheck(id: id, configurationJson: jsonEncode(configuration))),
  ) as AutomationJson;
  @override
  Future<AutomationProject> commitInventory(String id, AutomationJson configuration) async =>
      AutomationProject.fromJson(
        jsonDecode(
          await _automationGuard(
            () => native.automationInventoryCommit(id: id, configurationJson: jsonEncode(configuration)),
          ),
        ) as AutomationJson,
      );
  @override
  Future<AutomationProject> clearInventory(String projectId, int revision) async => AutomationProject.fromJson(
    jsonDecode(await _automationGuard(() => native.automationInventoryClear(id: projectId, expectedRevision: revision)))
        as AutomationJson,
  );
  @override
  Future<AutomationJson> inventory(String projectId) async =>
      jsonDecode(await _automationGuard(() => native.automationProjectInventory(id: projectId))) as AutomationJson;
  @override
  Future<AutomationJson> folderBinding(
    String projectId, {
    required String profileId,
    required String notificationScopeId,
    required int revision,
    required String groupName,
    required AutomationFolderBindingAction action,
    String? folderId,
  }) async {
    final operation = switch (action) {
      AutomationFolderBindingAction.linkDirect => 'link_direct',
      AutomationFolderBindingAction.snapshotDirect => 'snapshot_direct',
      AutomationFolderBindingAction.unlink => 'unlink',
    };
    return jsonDecode(
      await _automationGuard(
        () => native.automationProjectFolderBindingJson(
          requestJson: jsonEncode({
            'version': 1,
            'project_id': projectId,
            'expected_revision': revision,
            'profile_id': profileId,
            'notification_scope_id': notificationScopeId,
            'action': {
              'type': operation,
              'name': groupName,
              if (action != AutomationFolderBindingAction.unlink) 'folder_id': folderId,
            },
          }),
        ),
      ),
    ) as AutomationJson;
  }

  @override
  Future<AutomationProject> saveInventory(
    String projectId, {
    required int revision,
    required AutomationJson inventory,
  }) async => AutomationProject.fromJson(
    jsonDecode(
      await _automationGuard(
        () => native.automationProjectSaveInventory(
          id: projectId,
          expectedRevision: revision,
          inventoryJson: jsonEncode(inventory),
        ),
      ),
    ) as AutomationJson,
  );
  @override
  Future<AutomationProject> startGit({
    required String name,
    required String runnerId,
    required String url,
    required String reference,
    String? caPem,
  }) async => AutomationProject.fromJson(
    jsonDecode(
      await _automationGuard(
        () => native.automationProjectGitStart(
          name: name,
          runnerId: runnerId,
          url: url,
          reference: reference,
          caPem: caPem,
        ),
      ),
    ) as AutomationJson,
  );
  @override
  Future<void> cancelGit(String id) => _automationGuard(() => native.automationProjectGitCancel(id: id));
  @override
  Future<AutomationJson> prepareAi({
    required String providerId,
    required String task,
    required String path,
    String? selectedText,
  }) async => jsonDecode(
    await _automationGuard(
      () => native.automationAiPrepare(providerId: providerId, task: task, path: path, selectedText: selectedText),
    ),
  ) as AutomationJson;
  @override
  Future<AutomationJson> generateAi(String reviewId) async =>
      jsonDecode(await _automationGuard(() => native.automationAiGenerate(reviewId: reviewId))) as AutomationJson;
  @override
  Future<void> discardAi(String reviewId) => _automationGuard(() => native.automationAiDiscard(reviewId: reviewId));

  @override
  Future<AutomationJson> captureAiContext({
    required String operationId,
    required String profileId,
    required String notificationScopeId,
    required String projectId,
    required int expectedProjectRevision,
    required AutomationAiContextKind kind,
    required String subjectId,
  }) async => jsonDecode(await _automationGuard(() => native.automationAiContextCaptureJson(
    requestJson: jsonEncode({
      'operationId': operationId, 'profileId': profileId,
      'notificationScopeId': notificationScopeId, 'projectId': projectId,
      'expectedProjectRevision': expectedProjectRevision,
      'kind': kind.name, 'subjectId': subjectId,
    }),
  ))) as AutomationJson;

  @override
  Future<void> cancelAiContext(String operationId) =>
      _automationGuard(() => native.automationAiContextCancel(operationId: operationId));

  @override
  Future<AutomationJson> prepareAiWithContext({
    required String profileId,
    required String notificationScopeId,
    required String projectId,
    required int expectedProjectRevision,
    String? selectedHostId,
    required String providerId,
    required String task,
    required String path,
    String? selectedText,
    required List<AutomationJson> selections,
    required List<String> selectedGroups,
    required List<String> selectedTags,
  }) async => jsonDecode(await _automationGuard(() => native.automationAiPrepareContextJson(
    requestJson: jsonEncode({
      'profileId': profileId, 'notificationScopeId': notificationScopeId,
      'projectId': projectId, 'expectedProjectRevision': expectedProjectRevision,
      'selectedHostId': selectedHostId, 'providerId': providerId,
      'task': task, 'path': path, 'selectedText': selectedText,
      'selections': selections, 'selectedGroups': selectedGroups,
      'selectedTags': selectedTags,
    }),
  ))) as AutomationJson;

  @override
  Future<AutomationJson> jobEvents(String id, {required int after, int limit = 50}) async =>
      jsonDecode(await _automationGuard(() => native.automationJobEvents(id: id, after: after, limit: limit)))
          as AutomationJson;
  @override
  Future<AutomationJson> prepareRun(AutomationJson request) async =>
      jsonDecode(await _automationGuard(() => native.automationPrepareRun(requestJson: jsonEncode(request))))
          as AutomationJson;
  @override
  Future<void> discardReview(String id) => _automationGuard(() => native.automationDiscardReview(reviewId: id));
  @override
  Future<AutomationJson> confirmRun(String reviewId) async =>
      jsonDecode(await _automationGuard(() => native.automationConfirmRun(reviewId: reviewId))) as AutomationJson;
  @override
  Future<AutomationJson> confirmOwnerRun(String reviewId, {required StepUpProof proof}) async =>
      jsonDecode(await withStepUp(proof, (passphrase, token) => _automationGuard(() =>
        native.automationConfirmOwnerRun(reviewId: reviewId, passphrase: passphrase, presenceToken: token)))) as AutomationJson;
  @override
  Future<AutomationJson> jobs() async => jsonDecode(await _automationGuard(native.automationJobs)) as AutomationJson;
  @override
  Future<void> cancelJob(String id) => _automationGuard(() => native.automationCancelJob(id: id));
  @override
  Future<void> acknowledgeUnknown(String id, {required bool acceptPartialChanges}) =>
      _automationGuard(() => native.automationAcknowledgeUnknown(id: id, acceptPartialChanges: acceptPartialChanges));
  @override
  Future<void> disconnectRunner(String id) => _automationGuard(() => native.automationRunnerDisconnect(id: id));
  @override
  Future<void> forgetRunner(String id, {required int revision}) =>
      _automationGuard(() => native.automationRunnerForget(id: id, expectedRevision: revision));
  @override
  Future<AutomationRunnerCatalog> runners() async =>
      AutomationRunnerCatalog.fromJson(jsonDecode(await _automationGuard(native.automationRunners)) as AutomationJson);
  @override
  Future<AutomationRunnerProfile> createRunner({required String name, required AutomationJson configuration}) async =>
      AutomationRunnerProfile.fromJson(
        jsonDecode(
          await _automationGuard(
            () => native.automationRunnerCreate(name: name, configurationJson: jsonEncode(configuration)),
          ),
        ) as AutomationJson,
      );
  @override
  Future<AutomationJson> connectRunner(String id) async =>
      jsonDecode(await _automationGuard(() => native.automationRunnerConnect(id: id))) as AutomationJson;
  @override
  Future<AutomationProjectCatalog> projects() async => AutomationProjectCatalog.fromJson(
    jsonDecode(await _automationGuard(native.automationProjects)) as AutomationJson,
  );

  @override
  Future<AutomationProject> createProject({required String name, String? directory}) async =>
      AutomationProject.fromJson(
        jsonDecode(await _automationGuard(() => native.automationProjectCreate(name: name, directory: directory)))
            as AutomationJson,
      );

  @override
  Future<AutomationProject> renameProject(String id, {required int revision, required String name}) async =>
      AutomationProject.fromJson(
        jsonDecode(
          await _automationGuard(() => native.automationProjectRename(id: id, expectedRevision: revision, name: name)),
        ) as AutomationJson,
      );

  @override
  Future<AutomationProjectRevision> files(String projectId) async => AutomationProjectRevision.fromJson(
    jsonDecode(await _automationGuard(() => native.automationProjectFiles(id: projectId))) as AutomationJson,
  );

  @override
  Future<AutomationProjectFile> readFile(String projectId, String path) async => AutomationProjectFile.fromJson(
    jsonDecode(await _automationGuard(() => native.automationProjectFile(id: projectId, path: path))) as AutomationJson,
  );

  @override
  Future<AutomationProjectFile> saveFile(
    String projectId, {
    required String path,
    required String text,
    String? expectedSha256,
  }) async => AutomationProjectFile.fromJson(
    jsonDecode(
      await _automationGuard(
        () => native.automationProjectSave(id: projectId, path: path, expectedSha256: expectedSha256, text: text),
      ),
    ) as AutomationJson,
  );
}

Future<T> _automationGuard<T>(Future<T> Function() action) async {
  try {
    return await guard(action);
  } on AppException catch (error) {
    final exportReason = inventoryExportNativeReason(error.args['field'], error.args['rule']);
    if (exportReason != null) throw InventoryExportFailure(exportReason);
    final cleanupReason = runnerCleanupNativeReason(error.args['field'], error.args['rule']);
    if (cleanupReason != null) throw AutomationRunnerCleanupFailed(cleanupReason);
    if (const {'automation_ai_response', 'automation_ai_provider'}.contains(error.args['field'])) {
      throw AutomationAiFailed(error.args['rule'] ?? 'provider_rejected');
    }
    if (error.args['field'] == 'automation_runner' &&
        const {
          'prerequisites_changed',
          'ssh_environment_changed',
          'ssh_account_changed',
          'ssh_route_unsupported',
          'prerequisite_terminal_unavailable',
          'invalid_prerequisite_command',
          'invalid_prerequisite_packages',
        }.contains(error.args['rule'])) {
      throw AutomationRunnerPrerequisiteFailed(error.args['rule']!);
    }
    if (error.args['field'] == 'automation_runner' &&
        const {
          'installation_failed',
          'prerequisites_missing',
          'setup_failed',
          'setup_bootstrap_failed',
          'setup_command_too_large',
          'setup_python_missing',
          'setup_unavailable',
          'invalid_package',
          'unsafe_directory',
          'unsafe_home',
          'non_root_required',
        }.contains(error.args['rule'])) {
      throw AutomationRunnerSetupFailed(error.args['rule']!);
    }
    if (error.args['field'] == 'automation_folder_binding') {
      throw const AutomationFolderBindingChanged();
    }
    if (error.args['field'] == 'automation_runner_update') {
      throw const AutomationRunnerUpdateRequired();
    }
    if (error.args['field'] == 'automation_file' && error.args['rule'] == 'changed') {
      throw const AutomationFileConflict();
    }
    rethrow;
  }
}
