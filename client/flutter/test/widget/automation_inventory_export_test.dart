import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/automation/automation_inventory_export.dart';
import 'package:consolecrypt/automation/automation_inventory_export_dialog.dart';
import 'package:consolecrypt/automation/automation_inventory_export_save.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/file_dialog_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_ui_proof.dart';
import '../helpers/inventory_export_fixture.dart';
import '../helpers/test_app.dart' show settle, tapKey;

const exportScope = AutomationScope('export-profile', true);
const exportProject = AutomationProject(
  id: inventoryExportProjectId,
  name: 'Fixture inventory',
  readOnly: false,
  revision: 7,
);
final exportHost = Host(
  id: const ObjectId(inventoryExportHostId),
  name: 'Selected saved host',
  address: '192.0.2.10',
  createdAt: DateTime.utc(2026),
  updatedAt: DateTime.utc(2026),
);
final outsideHost = Host(
  id: const ObjectId(inventoryExportOtherId),
  name: 'Outside host',
  address: '192.0.2.20',
  createdAt: DateTime.utc(2026),
  updatedAt: DateTime.utc(2026),
);

class _Exports implements AutomationInventoryExportService {
  Map<String, dynamic> configuration = inventoryExportConfiguration(selected: [inventoryExportHostId]);
  final requests = <InventoryExportRequest>[];
  Completer<String>? pending;
  InventoryExportFailure? failure;
  String? nativeText;
  @override
  Future<InventoryExportContext> inventoryExportContext(String projectId) async =>
      InventoryExportContext.fromConfiguration(projectId, configuration);
  @override
  Future<IssuedInventorySnapshot> issueInventoryExport(InventoryExportRequest request) async {
    requests.add(request);
    if (failure != null) throw failure!;
    final raw =
        await (pending?.future ??
            Future.value(
              nativeText ??
                  '\n${jsonEncode(inventoryExportSnapshot(hostIds: request.hostIds, groups: request.groupNames))}\n',
            ));
    return IssuedInventorySnapshot.fromNative(raw, request);
  }
}

class _Files extends Fake implements FileDialogService {
  int choices = 0, finishes = 0;
  String? path = 'C:/fixture/user-chosen.consolecrypt.json';
  Completer<String?>? pending;
  @override
  Future<String?> chooseSaveFile({required String suggestedName, List<String> extensions = const []}) async {
    choices++;
    expect(suggestedName, 'inventory.consolecrypt.json');
    expect(extensions, ['json']);
    return pending?.future ?? path;
  }

  @override
  Future<bool> finishSaveFile(String path) async {
    finishes++;
    return true;
  }
}

class _Writer implements InventoryExportFileWriter {
  final writes = <(String, Uint8List)>[];
  final discarded = <String>[];
  @override
  Future<void> write(String path, Uint8List bytes) async {
    writes.add((path, Uint8List.fromList(bytes)));
  }

  @override
  Future<void> discardMobileStaging(String path) async {
    discarded.add(path);
  }
}

Future<ProviderContainer> _open(
  WidgetTester tester,
  _Exports service,
  _Files files,
  _Writer writer, {
  ValueNotifier<bool>? projectCurrent,
  Stream<List<Host>>? hosts,
  Size size = const Size(1300, 1000),
  Locale locale = const Locale('en'),
}) async {
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await loadAutomationProofFonts(tester);
  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        automationScopeProvider.overrideWithValue(exportScope),
        hostsProvider.overrideWith((ref) => hosts ?? Stream.value([exportHost, outsideHost])),
        fileDialogServiceProvider.overrideWithValue(files),
        inventoryExportFileWriterProvider.overrideWithValue(writer),
      ],
      child: MaterialApp(
        theme: AppTheme.dark(platform: TargetPlatform.windows),
        locale: locale,
        supportedLocales: AppLocalizations.supportedLocales,
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        home: Scaffold(
          body: RepaintBoundary(
            key: const ValueKey('export-fixture-body'),
            child: Builder(
              builder: (context) => TextButton(
                key: const ValueKey('open-export'),
                onPressed: () => showGlassDialog<bool>(
                  context,
                  builder: (_) => RepaintBoundary(
                    key: const ValueKey('automation-proof-root'),
                    child: AutomationInventoryExportDialog(
                      scope: exportScope,
                      project: exportProject,
                      service: service,
                      isProjectCurrent: () => projectCurrent?.value ?? true,
                    ),
                  ),
                ),
                child: const Text('Fixture'),
              ),
            ),
          ),
        ),
      ),
    ),
  );
  final container = ProviderScope.containerOf(tester.element(find.byKey(const ValueKey('open-export'))));
  await tapKey(tester, 'open-export');
  return container;
}

Future<void> selectHost(WidgetTester tester) => tapKey(tester, 'inventory-export-host-$inventoryExportHostId');
Future<void> close(WidgetTester tester) async {
  await tester.pumpWidget(const SizedBox());
  await tester.pump();
}

bool saveEnabled(WidgetTester tester) =>
    tester.widget<GlassButton>(find.byKey(const ValueKey('inventory-export-save'))).onPressed != null;

void _testDesktop(String name, Future<void> Function(WidgetTester) test) =>
    testWidgets(name, test, variant: const TargetPlatformVariant({TargetPlatform.windows}));
void main() {
  _testDesktop(
    'export requires explicit saved hosts; group selection never expands host IDs and saves exact Native bytes',
    (tester) async {
      final service = _Exports(), files = _Files(), writer = _Writer();
      await _open(tester, service, files, writer);
      expect(saveEnabled(tester), isFalse);
      expect(service.requests, isEmpty);
      expect(find.byKey(const ValueKey('inventory-export-host-$inventoryExportOtherId')), findsNothing);
      await tapKey(tester, 'inventory-export-group-web');
      expect(saveEnabled(tester), isFalse);
      await selectHost(tester);
      expect(saveEnabled(tester), isTrue);
      await tapKey(tester, 'inventory-export-save');
      expect(service.requests.single.toJson(), {
        'version': 1,
        'projectId': inventoryExportProjectId,
        'expectedRevision': 7,
        'hostIds': [inventoryExportHostId],
        'groupNames': ['web'],
      });
      expect(files.choices, 1);
      expect(files.finishes, 1);
      expect(writer.writes.single.$1, files.path);
      expect(utf8.decode(writer.writes.single.$2), '\n${jsonEncode(inventoryExportSnapshot())}\n');
      expect(writer.discarded, isEmpty);
      await close(tester);
    },
  );

  _testDesktop('cancelled destination issues nothing and performs no write', (tester) async {
    final service = _Exports(), files = _Files()..path = null, writer = _Writer();
    await _open(tester, service, files, writer);
    await selectHost(tester);
    await tapKey(tester, 'inventory-export-save');
    expect(service.requests, isEmpty);
    expect(writer.writes, isEmpty);
    expect(files.finishes, 0);
    await close(tester);
  });

  _testDesktop('Native unsupported route shows localized help without direct fallback or replacement data', (
    tester,
  ) async {
    final service = _Exports()..failure = const InventoryExportFailure(InventoryExportReason.unsupportedRoute);
    final files = _Files(), writer = _Writer();
    await _open(tester, service, files, writer);
    await selectHost(tester);
    await tapKey(tester, 'inventory-export-save');
    expect(find.textContaining('jump host'), findsOneWidget);
    expect(service.requests, hasLength(1));
    expect(writer.writes, isEmpty);
    expect(files.finishes, 0);
    await close(tester);
  });

  _testDesktop('lock while Native issuance is pending discards its response without saving', (tester) async {
    final service = _Exports()..pending = Completer<String>(), files = _Files(), writer = _Writer();
    final container = await _open(tester, service, files, writer);
    await selectHost(tester);
    await tapKey(tester, 'inventory-export-save');
    expect(service.requests, hasLength(1));
    container.updateOverrides([
      automationScopeProvider.overrideWithValue(const AutomationScope('export-profile', false)),
      hostsProvider.overrideWith((_) => Stream.value([exportHost, outsideHost])),
      fileDialogServiceProvider.overrideWithValue(files),
      inventoryExportFileWriterProvider.overrideWithValue(writer),
    ]);
    await settle(tester);
    service.pending!.complete(jsonEncode(inventoryExportSnapshot(groups: const [])));
    await settle(tester);
    expect(writer.writes, isEmpty);
    expect(files.finishes, 0);
    expect(find.byKey(const ValueKey('automation-inventory-export-dialog')), findsNothing);
    await close(tester);
  });

  _testDesktop('project change during destination picker prevents Native issuance', (tester) async {
    final valid = ValueNotifier(true);
    addTearDown(valid.dispose);
    final service = _Exports(), files = _Files()..pending = Completer<String?>(), writer = _Writer();
    await _open(tester, service, files, writer, projectCurrent: valid);
    await selectHost(tester);
    await tapKey(tester, 'inventory-export-save');
    valid.value = false;
    await tester.pump(const Duration(milliseconds: 300));
    files.pending!.complete(files.path);
    await settle(tester);
    expect(service.requests, isEmpty);
    expect(writer.writes, isEmpty);
    await close(tester);
  });

  _testDesktop('saved host change invalidates pending issuance instead of saving stale endpoints', (tester) async {
    final updates = StreamController<List<Host>>();
    addTearDown(updates.close);
    final service = _Exports()..pending = Completer<String>(), files = _Files(), writer = _Writer();
    await _open(tester, service, files, writer, hosts: updates.stream);
    updates.add([exportHost, outsideHost]);
    await settle(tester);
    await selectHost(tester);
    await tapKey(tester, 'inventory-export-save');
    updates.add([outsideHost]);
    await settle(tester);
    service.pending!.complete(jsonEncode(inventoryExportSnapshot(groups: const [])));
    await settle(tester);
    expect(writer.writes, isEmpty);
    expect(files.finishes, 0);
    expect(find.textContaining('changed'), findsOneWidget);
    await close(tester);
  });

  _testDesktop('external-only inventory cannot issue or save a snapshot', (tester) async {
    final service = _Exports()..configuration = inventoryExportConfiguration(mode: 'external_only');
    final files = _Files(), writer = _Writer();
    await _open(tester, service, files, writer);
    expect(saveEnabled(tester), isFalse);
    expect(find.textContaining('external-only'), findsOneWidget);
    expect(service.requests, isEmpty);
    expect(files.choices, 0);
    expect(writer.writes, isEmpty);
    await close(tester);
  });

  testWidgets('mobile export finishes the trusted document picker and removes only its private staging file', (
    tester,
  ) async {
    final service = _Exports(), files = _Files(), writer = _Writer();
    await _open(tester, service, files, writer);
    await selectHost(tester);
    await tapKey(tester, 'inventory-export-save');
    expect(service.requests, hasLength(1));
    expect(writer.writes, hasLength(1));
    expect(files.finishes, 1);
    expect(writer.discarded, [files.path]);
    await close(tester);
  }, variant: const TargetPlatformVariant({TargetPlatform.android}));

  for (final size in [const Size(1300, 1000), const Size(440, 720)]) {
    _testDesktop('bounded export selection has no overlapping lists at ${size.width}', (tester) async {
      final service = _Exports(), files = _Files(), writer = _Writer();
      await _open(tester, service, files, writer, size: size, locale: const Locale('ru'));
      await selectHost(tester);
      if (size.width < 700) {
        await captureAutomationWorkspace(tester, 'inventory-export-narrow-selection-ru');
      }
      final outer = find
          .ancestor(of: find.byKey(const ValueKey('inventory-export-host-list')), matching: find.byType(Scrollable))
          .first;
      tester.state<ScrollableState>(outer).position.jumpTo(0);
      await tester.pump();
      await captureAutomationWorkspace(
        tester,
        size.width > 700 ? 'inventory-export-wide-ru' : 'inventory-export-narrow-ru',
      );
      expect(tester.takeException(), isNull);
      final host = tester.getRect(find.byKey(const ValueKey('inventory-export-host-list')));
      final group = tester.getRect(find.byKey(const ValueKey('inventory-export-group-list')));
      expect(host.overlaps(group), isFalse);
      expect(saveEnabled(tester), isTrue);
      await close(tester);
    });
  }
}
