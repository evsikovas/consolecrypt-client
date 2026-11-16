import 'dart:convert';

import 'package:consolecrypt/app/critical_notice_providers.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/automation/automation_portable_import_dialog.dart';
import 'package:consolecrypt/automation/automation_portable_import_session.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/runtime_notice_adapter.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/portable_import_fixture.dart' as f;
import '../helpers/test_app.dart';
import '../widgets/automation_portable_host_test.dart' show Gateway;
import 'automation_portable_menu_test.dart' as old;

class ImportProjects extends old.PortableProjects implements AutomationPortableImportReviewCapability {
  ImportProjects(super.gateway);
  final imports = f.Gateway();
  int reads = 0;
  @override
  Future<AutomationProjectCatalog> projects() {
    reads++;
    return super.projects();
  }

  @override
  PortableImportReviewGateway createPortableImportReviewGateway() => imports;
}

Future<(ProviderContainer, ImportProjects, f.Files, RuntimeNoticeEventAdapter)> open(WidgetTester tester) async {
  final backend = testBackend(seed: false);
  addTearDown(backend.dispose);
  await backend.debugCreateUnlockedLocalProfile(name: 'Owned import menus');
  final events = RuntimeNoticeEventAdapter();
  addTearDown(events.dispose);
  final service = ImportProjects(Gateway(backend.services.profiles.currentProfiles.active!.id.value));
  final files = f.Files();
  await pumpApp(
    tester,
    backend,
    size: const Size(1500, 1000),
    overrides: [
      automationServiceProvider.overrideWithValue(service),
      criticalRuntimeEventsProvider.overrideWithValue(events),
      fileDialogServiceProvider.overrideWithValue(files),
    ],
  );
  final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
  container.read(routerProvider).go(AppRoutes.automation);
  await settle(tester);
  await tapKey(tester, 'automation-project-${service.project.id}');
  await tapKey(tester, 'automation-file-play.yml');
  return (container, service, files, events);
}

Future<void> menu(WidgetTester tester) async {
  await tapKey(tester, 'automation-project-actions');
  await tapKey(tester, 'automation-portable-import-open');
}

Future<f.Registration> review(WidgetTester tester, ImportProjects service) async {
  await tapKey(tester, 'portable-import-choose');
  await enterKey(tester, 'portable-import-name', 'New explicit import');
  await enterKey(tester, 'portable-import-password', 'SYNTHETIC_PASSWORD');
  await tapKey(tester, 'portable-import-prepare');
  final op = service.imports.calls.single;
  op.acknowledge();
  await tester.pump();
  op.session.previewResult.complete(jsonEncode(f.readyJson(op.intent)));
  await settle(tester);
  return op;
}

void main() {
  testWidgets('menu/open captures metadata only and preserves dirty editor without register or picker', (tester) async {
    final (_, service, files, _) = await open(tester);
    const draft = '---\n- hosts: all\n# UNSAVED_IMPORT_BUFFER\n';
    await enterKey(tester, 'automation-editor', draft);
    await menu(tester);
    expect(find.byType(AutomationPortableImportDialog), findsOneWidget);
    expect(service.imports.calls, isEmpty);
    expect(files.opens, 0);
    expect(service.saves, 0);
    expect(service.gateway.captures, 1);
    expect(tester.widget<TextField>(find.byKey(const ValueKey('automation-editor'))).controller!.text, draft);
  });
  testWidgets('only installed receipt plus true joined refreshes list; no project selection, save or run', (
    tester,
  ) async {
    final (_, service, _, _) = await open(tester);
    await menu(tester);
    final op = await review(tester, service);
    final ready = PortableImportReady.fromJson(f.readyJson(op.intent), intent: op.intent);
    final reads = service.reads;
    await tapKey(tester, 'portable-import-accept');
    op.session.acceptResult.complete(jsonEncode(f.installedJson(ready)));
    await tester.pump();
    expect(service.reads, reads);
    op.retire();
    await settle(tester);
    await tapKey(tester, 'portable-import-close');
    await settle(tester);
    expect(service.reads, reads + 1);
    expect(service.saves, 0);
    expect(op.session.accepts, 1);
    expect(find.byKey(const ValueKey('automation-project-${f.projectId}')), findsNothing);
    expect(find.byKey(const ValueKey('automation-editor')), findsOneWidget);
  });
  testWidgets('Terminal hide retires original review and returning cannot accept it', (tester) async {
    final (container, service, _, _) = await open(tester);
    await menu(tester);
    final op = await review(tester, service);
    container.read(routerProvider).go(AppRoutes.terminal);
    await settle(tester);
    expect(op.cancels, greaterThanOrEqualTo(1));
    expect(op.session.accepts, 0);
    op.retire();
    container.read(routerProvider).go(AppRoutes.automation);
    await settle(tester);
    expect(find.byType(AutomationPortableImportDialog), findsNothing);
    expect(service.imports.calls.length, 1);
  });
  testWidgets('global original epoch loss retires Ready and blocks acceptance', (tester) async {
    final (_, service, _, events) = await open(tester);
    await menu(tester);
    final op = await review(tester, service);
    events.boundary();
    await settle(tester);
    expect(op.cancels, greaterThanOrEqualTo(1));
    expect(op.session.accepts, 0);
    expect(find.byType(AutomationPortableImportDialog), findsNothing);
    op.retire();
  });
}
