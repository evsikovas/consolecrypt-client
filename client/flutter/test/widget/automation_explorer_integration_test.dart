import 'dart:convert';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_library_files.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/models/sftp_edit.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:crypto/crypto.dart' as crypto;
import 'package:flutter/gestures.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';
import '../helpers/automation_projects_fixture.dart' show Projects;

/// Synthetic file service: these assertions cover the real workspace dispatch,
/// selected editor and CAS arguments, not filesystem moves or Vault encryption.
class _Files extends Projects
    implements
        AutomationMoveService,
        AutomationFileActionsService,
        AutomationProjectRemovalService,
        AutomationVaultFileService,
        AutomationEditorService {
  _Files() {
    projectsCreated.add(
      const AutomationProject(
        id: '11111111-1111-4111-8111-111111111111',
        name: 'Explorer project',
        readOnly: false,
        revision: 7,
      ),
    );
    contents['blob.bin'] = 'binary-placeholder';
  }
  String profileId = '';
  final reads = <String>[], moves = <(String, String, String)>[];
  final vaultRequests = <AutomationJson>[];
  final directories = {'roles', 'files'};
  AutomationJson? openedEditor;
  String hash(String text) => crypto.sha256.convert(utf8.encode(text)).toString();

  @override
  Future<AutomationProject> renameProject(String id, {required int revision, required String name}) async {
    final index = projectsCreated.indexWhere((item) => item.id == id);
    expect(projectsCreated[index].revision, revision);
    return projectsCreated[index] = AutomationProject(id: id, name: name, readOnly: false, revision: revision + 1);
  }

  @override
  Future<void> removeProject(String id, {required int revision}) async {
    expect(projectsCreated.singleWhere((item) => item.id == id).revision, revision);
    projectsCreated.removeWhere((item) => item.id == id);
  }

  @override
  Future<void> openProjectEditor(
    String projectId,
    String path, {
    required String expectedSha256,
    required AutomationJson editor,
  }) async {
    expect(expectedSha256, hash(contents[path]!));
    openedEditor = editor;
  }

  @override
  Future<void> editProjectFileEntry(
    String projectId, {
    required String path,
    required String expectedSha256,
    required String action,
    String? newName,
  }) async {
    if (action.endsWith('_directory')) {
      expect(expectedSha256, hash(jsonEncode(contents)));
      expect(directories.remove(path), isTrue);
      if (action == 'rename_directory') {
        directories.add(newName!);
        for (final key in contents.keys.where((key) => key.startsWith('$path/')).toList()) {
          contents['$newName${key.substring(path.length)}'] = contents.remove(key)!;
        }
      }
      return;
    }
    expect(expectedSha256, hash(contents[path]!));
    final text = contents.remove(path)!;
    if (action == 'rename') {
      final parts = path.split('/');
      contents[[...parts.take(parts.length - 1), newName!].join('/')] = text;
    } else {
      expect(action, 'delete');
    }
  }

  @override
  Future<AutomationProjectRevision> files(String projectId) async => AutomationProjectRevision(
    digest: hash(jsonEncode(contents)),
    bytes: 100,
    entries: [
      for (final directory in directories) AutomationFileEntry(path: directory, directory: true),
      for (final entry in contents.entries)
        AutomationFileEntry(path: entry.key, directory: false, sha256: hash(entry.value)),
    ],
  );
  @override
  Future<AutomationProjectFile> readFile(String projectId, String path) async {
    reads.add(path);
    if (path.endsWith('.bin')) throw StateError('The UI must use the snapshot hash to move binary files');
    return AutomationProjectFile(path: path, text: contents[path]!, sha256: hash(contents[path]!), lineEndings: 'lf');
  }

  @override
  Future<void> moveProjectFile(
    String projectId, {
    required String sourcePath,
    required String destinationDirectory,
    required String expectedSha256,
  }) async {
    expect(expectedSha256, hash(contents[sourcePath]!));
    final path = destinationDirectory.isEmpty
        ? sourcePath.split('/').last
        : '$destinationDirectory/${sourcePath.split('/').last}';
    expect(contents.containsKey(path), false);
    moves.add((sourcePath, destinationDirectory, expectedSha256));
    contents[path] = contents.remove(sourcePath)!;
  }

  @override
  Future<AutomationJson> vaultFileContext(String projectId) async => {
    'frame': {
      'version': 1,
      'profileId': profileId,
      'notificationScopeId': 'owned-ui-frame',
      'projectId': projectId,
      'projectRevision': 7,
    },
  };
  @override
  Future<AutomationProjectFile> transformVaultFile(AutomationJson request, {SecretText? password}) async {
    vaultRequests.add(request);
    expect(password!.expose(), 'fixture-password');
    const encrypted =
        r'$ANSIBLE_VAULT;1.1;AES256'
        '\n001122\n';
    final path = request['path'] as String;
    contents[path] = encrypted;
    return AutomationProjectFile(path: path, text: encrypted, sha256: hash(encrypted), lineEndings: 'lf');
  }
}

class _AiFiles extends _Files implements AutomationAiService {
  _AiFiles() {
    contents['roles/web/tasks/main.yml'] = '---\n- name: Role task\n  ansible.builtin.debug:\n    msg: hello\n';
  }
  int calls = 0;
  @override
  Future<AutomationJson> prepareAi({
    required String providerId,
    required String task,
    required String path,
    String? selectedText,
  }) async {
    calls++;
    throw StateError('No implicit AI preparation');
  }

  @override
  Future<AutomationJson> generateAi(String reviewId) async {
    calls++;
    throw StateError('No implicit generation');
  }

  @override
  Future<void> discardAi(String reviewId) async {}
}

Future<void> _open(WidgetTester tester, _Files service) async {
  final backend = testBackend();
  addTearDown(backend.dispose);
  await backend.debugCreateUnlockedLocalProfile(name: 'Explorer');
  await pumpApp(tester, backend, overrides: [automationServiceProvider.overrideWithValue(service)]);
  final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
  service.profileId = container.read(activeProfileProvider)!.id.value;
  container.read(routerProvider).go(AppRoutes.automation);
  await settle(tester);
  await tapKey(tester, 'automation-project-${service.projectsCreated.single.id}');
  await tapKey(tester, 'automation-file-playbook.yml');
}

Future<void> dragFile(WidgetTester tester, String path, String directory) async {
  final gesture = await tester.startGesture(tester.getCenter(find.byKey(ValueKey('automation-file-$path'))));
  await gesture.moveBy(const Offset(15, 0));
  await tester.pump();
  await gesture.moveTo(tester.getCenter(find.byKey(ValueKey('automation-drop-$directory'))));
  await tester.pump();
  await gesture.up();
  await settle(tester);
}

void main() {
  testWidgets('moving the selected file refreshes its path and preserves its unchanged editor', (tester) async {
    final service = _Files();
    await _open(tester, service);
    final before = tester.widget<TextField>(find.byKey(const ValueKey('automation-editor'))).controller!.text;
    await dragFile(tester, 'playbook.yml', 'roles');
    expect(service.moves.single.$1, 'playbook.yml');
    expect(service.moves.single.$2, 'roles');
    expect(find.text('roles/playbook.yml'), findsOneWidget);
    expect(tester.widget<TextField>(find.byKey(const ValueKey('automation-editor'))).controller!.text, before);
    expect(tester.takeException(), isNull);
  });
  testWidgets('cancelling dirty-file confirmation prevents drag move and preserves edits', (tester) async {
    final service = _Files();
    await _open(tester, service);
    await enterKey(tester, 'automation-editor', '---\n# unsaved\n');
    await dragFile(tester, 'playbook.yml', 'roles');
    await tester.tap(find.text('Cancel').last);
    await settle(tester);
    expect(service.moves, isEmpty);
    expect(
      tester.widget<TextField>(find.byKey(const ValueKey('automation-editor'))).controller!.text,
      contains('unsaved'),
    );
    expect(tester.takeException(), isNull);
  });
  testWidgets('binary file moves use the snapshot SHA without reading it as plaintext', (tester) async {
    final service = _Files();
    await _open(tester, service);
    await dragFile(tester, 'blob.bin', 'files');
    expect(service.moves.single.$1, 'blob.bin');
    expect(service.reads, isNot(contains('blob.bin')));
    expect(service.contents, contains('files/blob.bin'));
    expect(tester.takeException(), isNull);
  });
  testWidgets('Library project search preserves selected editor and does not select another project', (tester) async {
    final service = _Files();
    await _open(tester, service);
    const another = AutomationProject(
      id: '22222222-2222-4222-8222-222222222222',
      name: 'Edge deployment',
      readOnly: false,
      revision: 1,
    );
    service.projectsCreated.add(another);
    final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    container.invalidate(automationProjectsProvider);
    await settle(tester);
    final before = tester.widget<TextField>(find.byKey(const ValueKey('automation-editor'))).controller!.text;
    final reads = service.reads.length;
    expect(find.byKey(const ValueKey('automation-library-search-input')), findsNothing);
    await tapKey(tester, 'automation-project-picker-toggle');
    await enterKey(tester, 'automation-library-search-input', 'Edge');
    expect(find.byKey(ValueKey('automation-project-${service.projectsCreated.first.id}')), findsNothing);
    expect(find.byKey(ValueKey('automation-project-${another.id}')), findsOneWidget);
    expect(tester.widget<TextField>(find.byKey(const ValueKey('automation-editor'))).controller!.text, before);
    expect(service.reads.length, reads);
    expect(service.moves, isEmpty);
    expect(tester.takeException(), isNull);
  });
  testWidgets('file context menu renames the open file and confirms deletion', (tester) async {
    final service = _Files();
    await _open(tester, service);
    Future<void> openMenu(String path) async {
      await tester.tap(find.byKey(ValueKey('automation-file-$path')), buttons: kSecondaryMouseButton);
      await settle(tester);
    }

    await openMenu('playbook.yml');
    await tester.tap(find.text('Rename').last);
    await settle(tester);
    await tester.enterText(find.byType(TextField).last, 'updated.yml');
    await tester.tap(find.text('Save').last);
    await settle(tester);
    expect(service.contents, contains('updated.yml'));
    expect(service.contents, isNot(contains('playbook.yml')));
    await openMenu('updated.yml');
    await tester.tap(find.text('Delete').last);
    await settle(tester);
    expect(service.contents, contains('updated.yml'));
    await tester.tap(find.text('Delete').last);
    await settle(tester);
    expect(service.contents, isNot(contains('updated.yml')));
    expect(tester.takeException(), isNull);
  });
  testWidgets('the project editor uses the saved SFTP default application', (tester) async {
    final service = _Files();
    await _open(tester, service);
    final ref = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    final settings = ref.read(settingsServiceProvider);
    await settings.updateLocal(
      settings.currentLocal.copyWith(sftpDefaultEditor: const AppRef(AppRefKind.bundleId, 'com.example.Editor')),
    );
    await settle(tester);
    await tapKey(tester, 'automation-open-editor');
    expect(service.openedEditor, {'kind': 'bundle_id', 'value': 'com.example.Editor'});
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));
  testWidgets('right-clicking a folder renames it and keeps the open child in the editor', (tester) async {
    final service = _Files()..contents['roles/main.yml'] = '# nested fixture';
    await _open(tester, service);
    await tapKey(tester, 'automation-file-roles');
    await tapKey(tester, 'automation-file-roles/main.yml');
    await tester.tap(find.byKey(const ValueKey('automation-file-roles')), buttons: kSecondaryMouseButton);
    await settle(tester);
    await tester.tap(find.text('Rename'));
    await settle(tester);
    await tester.enterText(find.byType(TextField).last, 'renamed');
    await tester.tap(find.text('Save').last);
    await settle(tester);
    expect(service.contents['renamed/main.yml'], '# nested fixture');
    expect(find.text('renamed/main.yml'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });
  testWidgets('the selected project menu renames and removes the catalog entry after confirmation', (tester) async {
    final service = _Files();
    await _open(tester, service);
    Future<void> openMenu() async {
      await tester.tap(find.byKey(const ValueKey('automation-project-picker-toggle')), buttons: kSecondaryMouseButton);
      await settle(tester);
    }

    await openMenu();
    await tester.tap(find.text('Rename'));
    await settle(tester);
    await tester.enterText(find.byType(TextField).last, 'Renamed project');
    await tester.tap(find.text('Save').last);
    await settle(tester);
    expect(service.projectsCreated.single.name, 'Renamed project');
    await openMenu();
    await tester.tap(find.text('Remove from library'));
    await settle(tester);
    expect(service.projectsCreated, hasLength(1));
    await tester.tap(find.text('Remove from library').last);
    await settle(tester);
    expect(service.projectsCreated, isEmpty);
    expect(service.contents, contains('playbook.yml'));
    expect(tester.takeException(), isNull);
  });
  testWidgets('library role opens the exact tasks file and existing AI review without generation', (tester) async {
    final service = _AiFiles();
    await _open(tester, service);
    tester
        .widget<GlassSelect<AutomationLibraryFilesView>>(find.byKey(const ValueKey('automation-library-file-kind')))
        .onChanged!(AutomationLibraryFilesView.roles);
    await settle(tester);
    await tapKey(tester, 'automation-library-item-roles/web/tasks/main.yml');
    expect(
      tester.widget<TextField>(find.byKey(const ValueKey('automation-editor'))).controller!.text,
      contains('Role task'),
    );
    await tapKey(tester, 'automation-ai-open');
    expect(find.byKey(const ValueKey('automation-ai-task')), findsOneWidget);
    expect(service.calls, 0);
    expect(service.moves, isEmpty);
    expect(service.vaultRequests, isEmpty);
    expect(tester.takeException(), isNull);
  });
  testWidgets('selected YAML Vault action refreshes the editor after the typed transform', (tester) async {
    final service = _Files();
    await _open(tester, service);
    await tapKey(tester, 'automation-vault-file-open');
    final password = find.descendant(
      of: find.byKey(const ValueKey('automation-vault-file-password')),
      matching: find.byType(TextFormField),
    );
    await tester.enterText(password, 'fixture-password');
    await settle(tester);
    await tapKey(tester, 'automation-vault-file-confirm');
    expect(service.vaultRequests.length, 1);
    expect(jsonEncode(service.vaultRequests), isNot(contains('fixture-password')));
    expect(
      tester.widget<TextField>(find.byKey(const ValueKey('automation-editor'))).controller!.text,
      startsWith(r'$ANSIBLE_VAULT;'),
    );
    expect(tester.takeException(), isNull);
  });
}
