import 'package:consolecrypt/core/models/step_up.dart';

typedef AutomationJson = Map<String, dynamic>;
const automationGitSourceFile = '.consolecrypt-git-source.json';

/// Metadata only. Passwords stay in Native custody and never enter this port.
abstract interface class AutomationVaultSelectionService {
  Future<AutomationJson> vaultContext(String projectId);
  Future<AutomationJson> selectVault(AutomationJson selection);
}

abstract interface class AutomationDiffService {
  Future<AutomationJson> projectDiff(String projectId, {required String currentDigest, int offset = 0});
  Future<AutomationJson> projectFileDiff(String projectId, String path, {required String currentDigest});
}

abstract interface class AutomationEditorService {
  Future<void> openProjectEditor(
    String projectId,
    String path, {
    required String expectedSha256,
    required AutomationJson editor,
  });
}

abstract interface class AutomationScaffoldService {
  Future<AutomationProjectFile> createRole(String projectId, String name);
}

abstract interface class AutomationSourceInventoryService {
  Future<AutomationJson> inventoryConfiguration(String projectId);
  Future<AutomationJson> startInventory(String projectId, String runnerId, List<AutomationJson> sources);
  Future<AutomationJson> inventoryPage(String id, {String query = '', int offset = 0});
  Future<void> discardInventory(String id);
  Future<AutomationJson> checkInventory(String id, AutomationJson configuration);
  Future<AutomationProject> commitInventory(String id, AutomationJson configuration);
  Future<AutomationProject> clearInventory(String projectId, int revision);
}

abstract interface class AutomationInventoryService {
  Future<AutomationJson> inventory(String projectId);
  Future<AutomationProject> saveInventory(String projectId, {required int revision, required AutomationJson inventory});
}

final class AutomationFolderBindingChanged implements Exception {
  const AutomationFolderBindingChanged();
}

enum AutomationFolderBindingAction { linkDirect, snapshotDirect, unlink }

abstract interface class AutomationFolderBindingService {
  Future<AutomationJson> folderBinding(
    String projectId, {
    required String profileId,
    required String notificationScopeId,
    required int revision,
    required String groupName,
    required AutomationFolderBindingAction action,
    String? folderId,
  });
}

abstract interface class AutomationInventorySelectionService {
  Future<AutomationProject> selectInventory(AutomationJson selection);
}

abstract interface class AutomationGitService {
  Future<AutomationProject> startGit({
    required String name,
    required String runnerId,
    required String url,
    required String reference,
    String? caPem,
  });
  Future<void> cancelGit(String id);
}

abstract interface class AutomationAiService {
  Future<AutomationJson> prepareAi({
    required String providerId,
    required String task,
    required String path,
    String? selectedText,
  });
  Future<AutomationJson> generateAi(String reviewId);
  Future<void> discardAi(String reviewId);
}

/// Optional fixed-purpose context capture. Selecting metadata never performs
/// a capture. Facts are Native-issued references, not caller-provided values.
enum AutomationAiContextKind { hostOs, runnerVersions }

abstract interface class AutomationAiContextService {
  Future<AutomationJson> captureAiContext({
    required String operationId,
    required String profileId,
    required String notificationScopeId,
    required String projectId,
    required int expectedProjectRevision,
    required AutomationAiContextKind kind,
    required String subjectId,
  });
  Future<void> cancelAiContext(String operationId);

  /// Native resolves captureId/fieldIds and current project groups/host tags.
  /// This does not accept caller-authored OS facts or a general command.
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
  });
}

abstract interface class AutomationEventService {
  Future<AutomationJson> jobEvents(String id, {required int after, int limit = 50});
}

/// Only an opaque native review ID can confirm execution. The UI never signs
/// or sends a manifest and a model proposal cannot implement this interface.
abstract interface class AutomationJobService {
  Future<AutomationJson> prepareRun(AutomationJson request);
  Future<void> discardReview(String id);
  Future<AutomationJson> confirmRun(String reviewId);
  Future<AutomationJson> jobs();
  Future<void> cancelJob(String id);
  Future<void> acknowledgeUnknown(String id, {required bool acceptPartialChanges});
}

/// An optional Native capability for an additional owner agreement over an
/// already frozen production review. No manifest or permission is minted here.
abstract interface class AutomationProductionOwnerService {
  Future<AutomationJson> confirmOwnerRun(String reviewId, {required StepUpProof proof});
}

final class AutomationFileConflict implements Exception {
  const AutomationFileConflict();
}

final class AutomationRunnerUpdateRequired implements Exception {
  const AutomationRunnerUpdateRequired();
}

final class AutomationProject {
  const AutomationProject({
    required this.id,
    required this.name,
    required this.readOnly,
    this.sourceKind,
    this.path,
    this.revision,
    this.importState,
    this.importError,
    this.gitSource,
  });
  factory AutomationProject.fromJson(AutomationJson value) => AutomationProject(
    id: value['id'] as String,
    name: value['name'] as String?,
    readOnly: value['read_only'] as bool,
    sourceKind: value['source_kind'] as String?,
    path: value['path'] as String?,
    revision: value['revision'] as int?,
    importState: value['import_state'] as String?,
    importError: value['import_error'] as String?,
    gitSource: value['git_source'] as AutomationJson?,
  );
  final String id;
  final String? name, sourceKind, path;
  final int? revision;
  final bool readOnly;
  final String? importState, importError;
  final AutomationJson? gitSource;
}

final class AutomationProjectCatalog {
  const AutomationProjectCatalog({
    required this.version,
    required this.readOnly,
    required this.deviceLocal,
    required this.projects,
  });
  factory AutomationProjectCatalog.fromJson(AutomationJson value) => AutomationProjectCatalog(
    version: value['schema_version'] as int,
    readOnly: value['read_only'] as bool,
    deviceLocal: value['device_local'] as bool,
    projects: [for (final project in value['projects'] as List) AutomationProject.fromJson(project as AutomationJson)],
  );
  final int version;
  final bool readOnly, deviceLocal;
  final List<AutomationProject> projects;
}

final class AutomationFileEntry {
  const AutomationFileEntry({required this.path, required this.directory, this.bytes = 0});
  factory AutomationFileEntry.fromJson(AutomationJson value) => AutomationFileEntry(
    path: value['path'] as String,
    directory: value['type'] == 'directory',
    bytes: value['bytes'] as int? ?? 0,
  );
  final String path;
  final bool directory;
  final int bytes;
}

final class AutomationProjectRevision {
  const AutomationProjectRevision({required this.digest, required this.bytes, required this.entries});
  factory AutomationProjectRevision.fromJson(AutomationJson value) => AutomationProjectRevision(
    digest: value['digest'] as String,
    bytes: value['bytes'] as int,
    entries: [for (final entry in value['entries'] as List) AutomationFileEntry.fromJson(entry as AutomationJson)],
  );
  final String digest;
  final int bytes;
  final List<AutomationFileEntry> entries;
}

final class AutomationProjectFile {
  const AutomationProjectFile({
    required this.path,
    required this.text,
    required this.sha256,
    required this.lineEndings,
  });
  factory AutomationProjectFile.fromJson(AutomationJson value) => AutomationProjectFile(
    path: value['path'] as String,
    text: value['text'] as String,
    sha256: value['sha256'] as String,
    lineEndings: value['line_endings'] as String,
  );
  final String path, text, sha256, lineEndings;
}

/// Project files work without Python, Ansible or a connected Runner.
abstract interface class AutomationService {
  Future<AutomationProjectCatalog> projects();
  Future<AutomationProject> createProject({required String name, String? directory});
  Future<AutomationProject> renameProject(String id, {required int revision, required String name});
  Future<AutomationProjectRevision> files(String projectId);
  Future<AutomationProjectFile> readFile(String projectId, String path);
  Future<AutomationProjectFile> saveFile(
    String projectId, {
    required String path,
    required String text,
    String? expectedSha256,
  });
}

final class AutomationRunnerProfile {
  const AutomationRunnerProfile({
    required this.id,
    required this.readOnly,
    this.name,
    this.revision,
    this.configuration,
  });
  factory AutomationRunnerProfile.fromJson(AutomationJson value) => AutomationRunnerProfile(
    id: value['id'] as String,
    readOnly: value['read_only'] as bool,
    name: value['name'] as String?,
    revision: value['revision'] as int?,
    configuration: value['configuration'] as AutomationJson?,
  );
  final String id;
  final bool readOnly;
  final String? name;
  final int? revision;
  final AutomationJson? configuration;
}

final class AutomationRunnerCatalog {
  const AutomationRunnerCatalog({required this.readOnly, required this.runners});
  factory AutomationRunnerCatalog.fromJson(AutomationJson value) => AutomationRunnerCatalog(
    readOnly: value['read_only'] as bool,
    runners: [for (final row in value['runners'] as List) AutomationRunnerProfile.fromJson(row as AutomationJson)],
  );
  final bool readOnly;
  final List<AutomationRunnerProfile> runners;
}

abstract interface class AutomationRunnerService {
  Future<AutomationRunnerCatalog> runners();
  Future<AutomationRunnerProfile> createRunner({required String name, required AutomationJson configuration});
  Future<AutomationJson> connectRunner(String id);
  Future<void> disconnectRunner(String id);
  Future<void> forgetRunner(String id, {required int revision});
}

abstract interface class AutomationRunnerEditingService {
  Future<AutomationRunnerProfile> updateRunner(
    String id, {
    required int revision,
    required String name,
    required AutomationJson configuration,
  });
}

/// Optional local setup; remote Runner configuration remains explicit.
abstract interface class AutomationRunnerSetupService {
  Future<AutomationJson> setupRunner(AutomationJson target, {required String operationId, bool install = false});
  Future<void> cancelRunnerSetup(String operationId);
}

abstract interface class AutomationRunnerPrerequisiteTerminalService {
  Future<String> openPrerequisiteTerminal(
    AutomationJson target, {
    required String operationId,
    required String command,
  });
}

final class AutomationRunnerPrerequisiteFailed implements Exception {
  const AutomationRunnerPrerequisiteFailed(this.reason);
  final String reason;
}

final class AutomationRunnerSetupFailed implements Exception {
  const AutomationRunnerSetupFailed([this.reason = 'setup_failed']);
  final String reason;
}

final class AutomationAiFailed implements Exception {
  const AutomationAiFailed(this.reason);
  final String reason;
}
