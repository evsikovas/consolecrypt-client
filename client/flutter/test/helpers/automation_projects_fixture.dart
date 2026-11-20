import 'package:consolecrypt/automation/automation_service.dart';
import 'package:flutter_test/flutter_test.dart';

class Projects implements AutomationService {
  final projectsCreated = <AutomationProject>[];
  final contents = <String, String>{'playbook.yml': '---\n[]\n'};
  final expected = <String, String?>{};
  String? readShaOverride;
  @override
  Future<AutomationProjectCatalog> projects() async =>
      AutomationProjectCatalog(version: 1, readOnly: false, deviceLocal: true, projects: projectsCreated);
  @override
  Future<AutomationProject> createProject({required String name, String? directory}) async {
    expect(directory, isNull);
    final created = AutomationProject(
      id: '00000000-0000-4000-8000-000000000001',
      name: name,
      readOnly: false,
      sourceKind: 'managed',
      revision: 1,
    );
    projectsCreated.add(created);
    return created;
  }

  @override
  Future<AutomationProjectRevision> files(String projectId) async => AutomationProjectRevision(
    digest: 'a' * 64,
    bytes: 100,
    entries: [for (final path in contents.keys) AutomationFileEntry(path: path, directory: false)],
  );
  @override
  Future<AutomationProjectFile> readFile(String projectId, String path) async =>
      AutomationProjectFile(path: path, text: contents[path]!, sha256: readShaOverride ?? 'b' * 64, lineEndings: 'lf');
  @override
  Future<AutomationProjectFile> saveFile(
    String projectId, {
    required String path,
    required String text,
    String? expectedSha256,
  }) async {
    expected[path] = expectedSha256;
    contents[path] = text;
    return AutomationProjectFile(path: path, text: text, sha256: 'c' * 64, lineEndings: 'lf');
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => throw StateError('no execution or credential capability');
}
