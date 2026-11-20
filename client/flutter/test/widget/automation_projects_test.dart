import 'dart:async';
import 'dart:convert';
import 'dart:io' as io;

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/automation/automation_ai_draft.dart';
import 'package:consolecrypt/automation/automation_inventory_export.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secure_clipboard.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/widgets/secret_exposure.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_ui_proof.dart';
import '../helpers/test_app.dart';

// Deliberately synthetic service for UI lifecycle tests. Actual disk/SQLCipher
// and SSH/Ansible execution are covered separately by native integration tests.
class _Projects implements AutomationService {
  static const project = AutomationProject(id: 'p', name: 'Fixture project', readOnly: false, revision: 1);
  String text = '---\n- hosts: all\n';
  String hash = 'initial-hash';
  String? savedExpected;
  bool conflict = false;
  Completer<AutomationProjectFile>? pendingRead;
  @override
  Future<AutomationProjectCatalog> projects() async =>
      const AutomationProjectCatalog(version: 1, readOnly: false, deviceLocal: true, projects: [project]);
  @override
  Future<AutomationProjectRevision> files(String projectId) async => const AutomationProjectRevision(
    digest: 'fixture',
    bytes: 20,
    entries: [AutomationFileEntry(path: 'playbook.yml', directory: false)],
  );
  @override
  Future<AutomationProjectFile> readFile(String projectId, String path) async => pendingRead == null
      ? AutomationProjectFile(path: path, text: text, sha256: hash, lineEndings: 'lf')
      : pendingRead!.future;
  @override
  Future<AutomationProjectFile> saveFile(
    String projectId, {
    required String path,
    required String text,
    String? expectedSha256,
  }) async {
    savedExpected = expectedSha256;
    if (conflict) throw const AutomationFileConflict();
    this.text = text;
    hash = 'saved-hash';
    return AutomationProjectFile(path: path, text: text, sha256: hash, lineEndings: 'lf');
  }

  @override
  Future<AutomationProject> createProject({required String name, String? directory}) async => project;
  @override
  Future<AutomationProject> renameProject(String id, {required int revision, required String name}) async => project;
}

class _Scaffolds extends _Projects implements AutomationScaffoldService {
  String? createdRole;
  Completer<AutomationProjectFile>? pendingRole;
  @override
  Future<AutomationProjectFile> createRole(String projectId, String name) async {
    createdRole = name;
    return pendingRole?.future ??
        AutomationProjectFile(
          path: 'roles/$name/tasks/main.yml',
          text: '---\n[]\n',
          sha256: 'role-sha',
          lineEndings: 'lf',
        );
  }
}

class _EditorPicker extends Fake implements FileDialogService {
  Completer<String?>? pending;
  String? choice = 'C:/Owned Editor/editor.exe';
  int calls = 0;
  @override
  Future<String?> chooseApplication({required String label, required String confirmButtonText}) async {
    calls++;
    return pending?.future ?? choice;
  }
}

class _Editor extends _Projects implements AutomationEditorService {
  final picker = _EditorPicker();
  final opened = <AutomationJson>[];
  @override
  Future<void> openProjectEditor(
    String projectId,
    String path, {
    required String expectedSha256,
    required AutomationJson editor,
  }) async {
    opened.add({'id': projectId, 'path': path, 'sha256': expectedSha256, 'editor': editor});
  }
}

class _DirectoryPicker extends Fake implements FileDialogService {
  String? choice = r'C:\fixtures\My Ansible Project\';
  int calls = 0;
  @override
  Future<String?> chooseDirectory() async {
    calls++;
    return choice;
  }
}

class _FolderProjects extends _Projects {
  final picker = _DirectoryPicker();
  AutomationJson? created;
  @override
  Future<AutomationProject> createProject({required String name, String? directory}) async {
    created = {'name': name, 'directory': directory};
    return _Projects.project;
  }
}

class _ConfigProjects extends _Projects {
  String? savedPath;
  @override
  Future<AutomationProjectRevision> files(String projectId) async => const AutomationProjectRevision(
    digest: 'config-fixture',
    bytes: 100,
    entries: [AutomationFileEntry(path: 'ansible.cfg', directory: false)],
  );
  @override
  Future<AutomationProjectFile> saveFile(
    String projectId, {
    required String path,
    required String text,
    String? expectedSha256,
  }) async {
    savedPath = path;
    return super.saveFile(projectId, path: path, text: text, expectedSha256: expectedSha256);
  }
}

class _EditingRunners extends _Runners implements AutomationRunnerEditingService {
  final actions = <String>[];
  AutomationJson? updated;
  bool blocked = false;
  @override
  Future<AutomationRunnerCatalog> runners() async => const AutomationRunnerCatalog(
    readOnly: false,
    runners: [
      AutomationRunnerProfile(
        id: 'runner',
        name: 'Existing Runner',
        revision: 3,
        readOnly: false,
        configuration: {
          'kind': 'local',
          'launch': {
            'kind': 'native',
            'python': '/opt/cc/bin/python',
            'module_dir': '/opt/cc/src',
            'root': '/home/fixture/private',
          },
        },
      ),
    ],
  );
  @override
  Future<void> disconnectRunner(String id) async {
    actions.add('disconnect');
    if (blocked) throw StateError('Active job fixture');
  }

  @override
  Future<AutomationRunnerProfile> updateRunner(
    String id, {
    required int revision,
    required String name,
    required AutomationJson configuration,
  }) async {
    actions.add('update');
    updated = {'id': id, 'revision': revision, 'name': name, 'configuration': configuration};
    return AutomationRunnerProfile(
      id: id,
      name: name,
      revision: revision + 1,
      readOnly: false,
      configuration: configuration,
    );
  }
}

class _Runners extends _Projects implements AutomationRunnerService {
  Completer<AutomationJson>? pendingProbe;
  bool ready = false;
  @override
  Future<AutomationRunnerCatalog> runners() async => const AutomationRunnerCatalog(
    readOnly: false,
    runners: [
      AutomationRunnerProfile(
        id: 'runner',
        name: 'Remote fixture',
        readOnly: false,
        revision: 1,
        configuration: {'kind': 'remote'},
      ),
    ],
  );
  @override
  Future<AutomationRunnerProfile> createRunner({required String name, required AutomationJson configuration}) async =>
      AutomationRunnerProfile(id: 'new', name: name, readOnly: false, revision: 1, configuration: configuration);
  @override
  Future<AutomationJson> connectRunner(String id) async => pendingProbe == null
      ? {
          'probe': {
            'system': 'Linux',
            'machine': 'x86_64',
            'python': '3.12.3',
            'packages': <String, dynamic>{
              for (final entry in {
                'PyYAML': '6.0.3',
                'ansible-core': '2.20.11',
                'ansible-lint': '26.11.0',
                'ansible-runner': '2.4.3',
                'cryptography': '50.0.2',
              }.entries)
                entry.key: {'actual': entry.value, 'expected': entry.value, 'compatible': true},
            },
            'dependencies_ready': ready,
            'non_root': true,
            'sandbox': {'filesystem': true, 'network': ready, 'pid': true},
          },
        }
      : pendingProbe!.future;
  @override
  Future<void> disconnectRunner(String id) async {}
  @override
  Future<void> forgetRunner(String id, {required int revision}) async {}
}

class _Ai extends _Projects implements AutomationAiService {
  String? previewText;
  final provider = AiProviderConfig.draft(AiProviderKind.openaiCompatible)
      .copyWith(name: 'AI fixture', chatModel: 'fixture-model');
  int generations = 0;
  String? includedContext, discarded;
  Completer<AutomationJson>? pendingProposal;
  Exception? generationError;
  AutomationJson proposal() => {
    'version': 1,
    'description': 'Fixture proposal',
    'files': [
      {'path': 'playbook.yml', 'content': '---\n# proposed draft only\n- hosts: all\n  tasks: []\n'},
    ],
    'variables': <AutomationJson>[],
    'dependencies': <AutomationJson>[],
    'expected_changes': ['Fixture change'],
    'checks': ['Fixture assertion'],
    'warnings': <String>[],
  };
  @override
  Future<AutomationJson> prepareAi({
    required String providerId,
    required String task,
    required String path,
    String? selectedText,
  }) async {
    includedContext = selectedText;
    return {
      'review_id': 'ai-review-fixture',
      'provider_name': 'AI fixture',
      'locality': 'public',
      'preview': previewText ?? 'Filtered context fixture',
    };
  }

  @override
  Future<AutomationJson> generateAi(String reviewId) async {
    generations++;
    if (generationError case final failure?) throw failure;
    return pendingProposal == null ? proposal() : pendingProposal!.future;
  }

  @override
  Future<void> discardAi(String reviewId) async {
    discarded = reviewId;
  }
}

class _AiRequirements extends _Ai {
  String? requirements;
  bool contractConflict = false;
  final writes = <String>[];
  @override
  AutomationJson proposal() => {
    ...super.proposal(),
    'variables': [
      {'name': 'fixture_input', 'description': 'Fixture value', 'required': true},
    ],
    'dependencies': [
      {'source': 'role:demo', 'version': '1.2.3'},
    ],
  };
  @override
  Future<AutomationProjectRevision> files(String projectId) async {
    final tree = await super.files(projectId);
    return AutomationProjectRevision(
      digest: tree.digest,
      bytes: tree.bytes,
      entries: [
        ...tree.entries,
        if (requirements != null) const AutomationFileEntry(path: automationRequirementsFile, directory: false),
      ],
    );
  }

  @override
  Future<AutomationProjectFile> readFile(String projectId, String path) async => path == automationRequirementsFile
      ? AutomationProjectFile(path: path, text: requirements!, sha256: 'contract-hash', lineEndings: 'lf')
      : super.readFile(projectId, path);
  @override
  Future<AutomationProjectFile> saveFile(
    String projectId, {
    required String path,
    required String text,
    String? expectedSha256,
  }) async {
    if (path == automationRequirementsFile) {
      if (contractConflict || (requirements == null ? expectedSha256 != null : expectedSha256 != 'contract-hash')) {
        throw const AutomationFileConflict();
      }
      writes.add(path);
      requirements = text;
      return AutomationProjectFile(path: path, text: text, sha256: 'contract-hash', lineEndings: 'lf');
    }
    final result = await super.saveFile(projectId, path: path, text: text, expectedSha256: expectedSha256);
    writes.add(path);
    return result;
  }
}

class _SetupRunners extends _Runners implements AutomationRunnerSetupService {
  final remoteHost = Host.create(name: 'Runner SSH fixture', address: '192.0.2.44');
  final targets = <AutomationJson>[];
  final calls = <bool>[];
  final operations = <String>[];
  Completer<AutomationJson>? pendingSetup;
  AutomationJson? created;
  bool missing = false, existing = false, outdated = false;
  static const paths = {
    'python': '/home/fixture/runner/venv/bin/python',
    'module_dir': '/home/fixture/runner/src',
    'root': '/home/fixture/runner/state',
  };
  @override
  Future<AutomationJson> setupRunner(AutomationJson target, {required String operationId, bool install = false}) async {
    calls.add(install);
    targets.add(Map.of(target));
    operations.add(operationId);
    return pendingSetup?.future ??
        (install
            ? {
                'paths': outdated
                    ? {
                        ...paths,
                        'python': '/home/fixture/runner-new/venv/bin/python',
                        'module_dir': '/home/fixture/runner-new/src',
                        'root': '/home/fixture/runner-new/state',
                      }
                    : paths,
                'probe': <String, dynamic>{},
              }
            : {
                'version': 1,
                'system': 'Linux',
                'machine': 'x86_64',
                'python': '3.12.3',
                'user': 'fixture',
                'users': ['fixture'],
                'non_root': true,
                'install_supported': !missing,
                'install_parent': '/home/fixture/private',
                'missing': missing ? ['python_venv'] : <String>[],
                'candidates': existing ? [paths] : <AutomationJson>[],
                'outdated_candidates': existing && outdated ? [paths['python']] : <String>[],
              });
  }

  final cancelled = <String>[];
  @override
  Future<void> cancelRunnerSetup(String operationId) async {
    cancelled.add(operationId);
  }

  @override
  Future<AutomationRunnerProfile> createRunner({required String name, required AutomationJson configuration}) async {
    created = configuration;
    return super.createRunner(name: name, configuration: configuration);
  }
}

class _WslTerminal extends Fake implements TerminalService, LocalTerminalService {
  Completer<LocalShellProfiles>? pending;
  int lists = 0;
  LocalShellProfiles value = const LocalShellProfiles(
    profiles: [
      LocalShellProfile(id: 'wsl:Alpha', kind: 'wsl', label: 'Alpha', shell: 'sh', distribution: 'Alpha'),
      LocalShellProfile(id: 'wsl:Beta', kind: 'wsl', label: 'Beta', shell: 'sh', distribution: 'Beta'),
    ],
  );
  @override
  Future<LocalShellProfiles> listLocalProfiles() async {
    lists++;
    return pending?.future ?? value;
  }
}

class _WslSetup extends _SetupRunners {
  final terminal = _WslTerminal();
  bool osKnown = true;
  @override
  Future<AutomationJson> setupRunner(AutomationJson target, {required String operationId, bool install = false}) async {
    final result = await super.setupRunner(target, operationId: operationId, install: install);
    if (!install) {
      return {
        ...result,
        'release': 'synthetic-kernel-wsl2',
        'wsl2': true,
        'os_release': osKnown
            ? {'id': 'fixture-os', 'version_id': '24.04', 'pretty_name': 'Actual captured distribution'}
            : 'unknown future shape',
      };
    }
    return {
      ...result,
      'probe': {
        'helper': 'fixture-helper-17',
        'system': 'Linux',
        'machine': 'x86_64',
        'python': '3.12.3',
        'non_root': true,
        'dependencies_ready': true,
        'environment_digest': 'fixture-install-env',
        'sandbox': {'filesystem': true, 'network': true, 'pid': true},
        'tools': {'ssh': true},
        'packages': {
          'ansible-core': {'actual': 'fixture-core-version', 'compatible': true},
          'ansible-runner': {'actual': 'fixture-runner-version', 'compatible': true},
          'ansible-lint': {'actual': 'fixture-lint-version', 'compatible': true},
        },
      },
    };
  }
}

class _PrerequisiteRuntime extends Fake implements TerminalService, RuntimeTerminalService {
  final closed = <TerminalSessionId>[];
  @override
  Future<RuntimeTerminalAttachment> attachRuntime(String runtimeId, {TerminalSize size = TerminalSize.initial}) async =>
      RuntimeTerminalAttachment(
        target: const LocalTerminalTarget(
          LocalShellProfile(id: 'runner-prerequisites', kind: 'cli', label: 'Runner setup fixture', shell: 'sh'),
        ),
        handle: TerminalSessionHandle(
          id: const TerminalSessionId('fixture-setup-terminal'),
          output: Stream.value(Uint8List.fromList(utf8.encode('[sudo] password for fixture: '))),
          events: Stream.value(const TerminalStateChanged(SessionConnectionState.connected)),
        ),
      );
  @override
  Future<void> close(TerminalSessionId id) async {
    closed.add(id);
  }

  @override
  Future<void> resize(TerminalSessionId id, TerminalSize size) async {}
  @override
  Future<void> write(TerminalSessionId id, Uint8List data) async {}
}

class _PrerequisiteSetup extends _SetupRunners implements AutomationRunnerPrerequisiteTerminalService {
  static const command = 'sudo apt-get update && sudo apt-get install --no-install-recommends python3-venv';
  final terminal = _PrerequisiteRuntime();
  AutomationJson? opened;
  Completer<String>? pendingTerminal;
  String? prerequisiteFailure;
  @override
  Future<AutomationJson> setupRunner(
    AutomationJson target, {
    required String operationId,
    bool install = false,
  }) async => {
    ...await super.setupRunner(target, operationId: operationId, install: install),
    'prerequisite_command': command,
  };
  @override
  Future<String> openPrerequisiteTerminal(
    AutomationJson target, {
    required String operationId,
    required String command,
  }) async {
    if (prerequisiteFailure != null) throw AutomationRunnerPrerequisiteFailed(prerequisiteFailure!);
    opened = {'target': target, 'operation_id': operationId, 'command': command};
    return pendingTerminal?.future ?? 'owned-runtime';
  }
}

class _Sources extends _Runners implements AutomationGitService {
  bool started = false;
  String? cancelled;
  String? importRunnerId;
  String? importUrl;
  String? importCa;
  int importRunnerConnections = 0;
  Object? gitFailure;
  @override
  Future<AutomationJson> connectRunner(String id) async {
    importRunnerConnections++;
    return super.connectRunner(id);
  }

  Completer<AutomationProject>? pendingStart;
  static const imported = AutomationProject(
    id: 'imported',
    name: 'Imported source',
    readOnly: true,
    revision: 1,
    importState: 'importing',
  );
  @override
  Future<AutomationProjectCatalog> projects() async => AutomationProjectCatalog(
    version: 1,
    readOnly: false,
    deviceLocal: true,
    projects: [
      _Projects.project,
      if (started)
        ready
            ? const AutomationProject(
                id: 'imported',
                name: 'Imported source',
                readOnly: false,
                revision: 2,
                importState: 'ready',
                gitSource: {'reference': 'main', 'commit': 'fixture-commit', 'snapshot_digest': 'fixture'},
              )
            : imported,
    ],
  );
  @override
  Future<AutomationProject> startGit({
    required String name,
    required String runnerId,
    required String url,
    required String reference,
    String? caPem,
  }) async {
    importRunnerId = runnerId;
    importUrl = url;
    importCa = caPem;
    if (gitFailure != null) throw gitFailure!;
    started = true;
    return pendingStart == null ? imported : pendingStart!.future;
  }

  @override
  Future<void> cancelGit(String id) async {
    cancelled = id;
  }
}

class _Inventory extends _Runners implements AutomationSourceInventoryService {
  Host member = Host.create(name: 'Saved inventory target', address: '192.0.2.10');
  Host other = Host.create(name: 'Other saved target', address: '192.0.2.11');
  AutomationJson? configuration, saved;
  List<AutomationJson>? chosenSources;
  final discarded = <String>[];
  Completer<AutomationJson>? pendingStart, pendingConfiguration, pendingPage, pendingCheck;
  int checks = 0, configurations = 0;
  String? delayedQuery;
  int count = 1, clears = 0;
  bool directoryFixture = false;
  AutomationJson? report;
  @override
  Future<AutomationProjectRevision> files(String projectId) async => AutomationProjectRevision(
    digest: 'fixture',
    bytes: 20,
    entries: [
      const AutomationFileEntry(path: 'playbook.yml', directory: false),
      const AutomationFileEntry(path: 'hosts.ini', directory: false),
      if (directoryFixture) const AutomationFileEntry(path: 'inventory', directory: true),
      if (directoryFixture) const AutomationFileEntry(path: 'inventory/hosts.ini', directory: false),
    ],
  );
  @override
  Future<AutomationJson> inventoryConfiguration(String projectId) async {
    configurations++;
    return pendingConfiguration?.future ??
        {'version': 1, 'project_id': 'p', 'project_revision': 1, 'configuration': configuration};
  }

  @override
  Future<AutomationJson> startInventory(String projectId, String runnerId, List<AutomationJson> sources) async {
    chosenSources = sources;
    return pendingStart?.future ??
        {'version': 1, 'preview_id': 'preview1', 'project_id': 'p', 'project_revision': 1, 'state': 'preparing'};
  }

  AutomationJson row(int index) => {
    'alias': 'service_${index.toString().padLeft(5, '0')}',
    'address': '192.0.2.10',
    'port': 2222,
    'username': 'deployer',
    'sources': ['hosts.ini'],
  };
  AutomationJson result({String query = '', int offset = 0}) {
    final found = [
      for (var i = 0; i < count; i++)
        if (row(i)['alias'].toString().contains(query) || query.isEmpty) row(i),
    ];
    final rows = found.skip(offset).take(50).toList();
    return {
      'version': 1,
      'preview_id': 'preview1',
      'project_id': 'p',
      'project_revision': 1,
      'state': 'ready',
      'hosts': rows,
      'cursor': offset + rows.length,
      'total': found.length,
      if (directoryFixture)
        'resolved_sources': [
          {'path': 'inventory/hosts.ini', 'format': 'ini'},
          {'path': 'inventory/group_vars/web.yml', 'format': 'group_vars'},
        ],
    };
  }

  @override
  Future<AutomationJson> inventoryPage(String id, {String query = '', int offset = 0}) async =>
      pendingPage != null && query == delayedQuery ? pendingPage!.future : result(query: query, offset: offset);
  @override
  Future<void> discardInventory(String id) async {
    discarded.add(id);
  }

  @override
  Future<AutomationJson> checkInventory(String id, AutomationJson configuration) async {
    checks++;
    return pendingCheck?.future ??
        report ??
        {
          'version': 1,
          'preview_id': id,
          'project_id': 'p',
          'valid': true,
          'conflict_count': 0,
          'conflicts': <AutomationJson>[],
        };
  }

  @override
  Future<AutomationProject> commitInventory(String id, AutomationJson configuration) async {
    saved = configuration;
    return const AutomationProject(id: 'p', name: 'Fixture project', readOnly: false, revision: 2);
  }

  @override
  Future<AutomationProject> clearInventory(String projectId, int revision) async {
    expect(revision, 1);
    clears++;
    return const AutomationProject(id: 'p', name: 'Fixture project', readOnly: false, revision: 2);
  }
}

class _SelectedInventory extends _Inventory implements AutomationInventorySelectionService {
  final folder = Group.create('Web folder');
  late final child = Group.create('Child folder', parentId: folder.id);
  AutomationJson? selection;
  _SelectedInventory() {
    member = member.withGroup(folder.id);
    other = other.withGroup(child.id);
  }
  @override
  Future<AutomationJson> inventoryConfiguration(String projectId) async => {
    ...await super.inventoryConfiguration(projectId),
    'saved_inventory': {'version': 2, 'groups': <String, dynamic>{}, 'selected_hosts': <String>[]},
  };
  @override
  Future<AutomationProject> selectInventory(AutomationJson selection) async {
    this.selection = selection;
    return const AutomationProject(id: 'p', name: 'Fixture project', readOnly: false, revision: 2);
  }
}

class _StaticInventory extends _Inventory {
  _StaticInventory(this.format);

  final String format;

  String get inventoryPath => 'inventory/hosts.$format';

  @override
  Future<AutomationProjectRevision> files(String projectId) async => AutomationProjectRevision(
    digest: 'inventory-fixture',
    bytes: 92,
    entries: [
      const AutomationFileEntry(path: 'playbook.yml', directory: false),
      AutomationFileEntry(path: inventoryPath, directory: false),
      const AutomationFileEntry(path: 'ansible.cfg', directory: false),
    ],
  );

  @override
  Future<AutomationProjectFile> readFile(String projectId, String path) async {
    if (path == 'ansible.cfg') {
      return AutomationProjectFile(
        path: path,
        text: '[defaults]\ninventory = ./$inventoryPath\n',
        sha256: 'config',
        lineEndings: 'lf',
      );
    }
    if (path == inventoryPath) {
      return AutomationProjectFile(
        path: path,
        text: format == 'ini' ? '[web]\nweb_example\n' : 'web:\n  hosts:\n    web_example:\n',
        sha256: 'inventory',
        lineEndings: 'lf',
      );
    }
    return super.readFile(projectId, path);
  }
}

AppServices _sameInventoryServices(AppServices services) => AppServices(
  profiles: services.profiles,
  auth: services.auth,
  vault: services.vault,
  inventory: services.inventory,
  terminal: services.terminal,
  sftp: services.sftp,
  tunnels: services.tunnels,
  snippets: services.snippets,
  ai: services.ai,
  devices: services.devices,
  sync: services.sync,
  settings: services.settings,
  backups: services.backups,
  files: services.files,
  sftpBrowser: services.sftpBrowser,
  prompts: services.prompts,
  sharing: services.sharing,
  teams: services.teams,
  enrollment: services.enrollment,
  kubernetes: services.kubernetes,
  databases: services.databases,
  docker: services.docker,
  automation: services.automation,
  jobNotices: services.jobNotices,
  runtimeNotices: services.runtimeNotices,
  notes: services.notes,
  developer: services.developer,
  onDispose: services.onDispose,
  approvedExitActions: services.approvedExitActions,
);

Future<void> inventoryInputs(WidgetTester tester) async {
  await toolbarAction(tester, 'automation-inventory-open');
  await tester.tap(find.byKey(const ValueKey('automation-inventory-runner')));
  await settle(tester);
  await tester.tap(find.text('Remote fixture').last);
  await settle(tester);
  await tapKey(tester, 'automation-inventory-file-hosts.ini');
}

Future<void> toolbarAction(WidgetTester tester, String key) async {
  final projectAction = const {'automation-new-project', 'automation-open-folder', 'automation-git-open'}.contains(key);
  await tapKey(tester, projectAction ? 'automation-project-actions' : 'automation-settings-menu');
  await tapKey(tester, key);
}

class _MenuProjects extends _Sources
    implements
        AutomationInventoryService,
        AutomationSourceInventoryService,
        AutomationScaffoldService,
        AutomationJobService,
        AutomationInventoryExportService {
  bool futureCatalog = false;
  @override
  Future<AutomationProjectCatalog> projects() async =>
      AutomationProjectCatalog(version: 1, readOnly: futureCatalog, deviceLocal: true, projects: [_Projects.project]);
  @override
  Future<AutomationJson> jobs() async => {'version': 1, 'jobs': <AutomationJson>[]};
  // The menu never executes a capability merely by opening it.
  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

Future<ProviderContainer> _open(
  WidgetTester tester,
  _Projects service, {
  Size size = const Size(1400, 900),
  AppLocale locale = AppLocale.en,
}) async {
  await loadAutomationProofFonts(tester);
  final backend = testBackend();
  addTearDown(backend.dispose);
  await backend.debugCreateUnlockedLocalProfile(name: 'Automation test');
  if (io.Platform.environment['CONSOLECRYPT_UI_PROOF_DIR'] != null) {
    final settings = backend.services.settings;
    await settings.updateLocal(settings.currentLocal.copyWith(themeMode: AppThemeMode.dark));
  }
  await pumpApp(
    tester,
    backend,
    size: size,
    locale: locale,
    captureBoundary: io.Platform.environment['CONSOLECRYPT_UI_PROOF_DIR'] != null,
    overrides: [
      automationServiceProvider.overrideWithValue(service),
      if (service is _PrerequisiteSetup) terminalServiceProvider.overrideWithValue(service.terminal),
      if (service is _WslSetup) terminalServiceProvider.overrideWithValue(service.terminal),
      if (service is _Editor) fileDialogServiceProvider.overrideWithValue(service.picker),
      if (service is _FolderProjects) fileDialogServiceProvider.overrideWithValue(service.picker),
      if (service is _Inventory) hostsProvider.overrideWith((ref) => Stream.value([service.member, service.other])),
      if (service is _SetupRunners) hostsProvider.overrideWith((ref) => Stream.value([service.remoteHost])),
      if (service is _SelectedInventory)
        groupsProvider.overrideWith((ref) => Stream.value([service.folder, service.child])),
      if (service is _Ai) aiProvidersProvider.overrideWith((ref) => Stream.value([service.provider])),
    ],
  );
  final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
  container.read(routerProvider).go(AppRoutes.automation);
  await settle(tester);
  await tapKey(tester, 'automation-project-p');
  return container;
}

void main() {
  for (final scenario in [
    (size: const Size(1500, 1050), scale: 1.0),
    (size: const Size(460, 950), scale: 1.0),
    (size: const Size(560, 1100), scale: 1.7),
  ]) {
    testWidgets(
      'logical toolbar and compact menu rows stay readable at ${scenario.size.width} scale ${scenario.scale}',
      (tester) async {
        tester.platformDispatcher.textScaleFactorTestValue = scenario.scale;
        addTearDown(tester.platformDispatcher.clearTextScaleFactorTestValue);
        await _open(tester, _MenuProjects(), size: scenario.size, locale: AppLocale.ru);
        final toolbar = find.byKey(const ValueKey('automation-project-toolbar'));
        expect(find.descendant(of: toolbar, matching: find.byType(GlassButton)), findsNWidgets(3));
        expect(
          tester.widget<GlassButton>(find.byKey(const ValueKey('automation-project-actions'))).label,
          '\u041f\u0440\u043e\u0435\u043a\u0442',
        );
        expect(
          tester.widget<GlassButton>(find.byKey(const ValueKey('automation-open-run'))).style,
          GlassButtonStyle.prominent,
        );
        for (final key in ['automation-project-actions', 'automation-settings-menu', 'automation-open-run']) {
          final trigger = find.byKey(ValueKey(key));
          final context = tester.element(trigger);
          final style = GlassTokens.of(context).typography.button;
          final line = MediaQuery.textScalerOf(context).scale(style.fontSize!) * (style.height ?? 1);
          expect(tester.getSize(trigger).height, greaterThanOrEqualTo(line));
        }
        for (final key in [
          'automation-new-project',
          'automation-git-open',
          'automation-runners-open',
          'automation-inventory-open',
        ]) {
          expect(find.byKey(ValueKey(key)), findsNothing);
        }
        await captureAutomationWorkspace(tester, 'toolbar-${scenario.size.width}-${scenario.scale}-ru');
        for (final entry in [
          (
            trigger: 'automation-project-actions',
            rows: ['automation-new-project', 'automation-open-folder', 'automation-git-open'],
          ),
          (trigger: 'automation-settings-menu', rows: ['automation-runners-open', 'automation-inventory-open']),
          (
            trigger: 'automation-create-menu',
            rows: ['automation-create-playbook', 'automation-create-role', 'automation-create-file'],
          ),
          (
            trigger: 'automation-project-menu',
            rows: [
              'automation-rename-project',
              'automation-ansible-settings',
              'automation-reload-project',
              'automation-inventory-export-open',
            ],
          ),
        ]) {
          await tapKey(tester, entry.trigger);
          Rect? previous;
          for (final key in entry.rows) {
            final finder = find.byKey(ValueKey(key));
            expect(finder.hitTestable(), findsOneWidget);
            final row = tester.getRect(finder);
            expect(row.left, greaterThanOrEqualTo(0));
            expect(row.right, lessThanOrEqualTo(scenario.size.width));
            if (scenario.scale == 1) expect(row.height, lessThanOrEqualTo(36));
            if (previous != null) expect(previous.overlaps(row), isFalse);
            previous = row;
          }
          await captureAutomationWorkspace(tester, '${entry.trigger}-${scenario.size.width}-${scenario.scale}-ru');
          await tester.sendKeyEvent(LogicalKeyboardKey.escape);
          await settle(tester);
          expect(find.byKey(ValueKey(entry.rows.first)), findsNothing);
          expect(tester.takeException(), isNull);
        }
        await tester.pumpWidget(const SizedBox());
      },
      variant: const TargetPlatformVariant({TargetPlatform.windows}),
    );
  }

  testWidgets('logical project menu supports keyboard opening selection and Escape without losing the draft', (
    tester,
  ) async {
    final service = _MenuProjects();
    await _open(tester, service);
    await tapKey(tester, 'automation-file-playbook.yml');
    const draft = '---\n# unsaved keyboard menu fixture\n';
    await enterKey(tester, 'automation-editor', draft);
    final button = find.byKey(const ValueKey('automation-project-actions'));
    final label = find.descendant(of: button, matching: find.byType(Text)).first;
    Focus.of(tester.element(label)).requestFocus();
    await tester.pump();
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    await settle(tester);
    expect(find.byKey(const ValueKey('automation-new-project')).hitTestable(), findsOneWidget);
    await tester.sendKeyEvent(LogicalKeyboardKey.arrowDown);
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    await settle(tester);
    expect(find.text('Save changes to this file?'), findsOneWidget);
    await tester.tap(find.text('Cancel'));
    await settle(tester);
    expect(service.savedExpected, isNull);
    expect(tester.widget<TextField>(find.byKey(const ValueKey('automation-editor'))).controller!.text, draft);
    await tapKey(tester, 'automation-project-actions');
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await settle(tester);
    expect(find.byKey(const ValueKey('automation-new-project')), findsNothing);
    expect(tester.widget<TextField>(find.byKey(const ValueKey('automation-editor'))).controller!.text, draft);
    await tester.pumpWidget(const SizedBox());
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));

  testWidgets('logical menus preserve future catalog and busy action guards', (tester) async {
    final service = _MenuProjects()..futureCatalog = true;
    final container = await _open(tester, service);
    expect(isEnabled(tester, 'automation-project-actions'), isFalse);
    service.futureCatalog = false;
    container.invalidate(automationProjectsProvider);
    await settle(tester);
    expect(isEnabled(tester, 'automation-project-actions'), isTrue);
    service.pendingRead = Completer<AutomationProjectFile>();
    await tester.tap(find.byKey(const ValueKey('automation-file-playbook.yml')));
    await settle(tester);
    expect(isEnabled(tester, 'automation-project-actions'), isFalse);
    expect(isEnabled(tester, 'automation-open-run'), isFalse);
    await tapKey(tester, 'automation-settings-menu');
    expect(tester.widget<PopupMenuItem<String>>(find.byKey(const ValueKey('automation-runners-open'))).enabled, isTrue);
    expect(find.byKey(const ValueKey('automation-groups-open')), findsNothing);
    expect(
      tester.widget<PopupMenuItem<String>>(find.byKey(const ValueKey('automation-inventory-open'))).enabled,
      isFalse,
    );
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await settle(tester);
    service.pendingRead!.complete(
      const AutomationProjectFile(path: 'playbook.yml', text: '---\n', sha256: 'read', lineEndings: 'lf'),
    );
    await settle(tester);
    expect(isEnabled(tester, 'automation-project-actions'), isTrue);
    await tester.pumpWidget(const SizedBox());
  });

  Future<void> editorShortcut(WidgetTester tester, LogicalKeyboardKey key) async {
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendKeyEvent(key);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    await tester.pump();
  }

  testWidgets('workspace source copy uses protected expiring clipboard and hidden selections stay masked', (
    tester,
  ) async {
    final copied = <Map<dynamic, dynamic>>[];
    var cleared = 0;
    tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(SecureClipboard.channel, (call) async {
      if (call.method == 'copySecret') {
        copied.add(call.arguments as Map);
        return 'workspace-clip-${copied.length}';
      }
      if (call.method == 'clearIfOwned') {
        cleared++;
        return 'cleared';
      }
      return null;
    });
    addTearDown(() => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(SecureClipboard.channel, null));
    final service = _Projects()..text = '[all]\nserver ansible_password=fixture-source-secret\n';
    await _open(tester, service);
    await tapKey(tester, 'automation-file-playbook.yml');
    await tester.tap(find.byKey(const ValueKey('automation-editor')));
    await editorShortcut(tester, LogicalKeyboardKey.keyA);
    await editorShortcut(tester, LogicalKeyboardKey.keyC);
    expect(copied, hasLength(1));
    expect(copied.single['text'], isNot(contains('fixture-source-secret')));
    expect(copied.single['text'], contains('•'));
    expect(copied.single['clearAfterMilliseconds'], 30000);
    await tapKey(tester, 'automation-toggle-secrets');
    await tester.tap(find.byKey(const ValueKey('automation-editor')));
    await editorShortcut(tester, LogicalKeyboardKey.keyA);
    await editorShortcut(tester, LogicalKeyboardKey.keyC);
    expect(copied, hasLength(2));
    expect(copied.last['text'], service.text);
    await tester.pump(const Duration(seconds: 31));
    await tester.pump();
    expect(cleared, 1);
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('workspace failed protected Cut preserves canonical source', (tester) async {
    var attempts = 0;
    tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(SecureClipboard.channel, (call) async {
      if (call.method == 'copySecret') {
        attempts++;
        throw PlatformException(code: 'unavailable');
      }
      return null;
    });
    addTearDown(() => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(SecureClipboard.channel, null));
    final service = _Projects();
    await _open(tester, service);
    await tapKey(tester, 'automation-file-playbook.yml');
    await tester.tap(find.byKey(const ValueKey('automation-editor')));
    await editorShortcut(tester, LogicalKeyboardKey.keyA);
    await editorShortcut(tester, LogicalKeyboardKey.keyX);
    expect(attempts, 1);
    expect(tester.widget<TextField>(find.byKey(const ValueKey('automation-editor'))).controller!.text, service.text);
    expect(service.savedExpected, isNull);
    expect(isEnabled(tester, 'automation-save'), isFalse);
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('workspace lock clears a late owned source copy and never applies pending Cut', (tester) async {
    final pending = Completer<String>();
    var attempts = 0, cleared = 0;
    tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(SecureClipboard.channel, (call) async {
      if (call.method == 'copySecret') {
        attempts++;
        return pending.future;
      }
      if (call.method == 'clearIfOwned') {
        cleared++;
        return 'cleared';
      }
      return null;
    });
    addTearDown(() => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(SecureClipboard.channel, null));
    final service = _Projects();
    final container = await _open(tester, service);
    await tapKey(tester, 'automation-file-playbook.yml');
    await tester.tap(find.byKey(const ValueKey('automation-editor')));
    await editorShortcut(tester, LogicalKeyboardKey.keyA);
    await editorShortcut(tester, LogicalKeyboardKey.keyX);
    expect(attempts, 1);
    await container.read(vaultServiceProvider).lock();
    await settle(tester);
    pending.complete('revoked-workspace-clip');
    await settle(tester);
    expect(cleared, 1);
    expect(find.byKey(const ValueKey('automation-editor')), findsNothing);
    expect(service.savedExpected, isNull);
    expect(service.text, '---\n- hosts: all\n');
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('project header keeps controls inside a narrow sidebar and settings in its menu', (tester) async {
    await _open(tester, _Projects());
    await tester.drag(find.byKey(const ValueKey('automation-project-resize')), const Offset(-400, 0));
    await settle(tester);
    final tree = tester.getRect(find.byKey(const ValueKey('automation-project-tree')));
    expect(tree.width, 210);
    for (final key in ['automation-create-menu', 'automation-project-menu']) {
      final button = tester.getRect(find.byKey(ValueKey(key)));
      expect(button.left, greaterThanOrEqualTo(tree.left));
      expect(button.right, lessThanOrEqualTo(tree.right));
    }
    expect(find.byKey(const ValueKey('automation-ansible-settings')), findsNothing);
    await captureAutomationWorkspace(tester, 'project-header-compact');
    await tapKey(tester, 'automation-project-menu');
    expect(find.byKey(const ValueKey('automation-ansible-settings')).hitTestable(), findsOneWidget);
    await captureAutomationWorkspace(tester, 'project-settings-menu');
    expect(tester.takeException(), isNull);
  });

  testWidgets('Ansible settings patch only explicit changes with ordinary file CAS', (tester) async {
    final service = _ConfigProjects()..text = '# keep\r\n[defaults]\r\nhost_key_checking = False\r\n';
    await _open(tester, service);
    await tapKey(tester, 'automation-project-menu');
    await tapKey(tester, 'automation-ansible-settings');
    await captureAutomationDialog(tester, 'ansible-settings');
    await tester.enterText(find.byKey(const ValueKey('task_timeout')), '120');
    await tester.pump();
    await tester.tap(find.text('Apply changes'));
    await settle(tester);
    expect(service.savedPath, 'ansible.cfg');
    expect(service.savedExpected, 'initial-hash');
    expect(service.text, contains('# keep\r\n'));
    expect(service.text, contains('host_key_checking = False\r\n'));
    expect(service.text, contains('task_timeout = 120'));
    expect(tester.takeException(), isNull);
  });

  testWidgets('Ansible config conflict keeps edited buffer and reports conflict', (tester) async {
    final service = _ConfigProjects()
      ..text = '[defaults]\n'
      ..conflict = true;
    await _open(tester, service);
    await tapKey(tester, 'automation-project-menu');
    await tapKey(tester, 'automation-ansible-settings');
    await tester.enterText(find.byKey(const ValueKey('task_timeout')), '90');
    await tester.pump();
    await tester.tap(find.text('Apply changes'));
    await settle(tester);
    expect(service.text, '[defaults]\n');
    expect(
      tester.widget<TextField>(find.byKey(const ValueKey('automation-editor'))).controller!.text,
      contains('task_timeout = 90'),
    );
    expect(isEnabled(tester, 'automation-save'), isTrue);
    expect(tester.takeException(), isNull);
  });

  testWidgets('inventory passwords start masked, reveal explicitly and save original bytes after hiding', (
    tester,
  ) async {
    final service = _Projects()..text = '[all]\nserver ansible_user=root ansible_ssh_pass=fixture-password\n';
    final initial = service.text;
    final container = await _open(tester, service);
    await tapKey(tester, 'automation-file-playbook.yml');
    TextField field() => tester.widget<TextField>(find.byKey(const ValueKey('automation-editor')));
    expect(field().readOnly, isTrue);
    expect(field().controller!.text, isNot(contains('fixture-password')));
    expect(service.text, initial);
    await captureAutomationWorkspace(tester, 'inventory-password-masked');
    final semantics = tester.ensureSemantics();
    expect(
      tester
          .getSemantics(
            find.descendant(of: find.byKey(const ValueKey('automation-editor')), matching: find.byType(EditableText)),
          )
          .toStringDeep(),
      isNot(contains('fixture-password')),
    );
    await tapKey(tester, 'automation-toggle-secrets');
    expect(field().readOnly, isFalse);
    expect(field().controller!.text, initial);
    expect(SecretExposure.instance.visible, isTrue);
    await enterKey(tester, 'automation-editor', '$initial# edited\n');
    await tapKey(tester, 'automation-toggle-secrets');
    expect(field().controller!.text, isNot(contains('fixture-password')));
    expect(SecretExposure.instance.visible, isFalse);
    await tapKey(tester, 'automation-save');
    expect(service.text, '$initial# edited\n');
    expect(service.savedExpected, 'initial-hash');
    await container.read(vaultServiceProvider).lock();
    await settle(tester);
    expect(find.byKey(const ValueKey('automation-editor')), findsNothing);
    expect(SecretExposure.instance.visible, isFalse);
    semantics.dispose();
    expect(tester.takeException(), isNull);
  });

  testWidgets('Ansible editor folds only the view and saving retains hidden tasks', (tester) async {
    const original = '''---
- name: Fixture play
  hosts: all
  tasks:
    - name: First task
      ansible.builtin.debug:
        msg: "{{ message | default('hello') }}"
    - name: Second task
      ansible.builtin.debug:
        msg: done
''';
    final service = _Projects()..text = original;
    await _open(tester, service);
    await tapKey(tester, 'automation-file-playbook.yml');
    await captureAutomationWorkspace(tester, 'ansible-editor-numbered');
    await tapKey(tester, 'ansible-fold-5');
    expect(service.text, original);
    expect(isEnabled(tester, 'automation-save'), isFalse);
    final display = tester.widget<TextField>(find.byKey(const ValueKey('automation-editor'))).controller!;
    expect(display.text, isNot(contains("default('hello')")));
    await captureAutomationWorkspace(tester, 'ansible-editor-folded');
    display.value = TextEditingValue(
      text: '${display.text}\n# appended note\n',
      selection: TextSelection.collapsed(offset: display.text.length + '\n# appended note\n'.length),
    );
    await tester.pump();
    await tapKey(tester, 'automation-save');
    expect(service.text, '$original\n# appended note\n');
    expect(tester.takeException(), isNull);
  });

  testWidgets('Open folder chooses the directory first and derives a renameable project name', (tester) async {
    final service = _FolderProjects();
    await _open(tester, service);
    await toolbarAction(tester, 'automation-open-folder');
    await settle(tester);
    expect(service.picker.calls, 1);
    expect(service.created, {'name': 'My Ansible Project', 'directory': service.picker.choice});
    expect(find.byType(AlertDialog), findsNothing); // No name dialog after the trusted folder picker.
    expect(tester.takeException(), isNull);
  });
  testWidgets('Cancelled folder picker creates no project and asks for no name', (tester) async {
    final service = _FolderProjects()..picker.choice = null;
    await _open(tester, service);
    await toolbarAction(tester, 'automation-open-folder');
    await settle(tester);
    expect(service.created, isNull);
    expect(find.byType(AlertDialog), findsNothing);
    expect(tester.takeException(), isNull);
  });
  testWidgets('Editing a Runner preloads paths and disconnects before revision-checked save', (tester) async {
    final service = _EditingRunners();
    await _open(tester, service);
    await toolbarAction(tester, 'automation-runners-open');
    await settle(tester);
    await tapKey(tester, 'automation-edit-runner-runner');
    expect(find.widgetWithText(TextField, 'Existing Runner'), findsOneWidget);
    final path = find.byWidgetPredicate(
      (widget) => widget is TextField && widget.controller?.text == '/home/fixture/private',
    );
    await tester.enterText(path, '/home/fixture/new private');
    await tester.tap(find.text('Save').last);
    await settle(tester);
    expect(service.actions, ['disconnect', 'update']);
    expect(service.updated!['revision'], 3);
    expect(
      ((service.updated!['configuration'] as AutomationJson)['launch'] as AutomationJson)['root'],
      '/home/fixture/new private',
    );
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.linux}));
  testWidgets('Create offers a named playbook draft and a distinct role scaffold action', (tester) async {
    final service = _Scaffolds();
    await _open(tester, service);
    await tapKey(tester, 'automation-create-menu');
    expect(find.text('Create role'), findsOneWidget);
    await tester.tap(find.text('New playbook'));
    await settle(tester);
    expect(tester.widget<TextField>(find.byType(TextField).last).controller!.text, 'playbook_2.yml');
    await tester.tap(find.text('Save').last);
    await settle(tester);
    final text = tester.widget<TextField>(find.byKey(const ValueKey('automation-editor'))).controller!.text;
    expect(text, contains('hosts: all'));
    expect(text, contains('ansible.builtin.debug'));
    expect(service.savedExpected, isNull);
    expect(service.createdRole, isNull);
    expect(tester.takeException(), isNull);
  });
  testWidgets('role creation calls the scaffold service and opens its tasks file', (tester) async {
    final service = _Scaffolds();
    await _open(tester, service);
    await tapKey(tester, 'automation-create-menu');
    await tester.tap(find.text('Create role'));
    await settle(tester);
    await tester.enterText(find.byType(TextField).last, 'web_service');
    await tester.tap(find.text('Save').last);
    await settle(tester);
    expect(service.createdRole, 'web_service');
    expect(tester.widget<TextField>(find.byKey(const ValueKey('automation-editor'))).controller!.text, '---\n[]\n');
    expect(tester.takeException(), isNull);
  });
  testWidgets('inventory can save ConsoleCrypt folders without a Runner or file', (tester) async {
    final service = _SelectedInventory();
    await _open(tester, service);
    await toolbarAction(tester, 'automation-inventory-open');
    expect(find.byKey(const ValueKey('automation-inventory-runner')), findsNothing);
    await tester.tap(find.byKey(const ValueKey('automation-inventory-folder')));
    await settle(tester);
    await tester.tap(find.text('Web folder').last);
    await settle(tester);
    expect(service.selection, isNull);
    await tapKey(tester, 'automation-inventory-save-saved');
    final ids = [service.member.id.value, service.other.id.value]..sort();
    expect(service.selection, {
      'version': 1,
      'project_id': 'p',
      'expected_revision': 1,
      'saved_hosts': ids,
      'configuration': null,
      'preview_id': null,
    });
    expect(service.chosenSources, isNull);
    expect(tester.takeException(), isNull);
  });
  for (final format in ['ini', 'yml']) {
    testWidgets('${format.toUpperCase()} inventory selected by ansible.cfg previews without connecting a Runner', (
      tester,
    ) async {
      final service = _StaticInventory(format);
      await _open(tester, service);
      await toolbarAction(tester, 'automation-inventory-open');
      final file = find.byKey(ValueKey('automation-inventory-file-${service.inventoryPath}'));
      expect(tester.widget<CheckboxListTile>(file).value, isTrue);
      final preview = find.byKey(const ValueKey('automation-inventory-start'));
      expect(tester.widget<FilledButton>(preview).onPressed, isNotNull);
      await tester.runAsync(() async {
        await tester.tap(preview);
        // The preview parses files in an isolate.
        for (var attempt = 0; attempt < 20; attempt++) {
          await Future<void>.delayed(const Duration(milliseconds: 40));
          await tester.pump();
          if (find.text('web_example').evaluate().isNotEmpty) break;
        }
      });
      expect(find.text('web_example'), findsOneWidget);
      expect(service.chosenSources, isNull);
      expect(tester.takeException(), isNull);
    });
  }

  testWidgets('mixed inventory saves the explicit saved host selection and external preview together', (tester) async {
    final service = _SelectedInventory();
    await _open(tester, service);
    await toolbarAction(tester, 'automation-inventory-open');
    await tester.tap(find.byKey(ValueKey('automation-inventory-saved-${service.other.id.value}')));
    await tester.tap(find.byKey(const ValueKey('automation-inventory-source-kind')));
    await settle(tester);
    await tester.tap(find.text('Hybrid').last);
    await settle(tester);
    await captureAutomationDialog(tester, 'inventory-mixed');
    final hostPanel = tester.getRect(find.byKey(const ValueKey('automation-inventory-host-panel')));
    final filePanel = tester.getRect(find.byKey(const ValueKey('automation-inventory-file-panel')));
    expect(filePanel.left - hostPanel.right, greaterThanOrEqualTo(16));
    expect(filePanel.top, hostPanel.top);
    expect(find.byKey(const ValueKey('automation-inventory-file-hosts.ini')).hitTestable(), findsOneWidget);
    await tester.scrollUntilVisible(
      find.byKey(const ValueKey('automation-inventory-runner')),
      150,
      scrollable: find
          .descendant(
            of: find.byKey(const ValueKey('automation-inventory-source-scroll')),
            matching: find.byType(Scrollable),
          )
          .first,
    );
    await tester.tap(find.byKey(const ValueKey('automation-inventory-runner')));
    await settle(tester);
    await tester.tap(find.text('Remote fixture').last);
    await settle(tester);
    await tapKey(tester, 'automation-inventory-file-hosts.ini');
    await tapKey(tester, 'automation-inventory-start');
    await tapKey(tester, 'automation-inventory-save');
    expect(service.selection!['saved_hosts'], [service.other.id.value]);
    expect((service.selection!['configuration'] as AutomationJson)['mode'], 'mixed');
    expect(service.selection!['preview_id'], 'preview1');
    expect(service.saved, isNull);
    expect(tester.takeException(), isNull);
  });
  testWidgets('mixed inventory stacks bounded independent lists on narrow desktop', (tester) async {
    final service = _SelectedInventory();
    await _open(tester, service, size: const Size(680, 760));
    await toolbarAction(tester, 'automation-inventory-open');
    await tester.tap(find.byKey(const ValueKey('automation-inventory-source-kind')));
    await settle(tester);
    await tester.tap(find.text('Hybrid').last);
    await settle(tester);
    final hostPanel = tester.getRect(find.byKey(const ValueKey('automation-inventory-host-panel')));
    final filePanel = tester.getRect(find.byKey(const ValueKey('automation-inventory-file-panel')));
    expect(filePanel.top - hostPanel.bottom, greaterThanOrEqualTo(16));
    expect(tester.getSize(find.byKey(const ValueKey('automation-inventory-saved-hosts'))).height, 200);
    await tester.scrollUntilVisible(
      find.byKey(const ValueKey('automation-inventory-file-hosts.ini')),
      180,
      scrollable: find
          .descendant(
            of: find.byKey(const ValueKey('automation-inventory-source-scroll')),
            matching: find.byType(Scrollable),
          )
          .first,
    );
    await tapKey(tester, 'automation-inventory-file-hosts.ini');
    expect(
      tester.widget<CheckboxListTile>(find.byKey(const ValueKey('automation-inventory-file-hosts.ini'))).value,
      isTrue,
    );
    await captureAutomationDialog(tester, 'inventory-mixed-narrow');
    expect(tester.takeException(), isNull);
  });
  Future<ProviderContainer> openSetup(
    WidgetTester tester,
    _SetupRunners service, {
    Size size = const Size(1400, 900),
  }) async {
    final container = await _open(tester, service, size: size);
    await toolbarAction(tester, 'automation-runners-open');
    await settle(tester);
    await tester.ensureVisible(find.text('Add environment'));
    await tester.tap(find.text('Add environment'));
    await settle(tester);
    return container;
  }

  testWidgets('Runner scan is read-only; install needs a separate reviewed click', (tester) async {
    final service = _SetupRunners();
    await openSetup(tester, service);
    expect(service.calls, isEmpty);
    await tapKey(tester, 'automation-detect-runner');
    expect(service.calls, [false]);
    await tapKey(tester, 'automation-install-runner');
    expect(service.calls, [false]);
    expect(find.textContaining('verifies the SHA-256 hashes'), findsOneWidget);
    await tester.tap(find.text('Cancel').last);
    await settle(tester);
    expect(service.calls, [false]);
    expect(service.created, isNull);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.linux}));
  for (final entry in {
    'ssh_account_changed': 'The environment or remote account changed.',
    'ssh_route_unsupported': 'A prerequisites terminal is unavailable for this route.',
    'invalid_prerequisite_packages': 'The prerequisites command could not be verified.',
  }.entries) {
    testWidgets('prerequisites ${entry.key} shows safe actionable help without opening a terminal', (tester) async {
      final service = _PrerequisiteSetup()
        ..missing = true
        ..prerequisiteFailure = entry.key;
      final container = await openSetup(tester, service);
      await tapKey(tester, 'automation-detect-runner');
      await tapKey(tester, 'automation-runner-execute-prerequisites');
      await tester.ensureVisible(find.textContaining(entry.value));
      await settle(tester);
      expect(find.textContaining(entry.value), findsOneWidget);
      expect(find.textContaining(entry.key), findsNothing);
      expect(container.read(terminalTabsProvider).tabs, isEmpty);
      expect(service.opened, isNull);
      expect(tester.takeException(), isNull);
    }, variant: const TargetPlatformVariant({TargetPlatform.linux}));
  }
  testWidgets('prerequisite button opens its approved runtime in the terminal section', (tester) async {
    final service = _PrerequisiteSetup()..missing = true;
    final container = await openSetup(tester, service);
    await tapKey(tester, 'automation-detect-runner');
    expect(service.opened, isNull);
    await tapKey(tester, 'automation-runner-execute-prerequisites');
    expect(service.opened!['command'], _PrerequisiteSetup.command);
    expect(service.opened!['target'], {'kind': 'native'});
    expect(container.read(terminalTabsProvider).tabs.single.displayTitle, 'Runner setup fixture');
    expect(find.text('Runner environments'), findsNothing);
    expect(service.created, isNull);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.linux}));
  testWidgets('Runner setup and install review fit a small desktop window', (tester) async {
    final service = _SetupRunners();
    await openSetup(tester, service, size: const Size(500, 700));
    await tapKey(tester, 'automation-detect-runner');
    await tapKey(tester, 'automation-install-runner');
    expect(find.textContaining('verifies the SHA-256 hashes'), findsOneWidget);
    expect(tester.takeException(), isNull);
    final bounds = tester.getRect(find.text('Cancel').last);
    expect(bounds.bottom, lessThanOrEqualTo(700));
    await tester.tap(find.text('Cancel').last);
    await settle(tester);
    expect(service.calls, [false]);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.linux}));
  testWidgets('WSL discovery shows pending and safe failure with explicit retry and no setup', (tester) async {
    final service = _WslSetup()..terminal.pending = Completer<LocalShellProfiles>();
    await openSetup(tester, service);
    expect(find.byKey(const ValueKey('automation-wsl-loading')), findsOneWidget);
    expect(service.terminal.lists, 1);
    expect(service.calls, isEmpty);
    service.terminal.pending!.completeError(StateError('PRIVATE_WSL_DISCOVERY_FAILURE'));
    await settle(tester);
    expect(find.textContaining('Could not read WSL distributions'), findsOneWidget);
    expect(find.textContaining('PRIVATE_WSL_DISCOVERY_FAILURE'), findsNothing);
    service.terminal.pending = null;
    await tapKey(tester, 'automation-wsl-refresh');
    expect(service.terminal.lists, 2);
    expect(service.calls, isEmpty);
    final select = tester.widget<GlassSelect<String?>>(find.byKey(const ValueKey('automation-wsl-distribution')));
    expect(select.value, isNull);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  testWidgets('WSL legacy empty and contradictory available status remain unknown without setup', (tester) async {
    final service = _WslSetup()..terminal.value = const LocalShellProfiles();
    await openSetup(tester, service, size: const Size(500, 760));
    expect(find.text('WSL distribution discovery status is unknown. Refresh to check again.'), findsOneWidget);
    expect(find.text('No registered WSL distributions were found.'), findsNothing);
    service.terminal.value = const LocalShellProfiles(wslDiscovery: WslDiscoveryStatus.available);
    await tapKey(tester, 'automation-wsl-refresh');
    expect(find.text('WSL distribution discovery status is unknown. Refresh to check again.'), findsOneWidget);
    expect(service.calls, isEmpty);
    expect(service.created, isNull);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  testWidgets('WSL executable missing is explicit discovery metadata with no feature repair or default selection', (
    tester,
  ) async {
    final service = _WslSetup()
      ..terminal.value = const LocalShellProfiles(wslDiscovery: WslDiscoveryStatus.executableMissing);
    await openSetup(tester, service, size: const Size(500, 760));
    expect(find.text('The WSL executable was not found at the system path.'), findsOneWidget);
    expect(find.text('No registered WSL distributions were found.'), findsNothing);
    expect(
      tester.widget<GlassSelect<String?>>(find.byKey(const ValueKey('automation-wsl-distribution'))).value,
      isNull,
    );
    expect(service.calls, isEmpty);
    expect(service.created, isNull);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  testWidgets('WSL typed discovery failures stay distinct and explicit refresh preserves no setup consent', (
    tester,
  ) async {
    final service = _WslSetup();
    final reasons = <WslDiscoveryStatus, String>{
      WslDiscoveryStatus.spawnFailed: 'The WSL executable could not be started.',
      WslDiscoveryStatus.timedOut: 'WSL distribution discovery timed out.',
      WslDiscoveryStatus.ioFailed: 'Could not read the WSL distribution list.',
      WslDiscoveryStatus.outputTooLarge: 'The WSL distribution list exceeded the safe size limit.',
      WslDiscoveryStatus.nonzeroExit: 'WSL distribution discovery exited unsuccessfully.',
    };
    service.terminal.value = LocalShellProfiles(wslDiscovery: reasons.keys.first);
    await openSetup(tester, service, size: const Size(500, 760));
    for (final entry in reasons.entries) {
      if (entry.key != reasons.keys.first) {
        service.terminal.value = LocalShellProfiles(wslDiscovery: entry.key);
        await tapKey(tester, 'automation-wsl-refresh');
      }
      expect(find.text(entry.value), findsOneWidget);
      expect(find.text('No registered WSL distributions were found.'), findsNothing);
      expect(service.calls, isEmpty);
      expect(service.created, isNull);
      expect(tester.takeException(), isNull);
    }
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  testWidgets('WSL empty discovery refresh never chooses a default and persists explicit distro user', (tester) async {
    final service = _WslSetup()..existing = true;
    final available = service.terminal.value;
    service.terminal.value = const LocalShellProfiles(wslDiscovery: WslDiscoveryStatus.empty);
    await openSetup(tester, service, size: const Size(500, 760));
    expect(find.text('No registered WSL distributions were found.'), findsOneWidget);
    expect(service.calls, isEmpty);
    service.terminal.value = available;
    await tapKey(tester, 'automation-wsl-refresh');
    final select = tester.widget<GlassSelect<String?>>(find.byKey(const ValueKey('automation-wsl-distribution')));
    expect(select.value, isNull);
    select.onChanged!('Beta');
    await settle(tester);
    await tapKey(tester, 'automation-detect-runner');
    expect(service.targets.single, {'kind': 'wsl', 'distribution': 'Beta', 'user': null});
    expect(service.created, isNull);
    await tester.ensureVisible(find.text('Save').last);
    await tester.tap(find.text('Save').last);
    await settle(tester);
    expect(service.created, {
      'kind': 'local',
      'launch': {'kind': 'wsl', ..._SetupRunners.paths, 'distribution': 'Beta', 'user': 'fixture'},
    });
    expect(service.calls, [false]);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  for (final known in [true, false]) {
    testWidgets('WSL OS version uses actual optional scan metadata, known=$known', (tester) async {
      final service = _WslSetup()..osKnown = known;
      await openSetup(tester, service);
      tester.widget<GlassSelect<String?>>(find.byKey(const ValueKey('automation-wsl-distribution'))).onChanged!(
        'Alpha',
      );
      await settle(tester);
      await tapKey(tester, 'automation-detect-runner');
      final version = tester.widget<Text>(find.byKey(const ValueKey('automation-runner-os-version'))).data!;
      expect(version.contains('24.04'), known);
      expect(version.contains('Actual captured distribution'), known);
      expect(version.contains('Alpha'), false);
      expect(version.contains('unknown future shape'), false);
      expect(find.textContaining('synthetic-kernel-wsl2'), findsOneWidget);
      expect(service.calls, [false]);
      expect(tester.takeException(), isNull);
    }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  }
  testWidgets('WSL distro edit clears diagnostics and only explicit Detect reads the new target', (tester) async {
    final service = _WslSetup();
    await openSetup(tester, service);
    tester.widget<GlassSelect<String?>>(find.byKey(const ValueKey('automation-wsl-distribution'))).onChanged!('Alpha');
    await settle(tester);
    await tapKey(tester, 'automation-detect-runner');
    tester.widget<GlassSelect<String?>>(find.byKey(const ValueKey('automation-wsl-distribution'))).onChanged!('Beta');
    await settle(tester);
    expect(find.byKey(const ValueKey('automation-runner-kernel')), findsNothing);
    expect(service.calls, [false]);
    await tapKey(tester, 'automation-detect-runner');
    expect(service.targets.last, {'kind': 'wsl', 'distribution': 'Beta', 'user': null});
    expect(service.calls, [false, false]);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  testWidgets('WSL explicit installation shows returned real probe without saving or extra requests', (tester) async {
    final service = _WslSetup();
    await openSetup(tester, service);
    tester.widget<GlassSelect<String?>>(find.byKey(const ValueKey('automation-wsl-distribution'))).onChanged!('Beta');
    await settle(tester);
    await tapKey(tester, 'automation-detect-runner');
    await tapKey(tester, 'automation-install-runner');
    expect(service.calls, [false]);
    await tester.tap(find.text('Install Runner').last);
    await settle(tester);
    expect(service.calls, [false, true]);
    expect(service.created, isNull);
    await tapKey(tester, 'runner-probe-details-fixture-install-env');
    expect(find.textContaining('fixture-helper-17'), findsOneWidget);
    expect(find.textContaining('fixture-core-version'), findsOneWidget);
    expect(find.text('OpenSSH'), findsOneWidget);
    await tester.scrollUntilVisible(
      find.byKey(const ValueKey('automation-runner-advanced')),
      220,
      scrollable: find.descendant(of: find.byType(AlertDialog).last, matching: find.byType(Scrollable)).first,
    );
    await tapKey(tester, 'automation-runner-advanced');
    final manual = find.byWidgetPredicate((w) => w is TextField && w.controller?.text == _SetupRunners.paths['python']);
    await tester.ensureVisible(manual);
    await tester.enterText(manual, '/home/fixture/other-python');
    await settle(tester);
    expect(find.textContaining('fixture-helper-17'), findsNothing);
    expect(service.calls, [false, true]);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  testWidgets('Runner installation fills paths; saving is a distinct action', (tester) async {
    final service = _SetupRunners();
    await openSetup(tester, service);
    await tapKey(tester, 'automation-detect-runner');
    await tapKey(tester, 'automation-install-runner');
    await tester.tap(find.text('Install Runner').last);
    await settle(tester);
    expect(service.calls, [false, true]);
    expect(service.created, isNull);
    await tester.ensureVisible(find.text('Save').last);
    await tester.tap(find.text('Save').last);
    await settle(tester);
    expect(service.created, {
      'kind': 'local',
      'launch': {'kind': 'native', ..._SetupRunners.paths},
    });
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.linux}));
  testWidgets('missing system prerequisites do not offer automatic installation', (tester) async {
    final service = _SetupRunners()..missing = true;
    await openSetup(tester, service);
    await tapKey(tester, 'automation-detect-runner');
    expect(find.textContaining('python3-venv'), findsOneWidget);
    expect(find.byKey(const ValueKey('automation-install-runner')), findsNothing);
    expect(service.calls, [false]);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.linux}));
  testWidgets('a detected existing Runner is selected and never offers a redundant installation', (tester) async {
    final service = _SetupRunners()..existing = true;
    await openSetup(tester, service);
    await tapKey(tester, 'automation-detect-runner');
    expect(find.byKey(const ValueKey('automation-install-runner')), findsNothing);
    expect(find.text(_SetupRunners.paths['python']!), findsOneWidget);
    await captureAutomationDialog(tester, 'runner-existing');
    expect(service.calls, [false]);
    await tester.ensureVisible(find.text('Save').last);
    await tester.tap(find.text('Save').last);
    await settle(tester);
    expect(service.calls, [false]);
    expect(service.created, {
      'kind': 'local',
      'launch': {'kind': 'native', ..._SetupRunners.paths},
    });
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.linux}));
  testWidgets('an outdated detected Runner offers an explicit update into a new preserved environment', (tester) async {
    final service = _SetupRunners()
      ..existing = true
      ..outdated = true;
    await openSetup(tester, service);
    await tapKey(tester, 'automation-detect-runner');
    expect(find.text('Install Runner'), findsNothing);
    expect(find.text('Update Runner'), findsOneWidget);
    expect(find.textContaining('needs an update for the current target checks'), findsOneWidget);
    await tapKey(tester, 'automation-install-runner');
    expect(service.calls, [false]);
    expect(find.textContaining('new, separate directory. Existing environments are preserved'), findsOneWidget);
    await tester.tap(find.text('Cancel').last);
    await settle(tester);
    expect(service.calls, [false]);
    await tapKey(tester, 'automation-install-runner');
    await tester.tap(find.text('Update Runner').last);
    await settle(tester);
    expect(service.calls, [false, true]);
    expect(find.byKey(const ValueKey('automation-install-runner')), findsNothing);
    expect(find.textContaining('needs an update for the current target checks'), findsNothing);
    expect(find.text('/home/fixture/runner-new/venv/bin/python'), findsOneWidget);
    await captureAutomationDialog(tester, 'runner-updated');
    await tester.ensureVisible(find.text('Save').last);
    await tester.pump();
    await tester.tap(find.text('Save').last);
    await settle(tester);
    expect((service.created!['launch'] as AutomationJson)['python'], '/home/fixture/runner-new/venv/bin/python');
    expect(_SetupRunners.paths['python'], '/home/fixture/runner/venv/bin/python');
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.linux}));
  testWidgets('remote SSH setup scans and installs only the selected host after approval', (tester) async {
    final service = _SetupRunners();
    await openSetup(tester, service);
    await tapKey(tester, 'automation-runner-mode');
    await tester.tap(find.text('Linux over SSH (manual runs)').last);
    await settle(tester);
    await tapKey(tester, 'automation-runner-choose-host');
    await tester.tap(find.text('Runner SSH fixture').last);
    await settle(tester);
    await tapKey(tester, 'automation-detect-runner');
    expect(service.targets.single, {'kind': 'remote', 'host_id': service.remoteHost.id.value});
    await tapKey(tester, 'automation-install-runner');
    expect(service.calls, [false]);
    await tester.tap(find.text('Install Runner').last);
    await settle(tester);
    expect(service.calls, [false, true]);
    expect(service.targets.every((target) => target['host_id'] == service.remoteHost.id.value), isTrue);
    await tester.ensureVisible(find.text('Save').last);
    await tester.tap(find.text('Save').last);
    await settle(tester);
    expect(service.created, {'kind': 'remote', 'host_id': service.remoteHost.id.value, 'helper': _SetupRunners.paths});
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.linux}));
  testWidgets('a scan completing after vault lock cannot populate the new profile', (tester) async {
    final service = _SetupRunners()..pendingSetup = Completer<AutomationJson>();
    final container = await openSetup(tester, service);
    await tapKey(tester, 'automation-detect-runner');
    await container.read(vaultServiceProvider).lock();
    await settle(tester);
    service.pendingSetup!.complete({
      'system': 'private-old-profile',
      'candidates': [_SetupRunners.paths],
    });
    await settle(tester);
    expect(find.textContaining('private-old-profile'), findsNothing);
    expect(service.created, isNull);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.linux}));
  testWidgets('closing the Runner form cancels only its own pending setup', (tester) async {
    final service = _SetupRunners()..pendingSetup = Completer<AutomationJson>();
    await openSetup(tester, service);
    await tapKey(tester, 'automation-detect-runner');
    await tester.tap(find.text('Close').last);
    await settle(tester);
    expect(service.cancelled, [service.operations.single]);
    service.pendingSetup!.complete({
      'system': 'late setup',
      'candidates': [_SetupRunners.paths],
    });
    await settle(tester);
    expect(find.textContaining('late setup'), findsNothing);
    expect(service.created, isNull);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.linux}));
  testWidgets('ANS01 pending application picker denies same-service AppServices replacement', (tester) async {
    final service = _Editor()..picker.pending = Completer<String?>();
    final container = await _open(tester, service);
    await tapKey(tester, 'automation-file-playbook.yml');
    await tapKey(tester, 'automation-open-editor');
    final original = container.read(appServicesProvider);
    container.updateOverrides([
      appServicesProvider.overrideWithValue(_sameInventoryServices(original)),
      automationServiceProvider.overrideWithValue(service),
      fileDialogServiceProvider.overrideWithValue(service.picker),
    ]);
    await settle(tester);
    service.picker.pending!.complete('C:/Owned Editor/editor.exe');
    await settle(tester);
    expect(service.picker.calls, 1);
    expect(service.opened, isEmpty);
    expect(service.savedExpected, isNull);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  testWidgets('ANS01 pending application picker cannot revive after hiding and returning to Automation', (
    tester,
  ) async {
    final service = _Editor()..picker.pending = Completer<String?>();
    final container = await _open(tester, service);
    await tapKey(tester, 'automation-file-playbook.yml');
    await tapKey(tester, 'automation-open-editor');
    container.read(routerProvider).go(AppRoutes.terminal);
    await settle(tester);
    container.read(routerProvider).go(AppRoutes.automation);
    await settle(tester);
    service.picker.pending!.complete('C:/Owned Editor/editor.exe');
    await settle(tester);
    expect(service.picker.calls, 1);
    expect(service.opened, isEmpty);
    expect(service.savedExpected, isNull);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  testWidgets('ANS01 dirty external-editor prompt cannot save into a replacement service', (tester) async {
    final service = _Editor();
    final container = await _open(tester, service);
    await tapKey(tester, 'automation-file-playbook.yml');
    await tester.enterText(find.byKey(const ValueKey('automation-editor')), '# original unsaved buffer');
    await tester.pump();
    await tapKey(tester, 'automation-open-editor');
    final oldSave = tester.widget<FilledButton>(find.widgetWithText(FilledButton, 'Save')).onPressed!;
    final replacement = _Editor();
    container.updateOverrides([
      appServicesProvider.overrideWithValue(container.read(appServicesProvider)),
      automationServiceProvider.overrideWithValue(replacement),
      fileDialogServiceProvider.overrideWithValue(replacement.picker),
    ]);
    // The original prompt callback can complete before its scheduled route
    // removal. It must not authorize a save against the replacement service.
    oldSave();
    await settle(tester);
    expect(service.savedExpected, isNull);
    expect(replacement.savedExpected, isNull);
    expect(service.picker.calls, 0);
    expect(replacement.picker.calls, 0);
    expect(service.opened, isEmpty);
    expect(replacement.opened, isEmpty);
    expect(
      tester.widget<TextField>(find.byKey(const ValueKey('automation-editor'))).controller!.text,
      '# original unsaved buffer',
    );
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  testWidgets('ANS01 explicit save then editor open uses only the resulting saved SHA', (tester) async {
    final service = _Editor();
    await _open(tester, service);
    await tapKey(tester, 'automation-file-playbook.yml');
    await tester.enterText(find.byKey(const ValueKey('automation-editor')), '# explicitly saved before opening');
    await tester.pump();
    await tapKey(tester, 'automation-open-editor');
    await tester.tap(find.widgetWithText(FilledButton, 'Save'));
    await settle(tester);
    expect(service.savedExpected, 'initial-hash');
    expect(service.opened.single['sha256'], 'saved-hash');
    expect(service.opened.single['id'], 'p');
    expect(service.opened.single['path'], 'playbook.yml');
    expect(service.text, '# explicitly saved before opening');
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  testWidgets('external editing is hidden on mobile platforms', (tester) async {
    final service = _Editor();
    await _open(tester, service);
    await tapKey(tester, 'automation-file-playbook.yml');
    expect(find.byKey(const ValueKey('automation-open-editor')), findsNothing);
    expect(service.opened, isEmpty);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.android, TargetPlatform.iOS}));
  testWidgets('external editor controls fit a narrow desktop workspace', (tester) async {
    final service = _Editor();
    await _open(tester, service, size: const Size(500, 850));
    await tester.scrollUntilVisible(
      find.byKey(const ValueKey('automation-file-playbook.yml')),
      100,
      scrollable: find
          .descendant(of: find.byKey(const ValueKey('automation-project-tree')), matching: find.byType(Scrollable))
          .first,
    );
    await tapKey(tester, 'automation-file-playbook.yml');
    await tapKey(tester, 'automation-open-editor');
    expect(service.opened, hasLength(1));
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.desktop());
  testWidgets('external editor receives the saved SHA and disk reload is explicit', (tester) async {
    final service = _Editor();
    await _open(tester, service);
    await tapKey(tester, 'automation-file-playbook.yml');
    await tapKey(tester, 'automation-open-editor');
    expect(service.opened.single, {
      'id': 'p',
      'path': 'playbook.yml',
      'sha256': 'initial-hash',
      'editor': {'kind': 'path', 'value': 'C:/Owned Editor/editor.exe'},
    });
    service.text = '# saved in external editor';
    service.hash = 'external-sha';
    expect(
      tester.widget<TextField>(find.byKey(const ValueKey('automation-editor'))).controller!.text,
      isNot(service.text),
    );
    await tapKey(tester, 'automation-reload-file');
    expect(tester.widget<TextField>(find.byKey(const ValueKey('automation-editor'))).controller!.text, service.text);
    expect(service.savedExpected, isNull);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.desktop());
  testWidgets('cancelling the external editor save prompt preserves the unsaved draft', (tester) async {
    final service = _Editor();
    await _open(tester, service);
    await tapKey(tester, 'automation-file-playbook.yml');
    await tester.enterText(find.byKey(const ValueKey('automation-editor')), '# private draft');
    await tester.pump();
    await tapKey(tester, 'automation-open-editor');
    await tester.tap(find.text('Cancel').last);
    await settle(tester);
    expect(service.opened, isEmpty);
    expect(service.picker.calls, 0);
    expect(service.savedExpected, isNull);
    expect(
      tester.widget<TextField>(find.byKey(const ValueKey('automation-editor'))).controller!.text,
      '# private draft',
    );
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.desktop());
  testWidgets('a late application choice cannot open a project after lock', (tester) async {
    final service = _Editor()..picker.pending = Completer<String?>();
    final container = await _open(tester, service);
    await tapKey(tester, 'automation-file-playbook.yml');
    await tapKey(tester, 'automation-open-editor');
    await container.read(vaultServiceProvider).lock();
    await settle(tester);
    service.picker.pending!.complete('C:/Owned Editor/editor.exe');
    await settle(tester);
    expect(service.opened, isEmpty);
    expect(find.byKey(const ValueKey('automation-editor')), findsNothing);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.desktop());
  testWidgets(
    'inventory original owner denies start after pending Runner connect and same-profile service replacement',
    (tester) async {
      final service = _Inventory()..pendingProbe = Completer<AutomationJson>();
      final container = await _open(tester, service);
      await inventoryInputs(tester);
      await tapKey(tester, 'automation-inventory-start');
      final replacement = _Inventory();
      container.updateOverrides([
        appServicesProvider.overrideWithValue(container.read(appServicesProvider)),
        automationServiceProvider.overrideWithValue(replacement),
        hostsProvider.overrideWith((ref) => Stream.value([service.member, service.other])),
      ]);
      await settle(tester);
      service.pendingProbe!.complete(<String, dynamic>{});
      await settle(tester);
      expect(service.chosenSources, isNull);
      expect(replacement.chosenSources, isNull);
      expect(service.discarded, isEmpty);
      expect(replacement.discarded, isEmpty);
      expect(find.byKey(const ValueKey('automation-inventory-start')), findsNothing);
      expect(tester.takeException(), isNull);
    },
  );
  testWidgets('inventory hidden then reopened cannot adopt a late preview and discards only original issued ID', (
    tester,
  ) async {
    final service = _Inventory()..pendingStart = Completer<AutomationJson>();
    final container = await _open(tester, service);
    await inventoryInputs(tester);
    await tapKey(tester, 'automation-inventory-start');
    container.read(routerProvider).go(AppRoutes.terminal);
    await settle(tester);
    container.read(routerProvider).go(AppRoutes.automation);
    await settle(tester);
    service.pendingStart!.complete({
      'version': 1,
      'preview_id': 'late-original',
      'project_id': 'p',
      'project_revision': 1,
      'state': 'preparing',
    });
    await settle(tester);
    expect(service.discarded, ['late-original']);
    expect(service.saved, isNull);
    expect(find.byKey(const ValueKey('automation-inventory-save')), findsNothing);
    expect(tester.takeException(), isNull);
  });
  testWidgets('inventory original owner denies commit after pending check and replacement service', (tester) async {
    final service = _Inventory()..pendingCheck = Completer<AutomationJson>();
    final container = await _open(tester, service);
    await inventoryInputs(tester);
    await tapKey(tester, 'automation-inventory-start');
    await tapKey(tester, 'automation-inventory-save');
    final replacement = _Inventory();
    container.updateOverrides([
      appServicesProvider.overrideWithValue(container.read(appServicesProvider)),
      automationServiceProvider.overrideWithValue(replacement),
      hostsProvider.overrideWith((ref) => Stream.value([service.member, service.other])),
    ]);
    await settle(tester);
    service.pendingCheck!.complete({
      'version': 1,
      'preview_id': 'preview1',
      'project_id': 'p',
      'valid': true,
      'conflict_count': 0,
      'conflicts': <AutomationJson>[],
    });
    await settle(tester);
    expect(service.saved, isNull);
    expect(replacement.saved, isNull);
    expect(service.discarded, ['preview1']);
    expect(replacement.discarded, isEmpty);
    expect(tester.takeException(), isNull);
  });
  testWidgets(
    'inventory changed source requires explicit rebind and another check before committing current provenance',
    (tester) async {
      final service = _Inventory();
      service.configuration = {
        'version': 1,
        'sources': [
          {'path': 'hosts.ini', 'format': 'ini'},
        ],
        'mode': 'external_only',
        'bindings': {
          'service_00000': {
            'host_id': service.member.id.value,
            'resolution': 'match',
            'source': {'address': '192.0.2.12', 'port': 22, 'username': null},
          },
        },
      };
      service.report = {
        'version': 1,
        'preview_id': 'preview1',
        'project_id': 'p',
        'valid': false,
        'conflict_count': 1,
        'conflicts': [
          {'alias': 'service_00000', 'kind': 'source_changed', 'saved': null},
        ],
      };
      await _open(tester, service);
      await toolbarAction(tester, 'automation-inventory-open');
      await tester.tap(find.byKey(const ValueKey('automation-inventory-runner')));
      await settle(tester);
      await tester.tap(find.text('Remote fixture').last);
      await settle(tester);
      await tapKey(tester, 'automation-inventory-start');
      expect(find.text('hosts.ini'), findsWidgets);
      expect(find.textContaining('deployer@192.0.2.10:2222'), findsOneWidget);
      await tapKey(tester, 'automation-inventory-save');
      expect(service.saved, isNull);
      expect(service.checks, 1);
      await tapKey(tester, 'automation-inventory-rebind-service_00000');
      expect(service.saved, isNull);
      expect(service.checks, 1);
      service.report = null;
      await tapKey(tester, 'automation-inventory-save');
      expect(service.checks, 2);
      expect(service.saved!['bindings'], {
        'service_00000': {
          'host_id': service.member.id.value,
          'resolution': 'match',
          'source': {'address': '192.0.2.10', 'port': 2222, 'username': 'deployer'},
        },
      });
      expect(service.saved!.containsKey('credentials'), false);
      expect(tester.takeException(), isNull);
    },
  );
  testWidgets('inventory parent action cannot adopt service replacement before deferred dialog build', (tester) async {
    final service = _Inventory()..pendingConfiguration = Completer<AutomationJson>();
    final container = await _open(tester, service);
    await tapKey(tester, 'automation-settings-menu');
    // Use the real menu action before its scheduled dialog builds, then replace
    // AppServices while preserving service/scope. Captured parent must retire.
    await tester.tap(find.byKey(const ValueKey('automation-inventory-open')));
    final original = container.read(appServicesProvider);
    final replacement = _sameInventoryServices(original);
    container.updateOverrides([
      appServicesProvider.overrideWithValue(replacement),
      automationServiceProvider.overrideWithValue(service),
      hostsProvider.overrideWith((ref) => Stream.value([service.member, service.other])),
    ]);
    await settle(tester);
    service.pendingConfiguration!.complete({
      'version': 1,
      'project_id': 'p',
      'project_revision': 1,
      'configuration': null,
    });
    await settle(tester);
    expect(find.byKey(const ValueKey('automation-inventory-start')), findsNothing);
    expect(service.chosenSources, isNull);
    expect(service.saved, isNull);
    expect(service.configurations, 0);
    expect(identical(original, replacement), false);
    expect(tester.takeException(), isNull);
  });
  testWidgets('inventory directory selection keeps the captured file list separate from bindings', (tester) async {
    final service = _Inventory()..directoryFixture = true;
    await _open(tester, service);
    await toolbarAction(tester, 'automation-inventory-open');
    await tester.tap(find.byKey(const ValueKey('automation-inventory-runner')));
    await settle(tester);
    await tester.tap(find.text('Remote fixture').last);
    await settle(tester);
    await tapKey(tester, 'automation-inventory-file-inventory');
    await tapKey(tester, 'automation-inventory-start');
    await tapKey(tester, 'automation-inventory-resolved-files');
    expect(find.text('inventory/group_vars/web.yml'), findsOneWidget);
    await tester.tap(find.text('Close').last);
    await settle(tester);
    await tapKey(tester, 'automation-inventory-save');
    expect(service.saved!['sources'], [
      {'path': 'inventory', 'format': 'directory'},
    ]);
    expect(service.saved!.containsKey('resolved_sources'), false);
    expect(tester.takeException(), isNull);
  });
  testWidgets('inventory conflicts are shown and removing a missing alias needs another explicit save', (tester) async {
    final service = _Inventory()
      ..configuration = {
        'version': 1,
        'sources': [
          {'path': 'hosts.ini', 'format': 'ini'},
        ],
        'mode': 'external_only',
        'bindings': {
          'removed_alias': {
            'host_id': 'missing-host',
            'resolution': 'match',
            'source': {'address': '192.0.2.12', 'port': 22, 'username': null},
          },
        },
      }
      ..report = {
        'version': 1,
        'preview_id': 'preview1',
        'project_id': 'p',
        'valid': false,
        'conflict_count': 1,
        'conflicts': [
          {'alias': 'removed_alias', 'kind': 'source_missing', 'saved': null},
        ],
      };
    await _open(tester, service);
    await toolbarAction(tester, 'automation-inventory-open');
    await tester.tap(find.byKey(const ValueKey('automation-inventory-runner')));
    await settle(tester);
    await tester.tap(find.text('Remote fixture').last);
    await settle(tester);
    await tapKey(tester, 'automation-inventory-start');
    await tapKey(tester, 'automation-inventory-save');
    expect(service.saved, isNull);
    expect(find.text('removed_alias: Entry is no longer in the selected sources'), findsOneWidget);
    await tapKey(tester, 'automation-inventory-conflict-remove-removed_alias');
    expect(service.saved, isNull);
    service.report = null;
    await tapKey(tester, 'automation-inventory-save');
    expect(service.saved!['bindings'], isEmpty);
    expect(tester.takeException(), isNull);
  });
  testWidgets('external inventory saves only explicit bindings and preserves the unsaved YAML editor', (tester) async {
    final service = _Inventory();
    await _open(tester, service);
    await tapKey(tester, 'automation-file-playbook.yml');
    await enterKey(tester, 'automation-editor', '---\n# keep unsaved inventory work\n');
    await inventoryInputs(tester);
    await tapKey(tester, 'automation-inventory-start');
    await tapKey(tester, 'automation-inventory-binding-service_00000');
    await tapKey(tester, 'automation-inventory-choose-${service.member.id}');
    await tester.tap(
      find.widgetWithText(
        CheckboxListTile,
        'Use the saved host address, port and user when they differ from the source',
      ),
    );
    await settle(tester);
    await tapKey(tester, 'automation-inventory-save');
    expect(service.saved, {
      'version': 1,
      'sources': [
        {'path': 'hosts.ini', 'format': 'ini'},
      ],
      'mode': 'external_only',
      'bindings': {
        'service_00000': {
          'host_id': service.member.id.value,
          'resolution': 'use_saved_host',
          'source': {'address': '192.0.2.10', 'port': 2222, 'username': 'deployer'},
        },
      },
    });
    expect(service.discarded, isEmpty);
    expect(
      tester.widget<TextField>(find.byKey(const ValueKey('automation-editor'))).controller!.text,
      '---\n# keep unsaved inventory work\n',
    );
    expect(service.savedExpected, isNull);
    expect(tester.takeException(), isNull);
  });
  testWidgets('large inventory pages and search stay bounded on a narrow window', (tester) async {
    final service = _Inventory()..count = 10000;
    await _open(tester, service, size: const Size(620, 820));
    await inventoryInputs(tester);
    await tapKey(tester, 'automation-inventory-start');
    expect(find.byKey(const ValueKey('automation-inventory-binding-service_00050')), findsNothing);
    await tapKey(tester, 'automation-inventory-next');
    expect(find.byKey(const ValueKey('automation-inventory-binding-service_00050')), findsOneWidget);
    await enterKey(tester, 'automation-inventory-search', 'service_09999');
    await settle(tester);
    expect(find.byKey(const ValueKey('automation-inventory-binding-service_09999')), findsOneWidget);
    expect(find.byKey(const ValueKey('automation-inventory-binding-service_00050')), findsNothing);
    await tester.tap(find.text('Cancel').last);
    await settle(tester);
    expect(service.discarded, ['preview1']);
    expect(tester.takeException(), isNull);
  });
  testWidgets('a stale inventory search response cannot replace the current search', (tester) async {
    final service = _Inventory()..count = 2;
    await _open(tester, service);
    await inventoryInputs(tester);
    await tapKey(tester, 'automation-inventory-start');
    service.delayedQuery = 'service_00000';
    service.pendingPage = Completer<AutomationJson>();
    await enterKey(tester, 'automation-inventory-search', 'service_00000');
    await settle(tester);
    await enterKey(tester, 'automation-inventory-search', 'service_00001');
    await settle(tester);
    service.pendingPage!.complete(service.result(query: 'service_00000'));
    await settle(tester);
    expect(find.byKey(const ValueKey('automation-inventory-binding-service_00001')), findsOneWidget);
    expect(find.byKey(const ValueKey('automation-inventory-binding-service_00000')), findsNothing);
    await tester.tap(find.text('Cancel').last);
    await settle(tester);
    expect(tester.takeException(), isNull);
  });
  testWidgets('a late inventory start after closing cancels exactly its preview', (tester) async {
    final service = _Inventory()..pendingStart = Completer<AutomationJson>();
    await _open(tester, service);
    await inventoryInputs(tester);
    await tester.tap(find.byKey(const ValueKey('automation-inventory-start')));
    await settle(tester);
    await tester.tap(find.text('Cancel').last);
    await settle(tester);
    service.pendingStart!.complete({
      'version': 1,
      'preview_id': 'late-owned-preview',
      'project_id': 'p',
      'project_revision': 1,
      'state': 'preparing',
    });
    await settle(tester);
    expect(service.discarded, ['late-owned-preview']);
    expect(find.byKey(const ValueKey('automation-inventory-start')), findsNothing);
    expect(tester.takeException(), isNull);
  });
  testWidgets('inventory clear needs confirmation and does not remove project files', (tester) async {
    final service = _Inventory()
      ..configuration = {
        'version': 1,
        'sources': [
          {'path': 'hosts.ini', 'format': 'ini'},
        ],
        'mode': 'external_only',
        'bindings': <String, dynamic>{},
      };
    await _open(tester, service);
    await toolbarAction(tester, 'automation-inventory-open');
    await tapKey(tester, 'automation-inventory-clear');
    await tester.tap(find.text('Cancel').last);
    await settle(tester);
    expect(service.clears, 0);
    await tapKey(tester, 'automation-inventory-clear');
    await tester.tap(find.widgetWithText(FilledButton, 'Disconnect external inventory').last);
    await settle(tester);
    expect(service.clears, 1);
    expect(find.byKey(const ValueKey('automation-file-playbook.yml')), findsOneWidget);
    expect(tester.takeException(), isNull);
  });
  testWidgets('a late inventory configuration cannot reopen plaintext after lock', (tester) async {
    final service = _Inventory()..pendingConfiguration = Completer<AutomationJson>();
    final container = await _open(tester, service);
    await toolbarAction(tester, 'automation-inventory-open');
    await container.read(vaultServiceProvider).lock();
    await settle(tester);
    service.pendingConfiguration!.complete({
      'version': 1,
      'project_id': 'p',
      'project_revision': 1,
      'configuration': null,
    });
    await settle(tester);
    expect(find.byKey(const ValueKey('automation-inventory-start')), findsNothing);
    expect(tester.takeException(), isNull);
  });
  Future<void> gitInputs(WidgetTester tester) async {
    await toolbarAction(tester, 'automation-git-open');
    await enterKey(tester, 'automation-git-name', 'Imported source');
    await enterKey(tester, 'automation-git-url', 'https://example.test/repo.git');
    expect(
      find.descendant(of: find.byType(AlertDialog).last, matching: find.byType(GlassSelect<String?>)),
      findsNothing,
    );
  }

  testWidgets('Git source remains unavailable until verified import completes', (tester) async {
    final service = _Sources();
    await _open(tester, service);
    await tapKey(tester, 'automation-project-picker-toggle');
    await gitInputs(tester);
    await tapKey(tester, 'automation-git-start');
    expect(service.importRunnerId, '');
    expect(service.importRunnerConnections, 0);
    final tile = tester.widget<ListTile>(find.byKey(const ValueKey('automation-project-imported')));
    expect(tile.onTap, isNull);
    service.ready = true;
    await tester.pump(const Duration(seconds: 1));
    await settle(tester);
    expect(find.text('Git · main · fixture-commit'), findsOneWidget);
    expect(find.text('There are local changes relative to the imported commit.'), findsNothing);
    await tapKey(tester, 'automation-file-playbook.yml');
    await enterKey(tester, 'automation-editor', '---\n# a manual change after import\n');
    expect(find.text('There are local changes relative to the imported commit.'), findsOneWidget);
    expect(service.cancelled, isNull);
    expect(tester.takeException(), isNull);
  });
  testWidgets('closing a pending Git import cancels only its import ID', (tester) async {
    final service = _Sources();
    await _open(tester, service);
    await gitInputs(tester);
    await tapKey(tester, 'automation-git-start');
    await tester.tap(find.text('Cancel').last);
    await settle(tester);
    expect(service.cancelled, 'imported');
    expect(tester.takeException(), isNull);
  });
  testWidgets('anonymous HTTP Git imports stay local and do not send stale HTTPS trust settings', (tester) async {
    final service = _Sources();
    await _open(tester, service);
    await gitInputs(tester);
    await tester.tap(find.text('Use a custom certificate authority'));
    await settle(tester);
    await enterKey(tester, 'automation-git-ca', 'HTTPS only');
    await enterKey(tester, 'automation-git-url', 'http://git.test/repo.git');
    expect(find.byKey(const ValueKey('automation-git-ca')), findsNothing);
    expect(find.textContaining('HTTP downloads are unencrypted'), findsOneWidget);
    await tapKey(tester, 'automation-git-start');
    expect(service.importUrl, 'http://git.test/repo.git');
    expect(service.importCa, isNull);
    expect(service.importRunnerId, '');
  });
  testWidgets('missing local Git is explained without connecting an Ansible Runner', (tester) async {
    final service = _Sources()
      ..gitFailure = const AppException(
        AppErrorCode.validation,
        'local Git unavailable',
        args: {'field': 'automation_git', 'rule': 'git_not_installed'},
      );
    await _open(tester, service);
    await gitInputs(tester);
    await tapKey(tester, 'automation-git-start');
    expect(find.textContaining('Install Git on this computer'), findsOneWidget);
    expect(service.importRunnerConnections, 0);
    expect(service.started, isFalse);
    expect(find.byKey(const ValueKey('automation-git-start')), findsOneWidget);
    expect(tester.takeException(), isNull);
  });
  testWidgets('a late Git start after closing the dialog is cancelled without reopening it', (tester) async {
    final service = _Sources()..pendingStart = Completer<AutomationProject>();
    await _open(tester, service);
    await gitInputs(tester);
    await tester.tap(find.byKey(const ValueKey('automation-git-start')));
    await settle(tester);
    await tester.tap(find.text('Cancel').last);
    await settle(tester);
    service.pendingStart!.complete(_Sources.imported);
    await settle(tester);
    expect(service.cancelled, 'imported');
    expect(find.byKey(const ValueKey('automation-git-start')), findsNothing);
    expect(tester.takeException(), isNull);
  });
  Future<void> aiPreview(WidgetTester tester) async {
    await tapKey(tester, 'automation-file-playbook.yml');
    await tapKey(tester, 'automation-ai-open');
    await tester.tap(find.byType(GlassSelect<String?>));
    await settle(tester);
    await tester.tap(find.text('AI fixture · fixture-model').last);
    await settle(tester);
    await enterKey(tester, 'automation-ai-task', 'Create a file and assert the result');
    await tapKey(tester, 'automation-ai-preview');
  }

  testWidgets('AI previews context and inserts only an unsaved editable draft', (tester) async {
    final service = _Ai();
    await _open(tester, service);
    final original = service.text;
    await aiPreview(tester);
    expect(service.generations, 0);
    expect(service.includedContext, original);
    expect(find.text('Filtered context fixture'), findsOneWidget);
    await tapKey(tester, 'automation-ai-generate');
    expect(service.generations, 1);
    expect(service.text, original);
    expect(service.savedExpected, isNull);
    await tapKey(tester, 'automation-ai-use');
    final editor = tester.widget<TextField>(find.byKey(const ValueKey('automation-editor')));
    expect(editor.controller!.text, contains('proposed draft only'));
    expect(service.text, original);
    await tapKey(tester, 'automation-save');
    expect(service.savedExpected, 'initial-hash');
    expect(service.text, contains('proposed draft only'));
    expect(tester.takeException(), isNull);
  });
  testWidgets('AI format failure has a useful message and preserves the original editor', (tester) async {
    final service = _Ai()..generationError = const AutomationAiFailed('invalid_response');
    await _open(tester, service);
    final original = service.text;
    await aiPreview(tester);
    await tapKey(tester, 'automation-ai-generate');
    expect(find.textContaining('required format'), findsOneWidget);
    expect(service.text, original);
    expect(service.savedExpected, isNull);
    expect(find.byKey(const ValueKey('automation-ai-use')), findsNothing);
    expect(tester.takeException(), isNull);
  });
  testWidgets('AI proposal has explicit Before After comparison and rejection preserves the editor', (tester) async {
    final service = _Ai();
    await _open(tester, service);
    final original = service.text;
    await aiPreview(tester);
    await tapKey(tester, 'automation-ai-generate');
    expect(find.text('Accept changes'), findsOneWidget);
    expect(find.text('Reject'), findsOneWidget);
    await captureAutomationDialog(tester, 'ai-proposal-diff');
    await tester.tap(find.text('Before'));
    await settle(tester);
    expect(find.descendant(of: find.byType(AlertDialog).last, matching: find.text(original)), findsOneWidget);
    await tester.tap(find.text('After'));
    await settle(tester);
    expect(find.textContaining('# proposed draft only'), findsOneWidget);
    await tapKey(tester, 'automation-ai-reject');
    expect(tester.widget<TextField>(find.byKey(const ValueKey('automation-editor'))).controller!.text, original);
    expect(service.savedExpected, isNull);
    expect(service.text, original);
    expect(tester.takeException(), isNull);
  });
  testWidgets('AI preview displays sanitized task and YAML while keeping the exact request available', (tester) async {
    const yaml = '---\n- name: Sanitized selected file\n  hosts: all\n  tasks: []\n';
    final exact =
        'Trusted instructions\n\n${jsonEncode({'task': 'sanitized task', 'path': 'playbook.yml', 'selected_file': yaml})}';
    final service = _Ai()..previewText = exact;
    await _open(tester, service, size: const Size(650, 800));
    await tester.scrollUntilVisible(
      find.byKey(const ValueKey('automation-file-playbook.yml')),
      100,
      scrollable: find
          .descendant(of: find.byKey(const ValueKey('automation-project-tree')), matching: find.byType(Scrollable))
          .first,
    );
    await aiPreview(tester);
    expect(find.text('sanitized task'), findsOneWidget);
    expect(find.text(yaml), findsOneWidget);
    expect(find.text(exact), findsNothing);
    await captureAutomationDialog(tester, 'ai-context-preview');
    await tapKey(tester, 'automation-ai-exact-request');
    expect(find.text(exact), findsOneWidget);
    expect(service.generations, 0);
    await tester.tap(find.text('Back'));
    await settle(tester);
    expect(
      tester.widget<TextField>(find.byKey(const ValueKey('automation-ai-task'))).controller!.text,
      'Create a file and assert the result',
    );
    expect(service.discarded, 'ai-review-fixture');
    expect(tester.takeException(), isNull);
  });
  testWidgets('AI acceptance preserves editor changes made during generation', (tester) async {
    final service = _Ai()..pendingProposal = Completer<AutomationJson>();
    await _open(tester, service);
    await aiPreview(tester);
    final controller = tester.widget<TextField>(find.byKey(const ValueKey('automation-editor'))).controller!;
    await tester.tap(find.byKey(const ValueKey('automation-ai-generate')));
    await tester.pump();
    controller.text = '---\n# manual change during generation\n';
    service.pendingProposal!.complete(service.proposal());
    await settle(tester);
    await tapKey(tester, 'automation-ai-use');
    expect(controller.text, '---\n# manual change during generation\n');
    expect(find.textContaining('The file or project changed while AI'), findsOneWidget);
    expect(service.savedExpected, isNull);
    expect(service.text, '---\n- hosts: all\n');
    expect(tester.takeException(), isNull);
  });
  testWidgets('AI requirements require diff approval, preserve drafts on cancel, save constraints before YAML', (
    tester,
  ) async {
    final service = _AiRequirements();
    await _open(tester, service);
    final original = service.text;
    await aiPreview(tester);
    await tapKey(tester, 'automation-ai-generate');
    await tapKey(tester, 'automation-ai-use');
    await tapKey(tester, 'automation-save');
    expect(service.writes, isEmpty);
    expect(find.text(automationRequirementsFile), findsOneWidget);
    await tester.tap(find.text('Cancel').last);
    await settle(tester);
    expect(service.text, original);
    expect(service.requirements, isNull);
    final editor = tester.widget<TextField>(find.byKey(const ValueKey('automation-editor')));
    expect(editor.controller!.text, contains('proposed draft only'));
    // Manual changes after generation remain the text that is saved.
    await enterKey(tester, 'automation-editor', '${editor.controller!.text}# retained manual edit\n');
    await tapKey(tester, 'automation-save');
    await tapKey(tester, 'automation-ai-save-requirements');
    expect(service.writes, [automationRequirementsFile, 'playbook.yml']);
    expect(service.text, contains('retained manual edit'));
    final document = jsonDecode(service.requirements!) as AutomationJson;
    expect(((document['playbooks'] as AutomationJson)['playbook.yml'] as AutomationJson)['required_extra_vars'], [
      'fixture_input',
    ]);
    expect(service.savedExpected, 'initial-hash');
    expect(tester.takeException(), isNull);
  });
  testWidgets('contract conflicts do not write YAML; second-write conflicts preserve constraints and editable draft', (
    tester,
  ) async {
    final service = _AiRequirements()..contractConflict = true;
    await _open(tester, service);
    final original = service.text;
    await aiPreview(tester);
    await tapKey(tester, 'automation-ai-generate');
    await tapKey(tester, 'automation-ai-use');
    await tapKey(tester, 'automation-save');
    await tapKey(tester, 'automation-ai-save-requirements');
    expect(service.writes, isEmpty);
    expect(service.text, original);
    service.contractConflict = false;
    service.conflict = true;
    await tapKey(tester, 'automation-save');
    await tapKey(tester, 'automation-ai-save-requirements');
    expect(service.writes, [automationRequirementsFile]);
    expect(service.text, original);
    expect(service.requirements, contains('fixture_input'));
    expect(find.textContaining('Requirements have been saved; the playbook has not.'), findsOneWidget);
    final editor = tester.widget<TextField>(find.byKey(const ValueKey('automation-editor')));
    expect(editor.controller!.text, contains('proposed draft only'));
    service.conflict = false;
    await tapKey(tester, 'automation-save');
    await tapKey(tester, 'automation-ai-save-requirements');
    expect(service.writes.last, 'playbook.yml');
    expect(tester.takeException(), isNull);
  });
  testWidgets('locking during requirements review discards the dialog without file writes', (tester) async {
    final service = _AiRequirements();
    final container = await _open(tester, service);
    await aiPreview(tester);
    await tapKey(tester, 'automation-ai-generate');
    await tapKey(tester, 'automation-ai-use');
    await tapKey(tester, 'automation-save');
    expect(find.byKey(const ValueKey('automation-ai-save-requirements')), findsOneWidget);
    await container.read(vaultServiceProvider).lock();
    await settle(tester);
    expect(find.byKey(const ValueKey('automation-ai-save-requirements')), findsNothing);
    expect(find.byKey(const ValueKey('automation-editor')), findsNothing);
    expect(service.writes, isEmpty);
    expect(service.requirements, isNull);
    expect(tester.takeException(), isNull);
  });
  testWidgets('requirements review remains usable on a narrow viewport', (tester) async {
    final service = _AiRequirements();
    await _open(tester, service, size: const Size(430, 820));
    await tester.scrollUntilVisible(
      find.byKey(const ValueKey('automation-file-playbook.yml')),
      100,
      scrollable: find
          .descendant(of: find.byKey(const ValueKey('automation-project-tree')), matching: find.byType(Scrollable))
          .first,
    );
    await aiPreview(tester);
    await tapKey(tester, 'automation-ai-generate');
    await tapKey(tester, 'automation-ai-use');
    await tapKey(tester, 'automation-save');
    expect(find.byKey(const ValueKey('automation-ai-save-requirements')), findsOneWidget);
    expect(tester.takeException(), isNull);
    await tapKey(tester, 'automation-ai-save-requirements');
    expect(service.writes, [automationRequirementsFile, 'playbook.yml']);
    expect(tester.takeException(), isNull);
  });
  testWidgets('a future manual requirements file is preserved and blocks saving the generated YAML', (tester) async {
    final service = _AiRequirements()..requirements = '{"version":2,"playbooks":{},"future":true}';
    final original = service.text, manual = service.requirements;
    await _open(tester, service);
    await aiPreview(tester);
    await tapKey(tester, 'automation-ai-generate');
    await tapKey(tester, 'automation-ai-use');
    await tapKey(tester, 'automation-save');
    expect(find.byKey(const ValueKey('automation-ai-save-requirements')), findsNothing);
    expect(service.writes, isEmpty);
    expect(service.text, original);
    expect(service.requirements, manual);
    expect(
      tester.widget<TextField>(find.byKey(const ValueKey('automation-editor'))).controller!.text,
      contains('proposed draft only'),
    );
    expect(tester.takeException(), isNull);
  });
  testWidgets('a late AI proposal cannot restore the editor after lock', (tester) async {
    final service = _Ai()..pendingProposal = Completer<AutomationJson>();
    final container = await _open(tester, service);
    await aiPreview(tester);
    await tester.tap(find.byKey(const ValueKey('automation-ai-generate')));
    await tester.pump();
    await container.read(vaultServiceProvider).lock();
    await settle(tester);
    service.pendingProposal!.complete(service.proposal());
    await settle(tester);
    expect(find.textContaining('proposed draft only'), findsNothing);
    expect(service.savedExpected, isNull);
    expect(tester.takeException(), isNull);
  });
  testWidgets('Runner diagnostics show each isolation check and never start a playbook', (tester) async {
    await _open(tester, _Runners()..ready = true);
    await toolbarAction(tester, 'automation-runners-open');
    await settle(tester);
    await tapKey(tester, 'automation-diagnose-runner');
    expect(find.text('Environment ready'), findsOneWidget);
    expect(find.text('Network isolation'), findsNothing);
    await captureAutomationDialog(tester, 'runner-ready');
    await tester.tap(find.text('Components and isolation'));
    await settle(tester);
    await tester.ensureVisible(find.text('Network isolation'));
    expect(find.text('Network isolation'), findsOneWidget);
    expect(find.textContaining('Execution requires separate'), findsOneWidget);
    expect(find.byTooltip('Remove connection profile'), findsOneWidget);
    await captureAutomationDialog(tester, 'runner-details');
    expect(tester.takeException(), isNull);
  });

  testWidgets('a pending Runner diagnostic cannot reopen a dialog after lock', (tester) async {
    final service = _Runners()..pendingProbe = Completer<AutomationJson>();
    final container = await _open(tester, service);
    await toolbarAction(tester, 'automation-runners-open');
    await settle(tester);
    await tapKey(tester, 'automation-diagnose-runner');
    await container.read(vaultServiceProvider).lock();
    await settle(tester);
    service.pendingProbe!.complete({
      'probe': {'private-old-profile': 'must not be rendered'},
    });
    await settle(tester);
    expect(find.text('Runner environments'), findsNothing);
    expect(find.textContaining('must not be rendered'), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('project editor saves against the read revision and keeps the command palette shortcut', (tester) async {
    final service = _Projects();
    await _open(tester, service);
    await tapKey(tester, 'automation-file-playbook.yml');
    await enterKey(tester, 'automation-editor', '---\n# edited manually\n');
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyS);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    await settle(tester);
    expect(service.savedExpected, 'initial-hash');
    expect(service.text, contains('edited manually'));
    expect(isEnabled(tester, 'automation-save'), isFalse);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyK);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    await settle(tester);
    expect(find.byKey(const ValueKey('command-palette')), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('failed save preserves unsaved edits and cancel does not replace the editor', (tester) async {
    final service = _Projects()..conflict = true;
    await _open(tester, service);
    await tapKey(tester, 'automation-file-playbook.yml');
    await enterKey(tester, 'automation-editor', 'my unsaved changes');
    await tapKey(tester, 'automation-save');
    expect(find.textContaining('changed outside ConsoleCrypt'), findsOneWidget);
    expect(
      tester.widget<TextField>(find.byKey(const ValueKey('automation-editor'))).controller!.text,
      'my unsaved changes',
    );
    await tapKey(tester, 'automation-project-picker-toggle');
    await tapKey(tester, 'automation-project-p');
    expect(find.text('Save changes to this file?'), findsOneWidget);
    await tester.tap(find.text('Cancel').last);
    await settle(tester);
    expect(
      tester.widget<TextField>(find.byKey(const ValueKey('automation-editor'))).controller!.text,
      'my unsaved changes',
    );
    expect(tester.takeException(), isNull);
  });

  testWidgets('a delayed file read cannot repopulate plaintext after locking', (tester) async {
    final service = _Projects()..pendingRead = Completer<AutomationProjectFile>();
    final container = await _open(tester, service);
    await tapKey(tester, 'automation-file-playbook.yml');
    await container.read(vaultServiceProvider).lock();
    await settle(tester);
    service.pendingRead!.complete(
      const AutomationProjectFile(path: 'playbook.yml', text: 'old private content', sha256: 'hash', lineEndings: 'lf'),
    );
    await settle(tester);
    expect(find.byKey(const ValueKey('automation-editor')), findsNothing);
    expect(find.textContaining('old private content'), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('narrow project workspace exposes files without horizontal overflow', (tester) async {
    await _open(tester, _Projects(), size: const Size(430, 900));
    await tester.scrollUntilVisible(
      find.byKey(const ValueKey('automation-file-playbook.yml')),
      100,
      scrollable: find
          .descendant(of: find.byKey(const ValueKey('automation-project-tree')), matching: find.byType(Scrollable))
          .first,
    );
    await tapKey(tester, 'automation-file-playbook.yml');
    expect(find.byKey(const ValueKey('automation-editor')), findsOneWidget);
    expect(tester.takeException(), isNull);
  });
}
