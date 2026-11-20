import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/automation/automation_dependency_dialog.dart';
import 'package:consolecrypt/automation/automation_dependency_models.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_dependency_fixture.dart' as f;
import '../helpers/automation_ui_proof.dart';
import '../helpers/test_app.dart';

class Projects implements AutomationService, AutomationDependencyService {
  final native = f.Service();
  int saves = 0;
  String text = '---\n# owned original document\n', sha = 'a' * 64;
  static const row = AutomationProject(
    id: f.project,
    name: 'Owned dependency project',
    readOnly: false,
    sourceKind: 'managed',
    revision: 1,
  );
  @override
  Future<String> dependencyAction(String raw) => native.dependencyAction(raw);
  @override
  Future<AutomationProjectCatalog> projects() async =>
      const AutomationProjectCatalog(
        version: 1,
        readOnly: false,
        deviceLocal: true,
        projects: [row],
      );
  @override
  Future<AutomationProject> createProject({
    required String name,
    String? directory,
  }) async => row;
  @override
  Future<AutomationProject> renameProject(
    String id, {
    required int revision,
    required String name,
  }) async => row;
  @override
  Future<AutomationProjectRevision> files(String id) async =>
      AutomationProjectRevision(
        digest: sha,
        bytes: text.length,
        entries: const [
          AutomationFileEntry(
            path: 'playbook.yml',
            directory: false,
            bytes: 32,
          ),
        ],
      );
  @override
  Future<AutomationProjectFile> readFile(String id, String path) async =>
      AutomationProjectFile(
        path: path,
        text: text,
        sha256: sha,
        lineEndings: 'lf',
      );
  @override
  Future<AutomationProjectFile> saveFile(
    String id, {
    required String path,
    required String text,
    String? expectedSha256,
  }) async {
    expect(expectedSha256, sha);
    saves++;
    this.text = text;
    sha = 'b' * 64;
    return readFile(id, path);
  }
}

Future<ProviderContainer> open(WidgetTester tester, Projects service) async {
  await loadAutomationProofFonts(tester);
  final backend = testBackend(seed: false);
  addTearDown(backend.dispose);
  await backend.debugCreateUnlockedLocalProfile(
    name: 'Owned installer workspace',
  );
  final profile = backend.services.profiles.currentProfiles.active!;
  final scope = AutomationScope(profile.id, true);
  await pumpApp(
    tester,
    backend,
    size: const Size(1500, 1100),
    overrides: [
      automationServiceProvider.overrideWithValue(service),
      automationRunnersProvider.overrideWith(
        (_) async => const AutomationRunnerCatalog(
          readOnly: false,
          runners: [
            AutomationRunnerProfile(
              id: f.runner,
              name: 'Owned Runner',
              readOnly: false,
              revision: 1,
            ),
          ],
        ),
      ),
      automationScopeProvider.overrideWithValue(scope),
      activeProfileProvider.overrideWithValue(profile),
      appStageProvider.overrideWithValue(AppStage.unlocked),
    ],
  );
  final container = ProviderScope.containerOf(
    tester.element(find.byType(Navigator).first),
  );
  container.read(routerProvider).go(AppRoutes.automation);
  await settle(tester);
  await tapKey(tester, 'automation-project-${f.project}');
  return container;
}

Future<void> dirty(WidgetTester tester) async {
  await tapKey(tester, 'automation-file-playbook.yml');
  await tester.enterText(
    find.byKey(const ValueKey('automation-editor')),
    '---\n# explicit unsaved draft\n',
  );
  await settle(tester);
}

Future<void> menu(WidgetTester tester) async {
  await tapKey(tester, 'automation-settings-menu');
  await tapKey(tester, 'automation-dependencies-open');
}

Future<void> finish(WidgetTester tester) async {
  await tester.pumpWidget(const SizedBox.shrink());
  await tester.pump();
}

void replace(
  ProviderContainer container, {
  AutomationService? automation,
  AppServices? services,
}) {
  final scope = container.read(automationScopeProvider),
      profile = container.read(activeProfileProvider);
  container.updateOverrides([
    appServicesProvider.overrideWithValue(
      services ?? container.read(appServicesProvider),
    ),
    automationServiceProvider.overrideWithValue(
      automation ?? container.read(automationServiceProvider),
    ),
    automationRunnersProvider.overrideWith(
      (_) async => const AutomationRunnerCatalog(readOnly: false, runners: []),
    ),
    automationScopeProvider.overrideWithValue(scope),
    activeProfileProvider.overrideWithValue(profile),
    appStageProvider.overrideWithValue(AppStage.unlocked),
  ]);
}

AppServices wrapper(AppServices s) => AppServices(
  profiles: s.profiles,
  auth: s.auth,
  vault: s.vault,
  inventory: s.inventory,
  terminal: s.terminal,
  sftp: s.sftp,
  tunnels: s.tunnels,
  snippets: s.snippets,
  ai: s.ai,
  devices: s.devices,
  sync: s.sync,
  settings: s.settings,
  backups: s.backups,
  files: s.files,
  sftpBrowser: s.sftpBrowser,
  prompts: s.prompts,
  sharing: s.sharing,
  teams: s.teams,
  enrollment: s.enrollment,
  kubernetes: s.kubernetes,
  databases: s.databases,
  docker: s.docker,
  automation: s.automation,
  jobNotices: s.jobNotices,
  runtimeNotices: s.runtimeNotices,
  notes: s.notes,
  developer: s.developer,
  onDispose: s.onDispose,
  approvedExitActions: s.approvedExitActions,
);
void main() {
  testWidgets(
    'current dirty discard opens real workspace installer without saving or downloading',
    (tester) async {
      final service = Projects();
      await open(tester, service);
      await dirty(tester);
      await menu(tester);
      expect(find.text('Save changes to this file?'), findsOneWidget);
      await tester.tap(find.text('Discard'));
      await settle(tester);
      expect(find.byType(AutomationDependencyDialog), findsOneWidget);
      expect(service.saves, 0);
      expect(service.native.requests, isEmpty);
      await finish(tester);
    },
    variant: const TargetPlatformVariant({TargetPlatform.windows}),
  );
  testWidgets(
    'current dirty save owns its request transition and opens installer once',
    (tester) async {
      final service = Projects();
      await open(tester, service);
      await dirty(tester);
      await menu(tester);
      await tester.tap(find.text('Save').last);
      await settle(tester);
      expect(service.saves, 1);
      expect(service.text, contains('explicit unsaved draft'));
      expect(find.byType(AutomationDependencyDialog), findsOneWidget);
      expect(service.native.requests, isEmpty);
      await finish(tester);
    },
    variant: const TargetPlatformVariant({TargetPlatform.windows}),
  );
  testWidgets(
    'Terminal hide during dirty prompt retires original intent and never revives on return',
    (tester) async {
      final service = Projects();
      final container = await open(tester, service);
      await dirty(tester);
      await menu(tester);
      expect(find.text('Save changes to this file?'), findsOneWidget);
      container.read(routerProvider).go(AppRoutes.terminal);
      await settle(tester);
      expect(find.text('Save changes to this file?'), findsNothing);
      expect(find.byType(AutomationDependencyDialog), findsNothing);
      container.read(routerProvider).go(AppRoutes.automation);
      await settle(tester);
      expect(find.byType(AutomationDependencyDialog), findsNothing);
      expect(service.saves, 0);
      expect(service.native.requests, isEmpty);
      await finish(tester);
    },
    variant: const TargetPlatformVariant({TargetPlatform.windows}),
  );
  testWidgets(
    'same-profile automation service replacement while dirty cannot create a fresh review',
    (tester) async {
      final original = Projects(), replacement = Projects();
      final container = await open(tester, original);
      await dirty(tester);
      await menu(tester);
      replace(container, automation: replacement);
      await settle(tester);
      expect(find.text('Save changes to this file?'), findsNothing);
      expect(find.byType(AutomationDependencyDialog), findsNothing);
      expect(original.saves, 0);
      expect(replacement.saves, 0);
      expect(original.native.requests, isEmpty);
      expect(replacement.native.requests, isEmpty);
      await finish(tester);
    },
    variant: const TargetPlatformVariant({TargetPlatform.windows}),
  );
  testWidgets(
    'AppServices replacement with same original service profile and scope retires owned prompt',
    (tester) async {
      final service = Projects();
      final container = await open(tester, service);
      await dirty(tester);
      await menu(tester);
      final scope = container.read(automationScopeProvider);
      replace(
        container,
        automation: service,
        services: wrapper(container.read(appServicesProvider)),
      );
      await settle(tester);
      expect(identical(container.read(automationScopeProvider), scope), true);
      expect(find.text('Save changes to this file?'), findsNothing);
      expect(find.byType(AutomationDependencyDialog), findsNothing);
      expect(service.saves, 0);
      expect(service.native.requests, isEmpty);
      await finish(tester);
    },
    variant: const TargetPlatformVariant({TargetPlatform.windows}),
  );
  testWidgets('retiring owned dirty route never pops an unrelated top modal', (
    tester,
  ) async {
    final service = Projects();
    final container = await open(tester, service);
    await dirty(tester);
    await menu(tester);
    final context = tester.element(find.text('Save changes to this file?'));
    final unrelated = showDialog<void>(
      context: context,
      barrierDismissible: false,
      builder: (_) =>
          const AlertDialog(title: Text('Unrelated owned test modal')),
    );
    await settle(tester);
    container.read(routerProvider).go(AppRoutes.terminal);
    await settle(tester);
    expect(find.text('Unrelated owned test modal'), findsOneWidget);
    expect(find.text('Save changes to this file?'), findsNothing);
    expect(service.saves, 0);
    expect(service.native.requests, isEmpty);
    Navigator.of(tester.element(find.text('Unrelated owned test modal'))).pop();
    await unrelated;
    await finish(tester);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
}
