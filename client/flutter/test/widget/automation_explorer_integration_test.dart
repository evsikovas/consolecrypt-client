import 'dart:convert';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:crypto/crypto.dart' as crypto;
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';
import 'automation_exporter_template_test.dart' show Projects;

/// Synthetic file service: these assertions cover the real workspace dispatch,
/// selected editor and CAS arguments, not filesystem moves or Vault encryption.
class _Files extends Projects implements AutomationMoveService, AutomationVaultFileService {
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
  String hash(String text) => crypto.sha256.convert(utf8.encode(text)).toString();
  @override
  Future<AutomationProjectRevision> files(String projectId) async => AutomationProjectRevision(
    digest: hash(jsonEncode(contents)),
    bytes: 100,
    entries: [
      const AutomationFileEntry(path: 'roles', directory: true),
      const AutomationFileEntry(path: 'files', directory: true),
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
