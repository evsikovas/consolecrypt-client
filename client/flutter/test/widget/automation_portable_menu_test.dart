import 'package:consolecrypt/app/critical_notice_providers.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/automation/automation_portable_dialog.dart';
import 'package:consolecrypt/automation/automation_portable_native_adapter.dart';
import 'package:consolecrypt/automation/automation_portable_service.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_screen.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/runtime_notice_adapter.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_ui_proof.dart';
import '../helpers/test_app.dart';
import '../unit/automation_portable_controller_test.dart' as f;
import '../widgets/automation_portable_host_test.dart' show Gateway;

class Projects extends Fake implements AutomationService {
  String text = '---\n- hosts: all\n';
  int saves = 0;
  final project = const AutomationProject(
    id: f.project,
    name: 'Owned portable project',
    readOnly: false,
    revision: 1,
    sourceKind: 'managed',
  );
  @override
  Future<AutomationProjectCatalog> projects() async =>
      AutomationProjectCatalog(version: 1, readOnly: false, deviceLocal: true, projects: [project]);
  @override
  Future<AutomationProjectRevision> files(String projectId) async => AutomationProjectRevision(
    digest: 'a' * 64,
    bytes: text.length,
    entries: const [AutomationFileEntry(path: 'play.yml', directory: false)],
  );
  @override
  Future<AutomationProjectFile> readFile(String projectId, String path) async =>
      AutomationProjectFile(path: path, text: text, sha256: 'b' * 64, lineEndings: 'lf');
  @override
  Future<AutomationProjectFile> saveFile(
    String projectId, {
    required String path,
    required String text,
    String? expectedSha256,
  }) async {
    saves++;
    this.text = text;
    return AutomationProjectFile(path: path, text: text, sha256: 'c' * 64, lineEndings: 'lf');
  }
}

class PortableProjects extends Projects implements AutomationPortableGatewayCapability {
  PortableProjects(this.gateway);
  final Gateway gateway;
  @override
  PortableNativeGateway createPortableGateway() => gateway;
}

Future<(ProviderContainer, Projects, Gateway, f.Files)> open(
  WidgetTester tester, {
  bool available = true,
  AppLocale locale = AppLocale.en,
  double width = 1600,
}) async {
  final backend = testBackend(seed: false);
  addTearDown(backend.dispose);
  await backend.debugCreateUnlockedLocalProfile(name: 'Owned portable menus');
  final hub = RuntimeNoticeEventAdapter(),
      gateway = Gateway(backend.services.profiles.currentProfiles.active!.id.value),
      files = f.Files();
  addTearDown(hub.dispose);
  final Projects service = available ? PortableProjects(gateway) : Projects();
  await loadAutomationProofFonts(tester);
  await backend.services.settings.updateLocal(
    backend.services.settings.currentLocal.copyWith(themeMode: AppThemeMode.dark),
  );
  await pumpApp(
    tester,
    backend,
    locale: locale,
    size: Size(width, 1100),
    captureBoundary: true,
    overrides: [
      automationServiceProvider.overrideWithValue(service),
      criticalRuntimeEventsProvider.overrideWithValue(hub),
      fileDialogServiceProvider.overrideWithValue(files),
    ],
  );
  final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
  container.read(routerProvider).go(AppRoutes.automation);
  await settle(tester);
  await tapKey(tester, 'automation-project-${f.project}');
  await tapKey(tester, 'automation-file-play.yml');
  return (container, service, gateway, files);
}

Future<void> exportMenu(WidgetTester tester) async {
  await tapKey(tester, 'automation-project-menu');
  await tapKey(tester, 'automation-portable-export-open');
}

void main() {
  testWidgets('Library export version opens saved snapshot review without writing or creating an archive', (
    tester,
  ) async {
    final (_, service, gateway, files) = await open(tester);
    expect(gateway.captures, 0);
    await tapKey(tester, 'automation-library-export-version');
    expect(find.byType(AutomationPortableDialog), findsOneWidget);
    expect(gateway.captures, 1);
    expect(gateway.calls.map((x) => x.kind), ['context']);
    expect(service.saves, 0);
    expect(files.saves, 0);
    expect(files.opens, 0);
    expect(tester.takeException(), isNull);
  });

  testWidgets('optional missing Native support exposes neither portable menu action and opens no scope', (
    tester,
  ) async {
    final (_, _, gateway, files) = await open(tester, available: false);
    await tapKey(tester, 'automation-project-actions');
    expect(find.byKey(const ValueKey('automation-portable-inspect-open')), findsNothing);
    await tester.tapAt(const Offset(1200, 700));
    await settle(tester);
    await tapKey(tester, 'automation-project-menu');
    expect(find.byKey(const ValueKey('automation-portable-export-open')), findsNothing);
    expect(gateway.captures, 0);
    expect(files.saves, 0);
    expect(files.opens, 0);
  });
  testWidgets('opening project menus alone does no portable capture or file operation', (tester) async {
    final (_, _, gateway, files) = await open(tester);
    await tapKey(tester, 'automation-project-actions');
    expect(find.byKey(const ValueKey('automation-portable-inspect-open')), findsOneWidget);
    expect(gateway.captures, 0);
    expect(gateway.calls, isEmpty);
    expect(files.opens, 0);
    await tester.tapAt(const Offset(1200, 700));
    await settle(tester);
    await tapKey(tester, 'automation-project-menu');
    expect(find.byKey(const ValueKey('automation-portable-export-open')), findsOneWidget);
    expect(gateway.captures, 0);
    expect(files.saves, 0);
  });
  testWidgets('dirty editor Cancel preserves draft and denies Native capture or save picker', (tester) async {
    final (_, service, gateway, files) = await open(tester);
    const draft = '---\n- hosts: all\n# owned unsaved draft\n';
    await enterKey(tester, 'automation-editor', draft);
    await exportMenu(tester);
    final l = tester.element(find.byType(AutomationScreen)).l10n;
    await tester.tap(find.text(l.commonCancel).last);
    await settle(tester);
    expect(tester.widget<TextField>(find.byKey(const ValueKey('automation-editor'))).controller!.text, draft);
    expect(service.saves, 0);
    expect(gateway.captures, 0);
    expect(files.saves, 0);
  });
  testWidgets('Use saved files preserves unsaved buffer and opens only readonly saved context', (tester) async {
    final (_, service, gateway, files) = await open(tester);
    const draft = '---\n- hosts: all\n# owned unsaved draft\n';
    await enterKey(tester, 'automation-editor', draft);
    await exportMenu(tester);
    final l = tester.element(find.byType(AutomationScreen)).l10n;
    await tester.tap(find.text(l.automationPortableUseSaved));
    await settle(tester);
    expect(find.byType(AutomationPortableDialog), findsOneWidget);
    expect(service.saves, 0);
    expect(gateway.captures, 1);
    expect(gateway.calls.map((x) => x.kind), ['context']);
    expect(tester.widget<TextField>(find.byKey(const ValueKey('automation-editor'))).controller!.text, draft);
    expect(files.saves, 0);
    expect(files.opens, 0);
  });
  testWidgets('Save choice saves current file before Native source capture and never auto-selects or exports', (
    tester,
  ) async {
    final (_, service, gateway, files) = await open(tester);
    const draft = '---\n- hosts: all\n# owned saved change\n';
    await enterKey(tester, 'automation-editor', draft);
    await exportMenu(tester);
    final l = tester.element(find.byType(AutomationScreen)).l10n;
    await tester.tap(find.text(l.commonSave).last);
    await settle(tester);
    expect(service.saves, 1);
    expect(service.text, draft);
    expect(gateway.captures, 1);
    expect(gateway.calls.map((x) => x.kind), ['context']);
    expect(files.saves, 0);
  });
  testWidgets('inspection action never runs dirty editor Save or changes active project', (tester) async {
    final (_, service, gateway, files) = await open(tester);
    const draft = '---\n- hosts: all\n# preserve on readonly inspection\n';
    await enterKey(tester, 'automation-editor', draft);
    await tapKey(tester, 'automation-project-actions');
    await tapKey(tester, 'automation-portable-inspect-open');
    expect(find.byType(AutomationPortableDialog), findsOneWidget);
    expect(gateway.captures, 1);
    expect(gateway.calls, isEmpty);
    expect(service.saves, 0);
    expect(files.opens, 0);
    expect(tester.widget<TextField>(find.byKey(const ValueKey('automation-editor'))).controller!.text, draft);
  });
  testWidgets('fine hub epoch retirement during dirty prompt cannot recapture latest unlock for saved export', (
    tester,
  ) async {
    final (container, service, gateway, files) = await open(tester);
    const draft = '---\n- hosts: all\n# retain original export intent\n';
    await enterKey(tester, 'automation-editor', draft);
    await exportMenu(tester);
    final l = tester.element(find.byType(AutomationScreen)).l10n;
    final originalScope = container.read(automationScopeProvider);
    (container.read(criticalRuntimeEventsProvider) as RuntimeNoticeEventAdapter).boundary();
    await tester.tap(find.text(l.automationPortableUseSaved));
    await settle(tester);
    expect(identical(container.read(automationScopeProvider), originalScope), isTrue);
    expect(gateway.captures, 0);
    expect(gateway.calls, isEmpty);
    expect(service.saves, 0);
    expect(files.saves, 0);
    expect(tester.widget<TextField>(find.byKey(const ValueKey('automation-editor'))).controller!.text, draft);
  });
  for (final width in [1400.0, 500.0]) {
    testWidgets('portable encrypted export real Screen RU at $width', (tester) async {
      if (width == 500) tester.platformDispatcher.textScaleFactorTestValue = 1.5;
      addTearDown(tester.platformDispatcher.clearTextScaleFactorTestValue);
      final (_, _, gateway, files) = await open(tester, locale: AppLocale.ru, width: width);
      await exportMenu(tester);
      expect(find.byType(AutomationPortableDialog), findsOneWidget);
      expect(gateway.calls.map((x) => x.kind), ['context']);
      expect(files.saves, 0);
      expect(tester.takeException(), isNull);
      await captureAutomationDialog(tester, 'portable-export-ru-${width.toInt()}');
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
  }
}
