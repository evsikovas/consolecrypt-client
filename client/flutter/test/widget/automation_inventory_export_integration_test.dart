import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/automation/automation_inventory_export.dart';
import 'package:consolecrypt/automation/automation_inventory_export_save.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/file_dialog_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/inventory_export_fixture.dart';
import '../helpers/test_app.dart';

const _project = AutomationProject(
  id: inventoryExportProjectId,
  name: 'Export fixture project',
  readOnly: false,
  revision: 7,
);
final _host = Host(
  id: const ObjectId(inventoryExportHostId),
  name: 'Saved SSH fixture',
  address: '192.0.2.10',
  createdAt: DateTime.utc(2026),
  updatedAt: DateTime.utc(2026),
);

class _Projects extends Fake implements AutomationService {
  final saves = <String>[];
  @override
  Future<AutomationProjectCatalog> projects() async =>
      const AutomationProjectCatalog(
        version: 1,
        readOnly: false,
        deviceLocal: true,
        projects: [_project],
      );
  @override
  Future<AutomationProjectRevision> files(String projectId) async =>
      const AutomationProjectRevision(
        digest: 'files',
        bytes: 40,
        entries: [
          AutomationFileEntry(path: 'playbook.yml', directory: false),
          AutomationFileEntry(path: 'other.yml', directory: false),
        ],
      );
  @override
  Future<AutomationProjectFile> readFile(String projectId, String path) async =>
      AutomationProjectFile(
        path: path,
        text: '---\n# $path\n',
        sha256: path,
        lineEndings: 'lf',
      );
  @override
  Future<AutomationProjectFile> saveFile(
    String projectId, {
    required String path,
    required String text,
    String? expectedSha256,
  }) async {
    saves.add(text);
    return AutomationProjectFile(
      path: path,
      text: text,
      sha256: 'saved',
      lineEndings: 'lf',
    );
  }
}

class _Exports extends _Projects implements AutomationInventoryExportService {
  final requests = <InventoryExportRequest>[];
  Completer<String>? pending;
  @override
  Future<InventoryExportContext> inventoryExportContext(String id) async =>
      InventoryExportContext.fromConfiguration(
        id,
        inventoryExportConfiguration(selected: [inventoryExportHostId]),
      );
  @override
  Future<IssuedInventorySnapshot> issueInventoryExport(
    InventoryExportRequest request,
  ) async {
    requests.add(request);
    final text =
        await (pending?.future ??
            Future.value(
              '\n${jsonEncode(inventoryExportSnapshot(hostIds: request.hostIds, groups: request.groupNames))}\n',
            ));
    return IssuedInventorySnapshot.fromNative(text, request);
  }
}

class _Files extends Fake implements FileDialogService {
  int choices = 0, finishes = 0;
  @override
  Future<String?> chooseSaveFile({
    required String suggestedName,
    List<String> extensions = const [],
  }) async {
    choices++;
    return 'C:/fixture/explicit-inventory.consolecrypt.json';
  }

  @override
  Future<bool> finishSaveFile(String path) async {
    finishes++;
    return true;
  }
}

class _Writer implements InventoryExportFileWriter {
  final bytes = <Uint8List>[];
  @override
  Future<void> write(String path, Uint8List data) async {
    bytes.add(Uint8List.fromList(data));
  }

  @override
  Future<void> discardMobileStaging(String path) async {}
}

Future<ProviderContainer> _open(
  WidgetTester tester,
  _Projects service,
  _Files files,
  _Writer writer,
) async {
  final backend = testBackend();
  addTearDown(backend.dispose);
  await backend.debugCreateUnlockedLocalProfile(name: 'Export menu fixture');
  await pumpApp(
    tester,
    backend,
    overrides: [
      automationServiceProvider.overrideWithValue(service),
      hostsProvider.overrideWith((ref) => Stream.value([_host])),
      fileDialogServiceProvider.overrideWithValue(files),
      inventoryExportFileWriterProvider.overrideWithValue(writer),
    ],
  );
  final container = ProviderScope.containerOf(
    tester.element(find.byType(Navigator).first),
  );
  container.read(routerProvider).go(AppRoutes.automation);
  await settle(tester);
  await tapKey(tester, 'automation-project-$inventoryExportProjectId');
  await tapKey(tester, 'automation-file-playbook.yml');
  return container;
}

Future<void> _export(WidgetTester tester) async {
  await tapKey(tester, 'automation-project-menu');
  await tapKey(tester, 'automation-inventory-export-open');
}

void main() {
  testWidgets(
    'project menu hides export when service has no Native capability',
    (tester) async {
      await _open(tester, _Projects(), _Files(), _Writer());
      await tapKey(tester, 'automation-project-menu');
      expect(
        find.byKey(const ValueKey('automation-inventory-export-open')),
        findsNothing,
      );
      await tester.pumpWidget(const SizedBox());
    },
  );

  testWidgets(
    'project menu export preserves unsaved editor and writes only the explicit Native selection',
    (tester) async {
      final service = _Exports(), files = _Files(), writer = _Writer();
      await _open(tester, service, files, writer);
      const unsaved = '---\n# unsaved editor fixture\n';
      await enterKey(tester, 'automation-editor', unsaved);
      await _export(tester);
      expect(service.requests, isEmpty);
      expect(files.choices, 0);
      await tapKey(tester, 'inventory-export-group-web');
      await tapKey(tester, 'inventory-export-host-$inventoryExportHostId');
      await tapKey(tester, 'inventory-export-save');
      expect(service.requests.single.expectedRevision, 7);
      expect(service.requests.single.projectId, inventoryExportProjectId);
      expect(service.requests.single.hostIds, [inventoryExportHostId]);
      expect(service.requests.single.groupNames, ['web']);
      expect(
        utf8.decode(writer.bytes.single),
        '\n${jsonEncode(inventoryExportSnapshot())}\n',
      );
      expect(files.finishes, 1);
      expect(service.saves, isEmpty);
      expect(
        tester
            .widget<TextField>(find.byKey(const ValueKey('automation-editor')))
            .controller!
            .text,
        unsaved,
      );
      await tester.pumpWidget(const SizedBox());
    },
  );

  testWidgets('project menu export cannot save a response after profile lock', (
    tester,
  ) async {
    final service = _Exports()..pending = Completer<String>(),
        files = _Files(),
        writer = _Writer();
    final container = await _open(tester, service, files, writer);
    await _export(tester);
    await tapKey(tester, 'inventory-export-host-$inventoryExportHostId');
    await tapKey(tester, 'inventory-export-save');
    expect(service.requests, hasLength(1));
    await container.read(vaultServiceProvider).lock();
    await settle(tester);
    service.pending!.complete(
      jsonEncode(inventoryExportSnapshot(groups: const [])),
    );
    await settle(tester);
    expect(writer.bytes, isEmpty);
    expect(files.finishes, 0);
    expect(service.saves, isEmpty);
    expect(
      find.byKey(const ValueKey('automation-inventory-export-dialog')),
      findsNothing,
    );
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets(
    'parent file request change invalidates export even within the same project revision',
    (tester) async {
      final service = _Exports()..pending = Completer<String>(),
          files = _Files(),
          writer = _Writer();
      await _open(tester, service, files, writer);
      await _export(tester);
      await tapKey(tester, 'inventory-export-host-$inventoryExportHostId');
      await tapKey(tester, 'inventory-export-save');
      // Simulates a parent workspace change while Native completion is pending;
      // the modal itself never grants access to the obscured editor controls.
      tester
          .widget<ListTile>(
            find.byKey(const ValueKey('automation-file-other.yml')),
          )
          .onTap!();
      await settle(tester);
      service.pending!.complete(
        jsonEncode(inventoryExportSnapshot(groups: const [])),
      );
      await settle(tester);
      expect(writer.bytes, isEmpty);
      expect(files.finishes, 0);
      expect(
        find.byKey(const ValueKey('automation-inventory-export-dialog')),
        findsNothing,
      );
      expect(
        tester
            .widget<TextField>(find.byKey(const ValueKey('automation-editor')))
            .controller!
            .text,
        contains('other.yml'),
      );
      await tester.pumpWidget(const SizedBox());
    },
  );
}
