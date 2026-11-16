import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/app/critical_notice_providers.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/automation/automation_awx_connection_form.dart';
import 'package:consolecrypt/automation/automation_awx_connection_host.dart';
import 'package:consolecrypt/automation/automation_awx_entry.dart';
import 'package:consolecrypt/automation/automation_awx_handle_gateway.dart';
import 'package:consolecrypt/automation/automation_awx_models.dart';
import 'package:consolecrypt/automation/automation_awx_workspace.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/core/services/runtime_notice_adapter.dart';
import 'package:consolecrypt/credentials/credential_dialogs.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_ui_proof.dart';
import '../helpers/test_app.dart';
import '../unit/automation_awx_handle_gateway_test.dart' as g;
import '../unit/automation_awx_models_test.dart' as m;

class Projects extends Fake implements AutomationService {
  @override
  Future<AutomationProjectCatalog> projects() async =>
      const AutomationProjectCatalog(version: 1, readOnly: false, deviceLocal: true, projects: []);
}

class Port extends g.Port {
  Port(this.profile);
  final String profile;
  @override
  Future<AwxIssuedRequestHandle> registerRequest(String raw) async {
    final request = jsonDecode(raw) as Map<String, dynamic>;
    final action = request['action'] as Map<String, dynamic>;
    final kind = action['kind'];
    if (kind != 'prepare' && kind != 'launch') return super.registerRequest(raw);
    requests.add(request);
    final data = kind == 'prepare'
        ? {
            'reviewTicket': '77777777-7777-4777-8777-777777777777',
            'template': m.templateJson(),
            'parameters': action['parameters'],
            'expiresUtc': DateTime.now().toUtc().add(const Duration(seconds: 60)).toIso8601String(),
            'connectionId': m.connectionId,
            'connectionRevision': 3,
          }
        : {
            'status': 'started',
            'job': m.jobJson(),
            'parametersIgnored': false,
            'originalJobUrl': 'https://awx.example.test/controller/api/v2/jobs/21/',
          };
    final handle = g.Handle(auto: g.envelope(kind as String, data));
    handles.add(handle);
    return handle;
  }

  @override
  Future<String> captureScope() async {
    captures++;
    return captureWait?.future ?? jsonEncode({'version': 1, 'profileId': profile, 'notificationScopeId': m.nonce});
  }
}

Future<(ProviderContainer, Port, RuntimeNoticeEventAdapter)> open(
  WidgetTester tester, {
  bool available = true,
  AppLocale locale = AppLocale.en,
  double width = 1600,
  bool stableIdentity = false,
}) async {
  final backend = testBackend(seed: false);
  addTearDown(backend.dispose);
  await backend.debugCreateUnlockedLocalProfile(name: 'Owned AWX entry');
  final port = Port(backend.services.profiles.currentProfiles.active!.id.value), hub = RuntimeNoticeEventAdapter();
  addTearDown(hub.dispose);
  Port factory() => port;
  final profile = backend.services.profiles.currentProfiles.active!;
  final fixedScope = AutomationScope(profile.id, true);
  await loadAutomationProofFonts(tester);
  await pumpApp(
    tester,
    backend,
    locale: locale,
    size: Size(width, 1000),
    captureBoundary: true,
    overrides: [
      automationServiceProvider.overrideWithValue(Projects()),
      if (stableIdentity) automationScopeProvider.overrideWithValue(fixedScope),
      if (stableIdentity) activeProfileProvider.overrideWithValue(profile),
      if (stableIdentity) appStageProvider.overrideWithValue(AppStage.unlocked),
      criticalRuntimeEventsProvider.overrideWithValue(hub),
      automationAwxPortFactoryProvider.overrideWithValue(available ? factory : null),
    ],
  );
  final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
  container.read(routerProvider).go(AppRoutes.automation);
  await settle(tester);
  return (container, port, hub);
}

Future<void> awx(WidgetTester tester) async {
  await tapKey(tester, 'automation-workspace-awx');
  await tapKey(tester, 'awx-catalog-refresh');
  await tapKey(tester, 'awx-connection-${m.connectionId}');
}

Future<void> paletteTerminal(WidgetTester tester) async {
  await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
  await tester.sendKeyEvent(LogicalKeyboardKey.keyK);
  await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
  await settle(tester);
  expect(find.byKey(const ValueKey('command-palette')), findsOneWidget);
  await enterKey(tester, 'palette-input', 'Terminal');
  await settle(tester);
  await tester.tap(find.text('Terminal').last);
  await settle(tester);
}

void main() {
  testWidgets('missing optional Native capability leaves existing Projects UI with no AWX tab', (tester) async {
    final (_, port, _) = await open(tester, available: false);
    expect(find.byKey(const ValueKey('automation-workspace-projects')), findsOneWidget);
    expect(find.byKey(const ValueKey('automation-workspace-awx')), findsNothing);
    expect(port.captures, 0);
    expect(port.requests, isEmpty);
  });
  testWidgets('opening/selecting AWX never reads until explicit catalog and keeps existing project branch', (
    tester,
  ) async {
    final (_, port, _) = await open(tester);
    expect(port.captures, 0);
    expect(port.requests, isEmpty);
    await tapKey(tester, 'automation-workspace-awx');
    expect(port.captures, 0);
    expect(port.catalogs, isEmpty);
    await tapKey(tester, 'awx-catalog-refresh');
    expect(port.captures, 1);
    expect(port.catalogs.length, 1);
    await tapKey(tester, 'automation-workspace-projects');
    await tapKey(tester, 'automation-workspace-awx');
    expect(port.catalogs.length, 1);
    expect(port.requests, isEmpty);
    expect(find.text('Synthetic AWX'), findsOneWidget);
  });
  testWidgets('CommandPalette section hide removes only owned connection form and nested secure credential route', (
    tester,
  ) async {
    final (container, port, _) = await open(tester);
    await awx(tester);
    await tapKey(tester, 'awx-connection-add');
    expect(find.byType(AwxConnectionForm), findsOneWidget);
    await tapKey(tester, 'awx-create-credential');
    expect(find.byType(PasswordCredentialDialog), findsOneWidget);
    await paletteTerminal(tester);
    expect(container.read(routerProvider).routeInformationProvider.value.uri.path, AppRoutes.terminal);
    expect(find.byType(AwxConnectionForm), findsNothing);
    expect(find.byType(PasswordCredentialDialog), findsNothing);
    expect(port.requests, isEmpty);
    container.read(routerProvider).go(AppRoutes.automation);
    await settle(tester);
    expect(find.byType(AwxWorkspace), findsOneWidget);
    expect(port.catalogs.length, 1);
  });
  testWidgets('initial Native scope completion after global epoch loss cannot adopt reopened session', (tester) async {
    final (_, port, hub) = await open(tester);
    port.captureWait = Completer<String>();
    await tapKey(tester, 'automation-workspace-awx');
    await tapKey(tester, 'awx-catalog-refresh');
    hub.retireGlobalStream();
    port.captureWait!.complete(jsonEncode({'version': 1, 'profileId': port.profile, 'notificationScopeId': m.nonce}));
    await settle(tester);
    expect(port.catalogs, isEmpty);
    expect(port.requests, isEmpty);
    expect(find.byKey(const ValueKey('awx-reopen')), findsOneWidget);
  });
  testWidgets('hidden view cancels owned in-flight read and prevents late body start without destroying accepted job', (
    tester,
  ) async {
    final (container, port, _) = await open(tester);
    await awx(tester);
    final c = tester.widget<AwxConnectionWorkspace>(find.byType(AwxConnectionWorkspace)).controller;
    c.job = AwxJob.fromJson(m.jobJson(), expectedId: 21);
    port.registrationWait = Completer<AwxIssuedRequestHandle>();
    await tapKey(tester, 'awx-read-templates');
    container.read(routerProvider).go(AppRoutes.terminal);
    await settle(tester);
    final raw = g.Handle();
    port.registrationWait!.complete(raw);
    await settle(tester);
    expect(raw.starts, 0);
    expect(raw.cancels, greaterThan(0));
    expect(c.job?.id, 21);
    container.read(routerProvider).go(AppRoutes.automation);
    await settle(tester);
    final after = tester.widget<AwxConnectionWorkspace>(find.byType(AwxConnectionWorkspace)).controller;
    expect(identical(after, c), isTrue);
    expect(after.job?.id, 21);
    expect(port.requests.every((r) => (r['action'] as Map)['kind'] != 'launch'), isTrue);
  });
  testWidgets('explicit accepted launch survives section hide without replay, polling or remote cancel', (
    tester,
  ) async {
    final (container, port, hub) = await open(tester);
    await awx(tester);
    await tapKey(tester, 'awx-read-templates');
    await tapKey(tester, 'awx-template-9');
    await tapKey(tester, 'awx-prepare');
    final c = tester.widget<AwxConnectionWorkspace>(find.byType(AwxConnectionWorkspace)).controller;
    expect(c.review, isNotNull);
    expect(c.job, isNull);
    await tapKey(tester, 'awx-launch');
    expect(c.job?.id, 21);
    expect(c.outcome, isA<AwxLaunchStarted>());
    final requestCount = port.requests.length;
    hub.streamLost();
    await settle(tester);
    expect(c.current, isTrue);
    container.read(routerProvider).go(AppRoutes.terminal);
    await settle(tester);
    expect(c.job?.id, 21);
    await tester.pump(const Duration(seconds: 60));
    expect(port.requests.length, requestCount);
    container.read(routerProvider).go(AppRoutes.automation);
    await settle(tester);
    final after = tester.widget<AwxConnectionWorkspace>(find.byType(AwxConnectionWorkspace)).controller;
    expect(identical(c, after), isTrue);
    expect(after.job?.id, 21);
    expect(port.requests.length, requestCount);
    expect(port.requests.where((r) => (r['action'] as Map)['kind'] == 'launch').length, 1);
    expect(port.handles.every((handle) => handle.cancels == 0), isTrue);
  });

  for (final width in [1400.0, 500.0]) {
    testWidgets('actual AWX Automation tab RU at $width', (tester) async {
      if (width == 500) tester.platformDispatcher.textScaleFactorTestValue = 1.5;
      addTearDown(tester.platformDispatcher.clearTextScaleFactorTestValue);
      final (_, port, _) = await open(tester, locale: AppLocale.ru, width: width);
      await awx(tester);
      expect(find.byType(AwxWorkspace), findsOneWidget);
      expect(port.requests, isEmpty);
      expect(tester.takeException(), isNull);
      await captureAutomationWorkspace(tester, 'awx-entry-ru-${width.toInt()}');
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
  }
  testWidgets('service replacement during owned editor closes original route and never saves to replacement', (
    tester,
  ) async {
    final (container, port, hub) = await open(tester);
    await awx(tester);
    await tapKey(tester, 'awx-connection-add');
    expect(find.byType(AwxConnectionForm), findsOneWidget);
    final services = container.read(appServicesProvider), factory = container.read(automationAwxPortFactoryProvider);
    container.updateOverrides([
      appServicesProvider.overrideWithValue(services),
      automationServiceProvider.overrideWithValue(Projects()),
      criticalRuntimeEventsProvider.overrideWithValue(hub),
      automationAwxPortFactoryProvider.overrideWithValue(factory),
    ]);
    await settle(tester);
    expect(tester.takeException(), isNull);
    expect(find.byType(AwxConnectionForm), findsNothing);
    expect(port.catalogs.length, 1);
    expect(port.requests, isEmpty);
  });

  testWidgets('optional capability retirement removes owned modal with no global route pop or stale save', (
    tester,
  ) async {
    final (container, port, hub) = await open(tester);
    await awx(tester);
    await tapKey(tester, 'awx-connection-add');
    expect(find.byType(AwxConnectionForm), findsOneWidget);
    final services = container.read(appServicesProvider), service = container.read(automationServiceProvider);
    container.updateOverrides([
      appServicesProvider.overrideWithValue(services),
      automationServiceProvider.overrideWithValue(service),
      criticalRuntimeEventsProvider.overrideWithValue(hub),
      automationAwxPortFactoryProvider.overrideWithValue(null),
    ]);
    await settle(tester);
    expect(tester.takeException(), isNull);
    expect(find.byType(AwxConnectionForm), findsNothing);
    expect(find.byKey(const ValueKey('automation-workspace-awx')), findsNothing);
    expect(container.read(routerProvider).routeInformationProvider.value.uri.path, AppRoutes.automation);
    expect(port.catalogs.length, 1);
    expect(port.requests, isEmpty);
  });
  testWidgets(
    'AppServices replacement with same original profile scope service and hub rejects late initial Native capture',
    (tester) async {
      final (container, port, hub) = await open(tester, stableIdentity: true);
      final services = container.read(appServicesProvider),
          service = container.read(automationServiceProvider),
          factory = container.read(automationAwxPortFactoryProvider),
          scope = container.read(automationScopeProvider),
          profile = container.read(activeProfileProvider);
      port.captureWait = Completer<String>();
      await tapKey(tester, 'automation-workspace-awx');
      await tapKey(tester, 'awx-catalog-refresh');
      final original = tester.widget<AwxConnectionWorkspace>(find.byType(AwxConnectionWorkspace)).controller;
      expect(original.current, isTrue);
      final replacement = AppServices(
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
        monitoring: services.monitoring,
        jobNotices: services.jobNotices,
        runtimeNotices: services.runtimeNotices,
        notes: services.notes,
        developer: services.developer,
        onDispose: services.onDispose,
        approvedExitActions: services.approvedExitActions,
      );
      container.updateOverrides([
        appServicesProvider.overrideWithValue(replacement),
        automationServiceProvider.overrideWithValue(service),
        criticalRuntimeEventsProvider.overrideWithValue(hub),
        automationAwxPortFactoryProvider.overrideWithValue(factory),
        automationScopeProvider.overrideWithValue(scope),
        activeProfileProvider.overrideWithValue(profile),
        appStageProvider.overrideWithValue(AppStage.unlocked),
      ]);
      await settle(tester);
      expect(identical(container.read(automationScopeProvider), scope), isTrue);
      expect(identical(container.read(automationServiceProvider), service), isTrue);
      expect(identical(container.read(criticalRuntimeEventsProvider), hub), isTrue);
      port.captureWait!.complete(jsonEncode({'version': 1, 'profileId': port.profile, 'notificationScopeId': m.nonce}));
      await settle(tester);
      expect(original.current, isFalse);
      expect(port.catalogs, isEmpty);
      expect(port.requests, isEmpty);
    },
  );
}
