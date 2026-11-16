import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/app/critical_notice_providers.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/automation/agent_installer_project_focus.dart';
import 'package:consolecrypt/automation/automation_agent_installer.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/core/bridge/rust_automation.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/file_dialog_service.dart';
import 'package:consolecrypt/core/services/runtime_notice_adapter.dart';
import 'package:consolecrypt/src/rust/frb_generated.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';
import '../support/agent_installer_fixture.dart' as f;
import 'automation_exporter_template_test.dart' show Projects;

// Real generated installer/project delegates and real Hosts/Automation widgets.
// Replies are synthetic; no Native installation, runtime or execution evidence.
class _Api extends Fake implements RustLibApi {
  late Host host;
  final calls = <String>[], unexpected = <Symbol>[], requests = <Map<String, dynamic>>[];
  final projects = <Map<String, dynamic>>[];
  Completer<String>? pendingContext, pendingCreate;
  Map<String, dynamic>? contextFrame, createRequest;
  bool omitHost = false;
  String contextReply(Map<String, dynamic> frame) => jsonEncode({
    ...frame,
    'hosts': [
      if (!omitHost) {'hostId': host.id.value, 'hostRevision': '9007199254740993', 'name': host.name},
      {'hostId': f.hostId, 'hostRevision': '2', 'name': 'Another source target'},
    ],
  });
  String createdReply(Map<String, dynamic> request) {
    final body = f.created(request)
      ..['profileId'] = request['profileId']
      ..['notificationScopeId'] = request['notificationScopeId']
      ..['hostId'] = request['hostId'];
    final project = body['project'] as Map<String, dynamic>;
    if (!projects.any((p) => p['id'] == project['id'])) projects.add(project);
    return jsonEncode(body);
  }

  @override
  Future<String> crateApiAgentInstallerAutomationAgentInstallerContextJson({required String requestJson}) async {
    calls.add('context');
    contextFrame = jsonDecode(requestJson) as Map<String, dynamic>;
    return pendingContext?.future ?? contextReply(contextFrame!);
  }

  @override
  Future<String> crateApiAgentInstallerAutomationAgentInstallerCreateJson({required String requestJson}) async {
    calls.add('create');
    final request = jsonDecode(requestJson) as Map<String, dynamic>;
    requests.add(request);
    createRequest = request;
    return pendingCreate?.future ?? createdReply(request);
  }

  @override
  Future<String> crateApiAutomationAutomationProjects() async {
    calls.add('projects');
    return jsonEncode({'schema_version': 1, 'read_only': false, 'device_local': true, 'projects': projects});
  }

  @override
  Future<String> crateApiAutomationAutomationProjectFiles({required String id}) async {
    calls.add('files');
    expect(id, f.projectId);
    return jsonEncode({
      'digest': 'a' * 64,
      'bytes': 8,
      'entries': [
        {'path': 'playbook.yml', 'type': 'file', 'bytes': 8},
      ],
    });
  }

  @override
  Future<String> crateApiAutomationAutomationProjectFile({required String id, required String path}) async {
    calls.add('read');
    expect(id, f.projectId);
    expect(path, 'playbook.yml');
    return jsonEncode({'path': path, 'text': '---\n[]\n', 'sha256': 'b' * 64, 'line_endings': 'lf'});
  }

  @override
  Future<String> crateApiAutomationAutomationRunners() async =>
      jsonEncode({'read_only': false, 'runners': <Object?>[]});
  @override
  Future<String> crateApiAutomationAutomationJobs() async => jsonEncode({'jobs': <Object?>[]});
  @override
  dynamic noSuchMethod(Invocation invocation) {
    unexpected.add(invocation.memberName);
    throw StateError('No automatic dispatch or privilege operation allowed');
  }
}

class _Files extends Fake implements FileDialogService {
  @override
  Future<String?> chooseOpenFile({List<String> extensions = const []}) async =>
      'D:/public/candidate.${extensions.single}';
}

Future<(MockBackend, ProviderContainer, RuntimeNoticeEventAdapter)> _open(
  WidgetTester t,
  _Api api, {
  bool previousJobs = false,
}) async {
  final backend = testBackend(), events = RuntimeNoticeEventAdapter();
  addTearDown(backend.dispose);
  addTearDown(events.dispose);
  await backend.debugCreateUnlockedLocalProfile(name: 'Owned Host lifecycle');
  final host = await backend.inventory.saveHost(Host.create(name: 'Owned Agent target', address: '192.0.2.8'));
  api.host = host;
  await pumpApp(
    t,
    backend,
    size: const Size(1500, 1100),
    overrides: [
      automationServiceProvider.overrideWithValue(RustAutomationService()),
      criticalRuntimeEventsProvider.overrideWithValue(events),
      fileDialogServiceProvider.overrideWithValue(_Files()),
    ],
  );
  final c = ProviderScope.containerOf(t.element(find.byType(Navigator).first));
  events.acceptScope(
    originalEpoch: events.epoch,
    nativeProfile: {
      'id': c.read(activeProfileProvider)!.id.value,
      'active': true,
      'vault_state': 'unlocked',
      'notification_scope_id': 'host original nonce',
    },
  );
  await settle(t);
  if (previousJobs) {
    c.read(routerProvider).go(AppRoutes.automation);
    await settle(t);
    await tapKey(t, 'automation-workspace-jobs');
    c.read(routerProvider).go(AppRoutes.hosts);
    await settle(t);
    api.calls.clear();
  }
  await enterKey(t, 'hosts-search', host.name);
  await tapKey(t, 'host-menu-${host.name}');
  await tapKey(t, 'host-agent-menu-${host.id.value}');
  return (backend, c, events);
}

Future<void> _fill(WidgetTester t, AgentInstallerOperation operation) async {
  await enterKey(t, 'agent-installer-name', 'Owned Agent action');
  t
      .widget<DropdownButtonFormField<AgentInstallerOperation>>(find.byKey(const ValueKey('agent-installer-operation')))
      .onChanged!(operation);
  await settle(t);
  if (operation == AgentInstallerOperation.install || operation == AgentInstallerOperation.update) {
    await tapKey(t, 'agent-installer-artifact');
    await tapKey(t, 'agent-installer-manifest');
  }
}

void main() {
  late _Api api;
  setUp(() {
    api = _Api();
    RustLib.initMock(api: api);
  });
  tearDown(RustLib.dispose);
  testWidgets(
    'Host cAdvisor action hands actual original Host to ordinary Automation disclosure without creating or running',
    (t) async {
      final (_, container, _) = await _open(t, api);
      expect(api.calls, ['context']);
      await tapKey(t, 'host-agent-cadvisor-ansible');
      await settle(t);
      final route = container.read(routerProvider).state;
      expect(route.uri.path, AppRoutes.automation);
      final focus = route.extra as AgentCadvisorComposerFocus;
      expect(focus.hostId, api.host.id.value);
      expect(focus.hostRevision, '9007199254740993');
      expect(focus.frame.notificationScopeId, 'host original nonce');
      expect(find.byKey(const ValueKey('automation-cadvisor-review')), findsOneWidget);
      expect(api.requests, isEmpty);
      expect(api.projects, isEmpty);
      expect(api.unexpected, isEmpty);
      await tapKey(t, 'automation-cadvisor-cancel');
      expect(api.requests, isEmpty);
      expect(api.projects, isEmpty);
      expect(t.takeException(), isNull);
    },
    variant: const TargetPlatformVariant({TargetPlatform.windows}),
  );
  for (final operation in [
    AgentInstallerOperation.install,
    AgentInstallerOperation.update,
    AgentInstallerOperation.rollback,
    AgentInstallerOperation.remove,
  ]) {
    testWidgets(
      'Host ${operation.name} creates only source for same exact Native target and returns from normal workspace',
      (t) async {
        final (_, c, _) = await _open(t, api, previousJobs: operation == AgentInstallerOperation.install);
        expect(api.calls, ['context']);
        expect(find.byKey(const ValueKey('agent-installer-host')), findsNothing);
        expect(find.byKey(const ValueKey('agent-installer-fixed-host')), findsOneWidget);
        final l = t.element(find.byKey(const ValueKey('agent-installer-dialog'))).l10n;
        expect(find.text(l.hostAgentObservedUnknown), findsOneWidget);
        await _fill(t, operation);
        await tapKey(t, 'agent-installer-create');
        expect(api.requests.single['operation'], operation.name);
        expect(api.requests.single['hostId'], api.host.id.value);
        expect(api.requests.single['expectedHostRevision'], '9007199254740993');
        expect(api.requests.single['start'], false);
        expect(c.read(routerProvider).state.uri.path, AppRoutes.hosts);
        expect(find.text(l.agentInstallProjectCreated), findsOneWidget);
        expect(find.text(l.agentInstallRuntimeUnknown), findsOneWidget);
        await tapKey(t, 'agent-installer-open');
        await settle(t);
        expect(c.read(routerProvider).state.uri.path, AppRoutes.automation);
        expect(find.byKey(const ValueKey('automation-file-playbook.yml')), findsOneWidget);
        expect(api.calls.where((s) => s == 'create').length, 1);
        expect(api.unexpected, isEmpty);
        await tapKey(t, 'automation-file-playbook.yml');
        expect(t.widget<GlassButton>(find.byKey(const ValueKey('automation-open-run'))).onPressed, isNotNull);
        expect(api.unexpected, isEmpty, reason: 'Normal run remains a separate explicit reviewed action');
        await tapKey(t, 'automation-return-agent-host');
        await settle(t);
        expect(c.read(routerProvider).state.uri.path, AppRoutes.hosts);
        expect(t.widget<GlassField>(find.byKey(const ValueKey('hosts-search'))).controller!.text, api.host.name);
        expect(find.byKey(ValueKey('host-${api.host.name}')), findsOneWidget);
        expect(api.unexpected, isEmpty);
        expect(t.takeException(), isNull);
      },
      variant: const TargetPlatformVariant({TargetPlatform.windows}),
    );
  }
  testWidgets('cancel reads metadata only and never creates or hands off', (t) async {
    final (_, c, _) = await _open(t, api);
    await tapKey(t, 'agent-installer-cancel');
    expect(api.calls, ['context']);
    expect(c.read(routerProvider).state.uri.path, AppRoutes.hosts);
    expect(api.requests, isEmpty);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  testWidgets('missing exact target cannot fall back to another Native host', (t) async {
    api.omitHost = true;
    await _open(t, api);
    final l = t.element(find.byKey(const ValueKey('agent-installer-dialog'))).l10n;
    expect(find.text(l.hostAgentTargetChanged), findsOneWidget);
    expect(t.widget<TextButton>(find.byKey(const ValueKey('agent-installer-create'))).onPressed, isNull);
    expect(t.widget<TextButton>(find.byKey(const ValueKey('host-agent-cadvisor-ansible'))).onPressed, isNull);
    expect(api.requests, isEmpty);
    expect(find.text('Another source target'), findsNothing);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  testWidgets('host replacement before create retires original dialog with zero writes', (t) async {
    final (backend, c, _) = await _open(t, api);
    await _fill(t, AgentInstallerOperation.remove);
    await backend.inventory.saveHost(api.host.withNotes('changed'));
    await settle(t);
    expect(find.byKey(const ValueKey('agent-installer-dialog')), findsNothing);
    expect(api.requests, isEmpty);
    expect(c.read(routerProvider).state.uri.path, AppRoutes.hosts);
    expect(api.unexpected, isEmpty);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  testWidgets('original notice boundary during pending metadata rejects late frame without writes', (t) async {
    api.pendingContext = Completer();
    final (_, c, events) = await _open(t, api);
    events.boundary();
    await settle(t);
    api.pendingContext!.complete(api.contextReply(api.contextFrame!));
    await settle(t);
    expect(api.requests, isEmpty);
    expect(find.byKey(const ValueKey('agent-installer-dialog')), findsNothing);
    expect(c.read(routerProvider).state.uri.path, AppRoutes.hosts);
    expect(api.unexpected, isEmpty);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  testWidgets('late creation receipt after owner boundary cannot hand off or dispatch', (t) async {
    api.pendingCreate = Completer();
    final (_, c, events) = await _open(t, api);
    await _fill(t, AgentInstallerOperation.remove);
    await tapKey(t, 'agent-installer-create');
    events.boundary();
    await settle(t);
    api.pendingCreate!.complete(api.createdReply(api.createRequest!));
    await settle(t);
    expect(api.requests.length, 1, reason: 'Already dispatched source creation is not claimed retracted');
    expect(c.read(routerProvider).state.uri.path, AppRoutes.hosts);
    expect(api.calls, isNot(contains('files')));
    expect(api.unexpected, isEmpty);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  testWidgets('older project service without installer capability exposes no fake Host action', (t) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugCreateUnlockedLocalProfile(name: 'Older source workflow');
    final host = await backend.inventory.saveHost(Host.create(name: 'Older SSH source', address: '192.0.2.9'));
    await pumpApp(t, backend, overrides: [automationServiceProvider.overrideWithValue(Projects())]);
    await tapKey(t, 'host-menu-${host.name}');
    expect(find.byKey(ValueKey('host-agent-menu-${host.id.value}')), findsNothing);
    expect(api.calls, isEmpty);
    expect(api.unexpected, isEmpty);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
}
