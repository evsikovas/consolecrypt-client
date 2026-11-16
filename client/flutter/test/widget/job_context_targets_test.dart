import 'dart:async';
import 'dart:typed_data';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/automation/ansible_code_editor.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_run_dialog.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/mock/mock_docker.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/terminal_service.dart';
import 'package:consolecrypt/docker/docker_providers.dart';
import 'package:consolecrypt/docker/docker_service.dart';
import 'package:consolecrypt/docker/docker_workspace.dart';
import 'package:consolecrypt/monitoring/job_context_focus.dart';
import 'package:consolecrypt/monitoring/job_context_navigation.dart';
import 'package:consolecrypt/monitoring/job_metric_correlation.dart';
import 'package:consolecrypt/monitoring/monitoring_binding_models.dart';
import 'package:consolecrypt/monitoring/monitoring_binding_reader.dart';
import 'package:consolecrypt/monitoring/monitoring_host_context.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_pinned_observation.dart';
import 'package:consolecrypt/monitoring/monitoring_pinned_providers.dart';
import 'package:consolecrypt/monitoring/monitoring_providers.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/monitoring_binding_fakes.dart';
import '../helpers/monitoring_binding_reads_fakes.dart';
import '../helpers/monitoring_fakes.dart';
import '../helpers/test_app.dart';
import '../unit/job_metric_correlation_test.dart' as f;

final hostA = Host(
  id: ObjectId(f.host),
  name: 'Context host A',
  address: 'a.invalid',
  createdAt: f.at,
  updatedAt: f.at,
);
final hostB = Host(
  id: ObjectId(f.uuid(88)),
  name: 'Existing host B',
  address: 'b.invalid',
  createdAt: f.at,
  updatedAt: f.at,
);

class _Terminal implements TerminalService, LocalTerminalService {
  final opened = <String>[], closed = <TerminalSessionId>[];
  int discoveries = 0;
  @override
  Future<TerminalSessionHandle> open({required ObjectId hostId, required TerminalSize size}) async {
    opened.add(hostId.value);
    return TerminalSessionHandle(
      id: TerminalSessionId.generate(),
      output: const Stream<Uint8List>.empty(),
      events: Stream.value(const TerminalStateChanged(SessionConnectionState.connected)),
    );
  }

  @override
  Future<LocalShellProfiles> listLocalProfiles() async {
    discoveries++;
    return const LocalShellProfiles();
  }

  @override
  Future<TerminalSessionHandle> openLocal({required String profileId, required TerminalSize size}) =>
      throw StateError('No local fallback');
  @override
  Future<void> close(TerminalSessionId id) async => closed.add(id);
  @override
  Future<void> write(TerminalSessionId id, Uint8List data) async {}
  @override
  Future<void> resize(TerminalSessionId id, TerminalSize size) async {}
  @override
  Future<void> answerHostKey(TerminalSessionId id, HostKeyDecision decision) async {}
  @override
  Future<void> answerPassword(TerminalSessionId id, SecretText? password) async {}
  @override
  Future<void> reconnect(TerminalSessionId id) async {}
}

class _Docker extends MockDockerService {
  final connected = <String>[], disconnected = <String>[];
  Completer<DockerSessionInfo>? pending;
  @override
  Future<List<DockerHost>> hosts() async => [
    const DockerHost(id: 'local', name: 'Local must not be selected', kind: DockerHostKind.local),
    DockerHost(id: hostA.id.value, name: hostA.name, kind: DockerHostKind.ssh),
    DockerHost(id: hostB.id.value, name: hostB.name, kind: DockerHostKind.ssh),
  ];
  @override
  Future<DockerSessionInfo> connect(String hostId) async {
    connected.add(hostId);
    if (pending != null) return pending!.future;
    final value = await super.connect('local');
    return DockerSessionInfo(
      id: value.id,
      hostId: hostId,
      hostName: hostId == hostA.id.value ? hostA.name : hostB.name,
      engineVersion: value.engineVersion,
    );
  }

  @override
  Future<void> disconnect(String id) async {
    disconnected.add(id);
    await super.disconnect(id);
  }
}

class _Projects implements AutomationService, AutomationJobService {
  late String profileId;
  int jobRevision = 3;
  String jobState = 'succeeded';
  String digest = f.digest;
  final filesRead = <String>[], textRead = <String>[], saved = <String>[];
  static final a = AutomationProject(id: f.project, name: 'Context project A', revision: 6, readOnly: false);
  static final b = AutomationProject(id: f.uuid(600), name: 'Existing project B', revision: 2, readOnly: false);
  @override
  Future<AutomationProjectCatalog> projects() async =>
      AutomationProjectCatalog(version: 1, readOnly: false, deviceLocal: true, projects: [a, b]);
  @override
  Future<AutomationProjectRevision> files(String projectId) async {
    filesRead.add(projectId);
    return AutomationProjectRevision(
      digest: projectId == a.id ? digest : List.filled(64, 'b').join(),
      bytes: 20,
      entries: const [AutomationFileEntry(path: 'playbook.yml', directory: false)],
    );
  }

  @override
  Future<AutomationProjectFile> readFile(String projectId, String path) async {
    textRead.add(projectId);
    return AutomationProjectFile(
      path: path,
      text: '---\n# existing B draft\n',
      sha256: List.filled(64, 'b').join(),
      lineEndings: 'lf',
    );
  }

  @override
  Future<AutomationProjectFile> saveFile(
    String projectId, {
    required String path,
    required String text,
    String? expectedSha256,
  }) async {
    saved.add(projectId);
    return AutomationProjectFile(path: path, text: text, sha256: f.digest, lineEndings: 'lf');
  }

  @override
  Future<AutomationJson> jobs() async {
    final row = f.row(hosts: [hostA.id.value], revision: jobRevision, state: jobState);
    (row['manifest'] as Map<String, dynamic>)['tenant_id'] = profileId;
    return f.catalog([row]);
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => throw StateError('No automatic mutation, run or install');
}

final _metricsSource = MonitoringSource(id: f.source, name: 'Explicit metrics A', endpoint: 'http://metrics.invalid', revision: 3);
final _otherSource = MonitoringSource(id: f.uuid(777), name: 'Previous metrics B', endpoint: 'http://previous.invalid', revision: 1);
MonitoringBinding _metricsBinding() => MonitoringBinding(id: f.binding, name: 'Explicit host A binding', hostId: hostA.id.value,
  sourceId: f.source, revision: 4, sourceRevision: 3, selector: const {'job': 'node', 'instance': 'explicit:9100'},
  state: MonitoringBindingState.current);

class _MetadataBackend implements MonitoringBackend {
  _MetadataBackend({this.bindingCatalog = false});
  final bool bindingCatalog;
  final store = MonitoringBindingTestStore(initial: [_otherSource, _metricsSource], rows: [_metricsBinding()]);
  int hostRevision = 9, metadataCalls = 0, metrics = 0;
  Completer<Json>? pending;
  @override
  MonitoringSourceStore createSourceStore() => bindingCatalog ? store : MonitoringTestStore();
  @override
  MonitoringService createService() => AppCoreMonitoringService(
    read: (request) async {
      metrics++;
      final action = request['action'] as Map;
      if (bindingCatalog && action['kind'] == 'bindingPanel') {
        final panel = MonitoringPanel.values.singleWhere((p) => p.name == action['panel']);
        final result = bindingPanelJson(panel: panel);
        result['bindingId'] = f.binding; result['bindingRevision'] = 4;
        final plan = (result['preparation'] as Map)['plan'] as Map;
        plan['bindingRevision'] = 4; plan['sourceId'] = f.source; plan['hostId'] = hostA.id.value;
        plan['start'] = action['start']; plan['end'] = action['end']; plan['stepSeconds'] = action['stepSeconds'];
        return {'sourceId': request['sourceId'], 'sourceRevision': request['sourceRevision'], 'result': result};
      }
      return {};
    },
    cancelOperation: (_) async {},
    hostContextRead: (request) async {
      metadataCalls++;
      return pending?.future ??
          {
            'version': 1,
            'profileId': request['profileId'],
            'notificationScopeId': request['notificationScopeId'] ?? f.uuid(900),
            'hostId': request['hostId'],
            'hostRevision': hostRevision,
            'protocol': 'ssh',
            'sourceId': request['sourceId'],
            'sourceRevision': request['sourceRevision'],
            'bindingId': request['bindingId'],
            'bindingRevision': request['bindingRevision'],
          };
    },
  );
}

class _Harness {
  _Harness(this.container, this.metadata, this.terminal, this.docker, this.projects);
  final ProviderContainer container;
  final _MetadataBackend metadata;
  final _Terminal terminal;
  final _Docker docker;
  final _Projects projects;
  Future<JobContextFocus> reverseFocus() async {
    final session = container.read(monitoringSessionProvider)!;
    final reader = session.service as MonitoringHostContextReader;
    final captured = await reader.captureHostContext(profileId: projects.profileId,
      source: _metricsSource, binding: _metricsBinding());
    return JobContextFocus.forMonitoring(scope: session.correlationScope!, metadata: captured,
      jobId: f.uuid(100), jobRevision: projects.jobRevision, projectId: f.project, manifestDigest: f.digest)!;
  }
  JobContextFocus focus(JobContextDestination destination) {
    final scope = container.read(monitoringSessionProvider)!.correlationScope!;
    final context = JobCorrelationContext(
      scope: scope,
      hostId: hostA.id.value,
      hostRevision: 9,
      sourceId: f.source,
      sourceRevision: 3,
      bindingId: f.binding,
      bindingRevision: 4,
      bindingHostId: hostA.id.value,
      bindingSourceId: f.source,
      bindingSourceRevision: 3,
      bindingState: JobCorrelationBindingState.current,
      startUtc: f.at,
      endUtc: f.at.add(const Duration(minutes: 10)),
    );
    final metadata = JobContextMetadata.parse({
      'version': 1,
      'profileId': scope.profileId,
      'notificationScopeId': f.uuid(900),
      'hostId': context.hostId,
      'hostRevision': context.hostRevision,
      'protocol': 'ssh',
      'sourceId': context.sourceId,
      'sourceRevision': context.sourceRevision,
      'bindingId': context.bindingId,
      'bindingRevision': context.bindingRevision,
    })!;
    final correlation = JobMetricCorrelation()..bind(context);
    final row = f.row(hosts: [hostA.id.value], revision: 3);
    (row['manifest'] as Map<String, dynamic>)['tenant_id'] = scope.profileId;
    correlation.complete(correlation.begin(context)!, f.catalog([row]), f.at);
    final frame = JobContextNavigationFrame(
      context: context,
      notificationScopeId: metadata.notificationScopeId,
      metadata: metadata,
      jobs: correlation.snapshot,
      project: JobContextProject(f.project, 6, f.digest),
    );
    final owner = JobContextNavigation()..bind(frame);
    final intent = owner
        .propose(
          frame,
          destination,
          job: destination == JobContextDestination.ansibleProject ? frame.jobs!.markers.single : null,
        )
        .intent!;
    return JobContextFocus.fromTaken(owner.take(intent)!, frame)!;
  }
}

Future<_Harness> _open(WidgetTester tester, {bool reverse = false}) async {
  final backend = testBackend(),
      metadata = _MetadataBackend(bindingCatalog: reverse),
      terminal = _Terminal(),
      docker = _Docker(),
      projects = _Projects();
  addTearDown(backend.dispose);
  await backend.debugCreateUnlockedLocalProfile(name: 'Owned context profile');
  projects.profileId = backend.services.profiles.currentProfiles.active!.id.value;
  await pumpApp(
    tester,
    backend,
    overrides: [
      hostsProvider.overrideWith((ref) => Stream.value([hostA, hostB])),
      monitoringBackendProvider.overrideWithValue(metadata),
      terminalServiceProvider.overrideWithValue(terminal),
      dockerServiceProvider.overrideWithValue(docker),
      automationServiceProvider.overrideWithValue(projects),
    ],
  );
  final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
  container.read(routerProvider).go(AppRoutes.monitoring);
  await settle(tester);
  return _Harness(container, metadata, terminal, docker, projects);
}

void main() {
  testWidgets('failed job opens approved metrics chooser then focuses saved binding without query or execution', (tester) async {
    final h = await _open(tester, reverse: true);
    h.projects.jobState = 'failed';
    h.container.read(routerProvider).go(AppRoutes.automation);
    await settle(tester);
    await tapKey(tester, 'automation-workspace-jobs');
    await tapKey(tester, 'automation-job-metrics-${f.uuid(100)}');
    expect(h.metadata.metrics, 0);
    tester.widget<GlassSelect<String?>>(find.byKey(const ValueKey('job-metrics-host'))).onChanged!(hostA.id.value);
    await tester.pump();
    tester.widget<GlassSelect<String?>>(find.byKey(const ValueKey('job-metrics-source'))).onChanged!(f.source);
    await tester.pump();
    tester.widget<GlassSelect<String?>>(find.byKey(const ValueKey('job-metrics-binding'))).onChanged!(f.binding);
    await settle(tester);
    await tapKey(tester, 'job-metrics-open');
    expect(h.container.read(routerProvider).routeInformationProvider.value.uri.path, AppRoutes.monitoring);
    final reader = tester.widget<MonitoringBindingReaderView>(find.byType(MonitoringBindingReaderView));
    expect(reader.binding.id, f.binding);
    expect(reader.binding.hostId, hostA.id.value);
    expect(reader.source.id, f.source);
    expect(reader.requestedContext!.originalContext, isNull);
    expect(h.metadata.metrics, 0);
    expect(h.terminal.opened, isEmpty);
    expect(h.docker.connected, isEmpty);
    expect(h.projects.saved, isEmpty);
    await tapKey(tester, 'monitoring-binding-load-panel');
    expect(h.metadata.metrics, 1);
    expect(find.byKey(const ValueKey('monitoring-binding-read-state')), findsOneWidget);
    expect(h.projects.saved, isEmpty);
    h.container.read(monitoringPinnedObservationProvider)?.stop(MonitoringPinStop.manual);
  });
  testWidgets('receiver hide and return cannot accept a late pending original context', (tester) async {
    final h = await _open(tester, reverse: true);
    h.projects.jobState = 'failed';
    final packet = await h.reverseFocus();
    h.metadata.pending = Completer<Json>();
    h.container.read(routerProvider).go(AppRoutes.monitoring, extra: packet);
    await settle(tester);
    expect(h.metadata.metadataCalls, 2);
    h.container.read(routerProvider).go(AppRoutes.terminal);
    await settle(tester);
    h.container.read(routerProvider).go(AppRoutes.monitoring);
    await settle(tester);
    h.metadata.pending!.complete(_metadata(packet));
    await settle(tester);
    expect(find.byType(MonitoringBindingReaderView), findsNothing);
    expect(h.metadata.metrics, 0);
    expect(h.terminal.opened, isEmpty);
  });
  testWidgets('manual Read rechecks original host/job context and sends no panel request after host revision changed', (tester) async {
    final h = await _open(tester, reverse: true);
    h.projects.jobState = 'failed';
    final packet = await h.reverseFocus();
    h.container.read(routerProvider).go(AppRoutes.monitoring, extra: packet);
    await settle(tester);
    expect(find.byType(MonitoringBindingReaderView), findsOneWidget);
    expect(h.metadata.metrics, 0);
    h.metadata.hostRevision++;
    await tapKey(tester, 'monitoring-binding-load-panel');
    expect(h.metadata.metrics, 0);
    expect(find.byKey(const ValueKey('monitoring-binding-read-error')), findsOneWidget);
    expect(h.projects.saved, isEmpty);
  });
  testWidgets('same-profile Monitoring session replacement retires an original reverse packet before late completion', (tester) async {
    final h = await _open(tester, reverse: true);
    h.projects.jobState = 'failed';
    final packet = await h.reverseFocus();
    h.metadata.pending = Completer<Json>();
    h.container.read(routerProvider).go(AppRoutes.monitoring, extra: packet);
    await settle(tester);
    h.container.invalidate(monitoringSessionProvider);
    await settle(tester);
    h.metadata.pending!.complete(_metadata(packet));
    await settle(tester);
    expect(find.byType(MonitoringBindingReaderView), findsNothing);
    expect(h.metadata.metrics, 0);
    expect(h.projects.saved, isEmpty);
  });

  testWidgets(
    'Terminal existing B survives exact A chooser; no local discovery or connection before explicit Connect',
    (tester) async {
      final h = await _open(tester);
      final b = await h.container.read(terminalTabsProvider.notifier).open(hostB);
      h.container.read(routerProvider).go(AppRoutes.terminal, extra: h.focus(JobContextDestination.sshHost));
      await settle(tester);
      expect(find.byKey(const ValueKey('terminal-context-focus-hint')), findsOneWidget);
      expect(find.byKey(ValueKey('pick-${hostA.name}')), findsOneWidget);
      expect(find.byKey(ValueKey('pick-${hostB.name}')), findsNothing);
      expect(h.terminal.opened, [hostB.id.value]);
      expect(h.terminal.discoveries, 0);
      expect(h.container.read(terminalTabsProvider).tabs.single, same(b));
      await tapKey(tester, 'terminal-context-connect');
      expect(h.terminal.opened, [hostB.id.value, hostA.id.value]);
      expect(h.metadata.metadataCalls, 2);
      expect(h.metadata.metrics, 0);
      expect(h.container.read(terminalTabsProvider).tabs.first, same(b));
    },
  );
  testWidgets('host revision change while Terminal chooser is open denies the explicit Connect', (tester) async {
    final h = await _open(tester);
    h.container.read(routerProvider).go(AppRoutes.terminal, extra: h.focus(JobContextDestination.sshHost));
    await settle(tester);
    h.metadata.hostRevision = 10;
    await tapKey(tester, 'terminal-context-connect');
    expect(h.terminal.opened, isEmpty);
    expect(h.metadata.metadataCalls, 2);
    expect(h.container.read(terminalTabsProvider).tabs, isEmpty);
  });
  testWidgets('leaving Terminal dismisses only its original context chooser without opening a connection', (
    tester,
  ) async {
    final h = await _open(tester);
    h.container.read(routerProvider).go(AppRoutes.terminal, extra: h.focus(JobContextDestination.sshHost));
    await settle(tester);
    h.container.read(routerProvider).go(AppRoutes.monitoring);
    await settle(tester);
    expect(find.byKey(const ValueKey('terminal-picker')), findsNothing);
    expect(h.terminal.opened, isEmpty);
  });
  testWidgets('Docker B remains active while actual A chooser is read-only; standard Disconnect precedes Connect A', (
    tester,
  ) async {
    final h = await _open(tester);
    h.container.read(routerProvider).go(AppRoutes.docker);
    await settle(tester);
    await tapKey(tester, 'docker-connect-${hostB.id.value}');
    final b = tester.widget<DockerWorkspace>(find.byType(DockerWorkspace)).session;
    h.container.read(routerProvider).go(AppRoutes.monitoring);
    await settle(tester);
    h.container.read(routerProvider).go(AppRoutes.docker, extra: h.focus(JobContextDestination.dockerHost));
    await settle(tester);
    expect(h.docker.connected, [hostB.id.value]);
    expect(h.docker.disconnected, isEmpty);
    expect(tester.widget<GlassButton>(find.byKey(ValueKey('docker-connect-${hostA.id.value}'))).onPressed, isNull);
    await tester.tap(find.widgetWithText(GlassButton, 'Close').last);
    await settle(tester);
    expect(tester.widget<DockerWorkspace>(find.byType(DockerWorkspace)).session, same(b));
    await tapKey(tester, 'docker-disconnect');
    await tapKey(tester, 'docker-connect-${hostA.id.value}');
    expect(h.docker.connected, [hostB.id.value, hostA.id.value]);
    expect(h.docker.disconnected, [b.id]);
    expect(h.metadata.metadataCalls, 2);
  });
  testWidgets('late explicit Docker connect after leaving section closes only its returned owned session', (
    tester,
  ) async {
    final h = await _open(tester);
    h.container.read(routerProvider).go(AppRoutes.docker, extra: h.focus(JobContextDestination.dockerHost));
    await settle(tester);
    h.docker.pending = Completer<DockerSessionInfo>();
    await tester.tap(find.byKey(ValueKey('docker-connect-${hostA.id.value}')));
    await tester.pump();
    h.container.read(routerProvider).go(AppRoutes.monitoring);
    await settle(tester);
    h.docker.pending!.complete(
      DockerSessionInfo(id: 'owned-late-A', hostId: hostA.id.value, hostName: hostA.name, engineVersion: 'owned'),
    );
    await settle(tester);
    expect(h.docker.disconnected, ['owned-late-A']);
    h.container.read(routerProvider).go(AppRoutes.docker);
    await settle(tester);
    expect(find.byType(DockerWorkspace), findsNothing);
  });
  testWidgets('Automation A handoff highlights actual tree and preserves dirty B until separate Open and Cancel', (
    tester,
  ) async {
    final h = await _open(tester);
    h.container.read(routerProvider).go(AppRoutes.automation);
    await settle(tester);
    await tapKey(tester, 'automation-project-${_Projects.b.id}');
    await tapKey(tester, 'automation-file-playbook.yml');
    final editor = tester.widget<AnsibleCodeEditor>(find.byType(AnsibleCodeEditor)).controller;
    editor.text = '---\n# UNSAVED B DOCUMENT\n';
    await tester.pump();
    final reads = h.projects.filesRead.length;
    h.container.read(routerProvider).go(AppRoutes.monitoring);
    await settle(tester);
    h.container.read(routerProvider).go(AppRoutes.automation, extra: h.focus(JobContextDestination.ansibleProject));
    await settle(tester);
    expect(find.byKey(ValueKey('automation-context-project-${_Projects.a.id}')), findsOneWidget);
    expect(editor.text, contains('UNSAVED B DOCUMENT'));
    expect(h.projects.filesRead.length, reads);
    await tapKey(tester, 'automation-context-open');
    await tester.tap(find.widgetWithText(TextButton, 'Cancel').last);
    await settle(tester);
    expect(editor.text, contains('UNSAVED B DOCUMENT'));
    expect(h.projects.filesRead.length, reads);
    expect(h.projects.saved, isEmpty);
  });
  testWidgets('changed actual project digest denies Open after dirty Discard without replacing B document', (
    tester,
  ) async {
    final h = await _open(tester);
    h.container.read(routerProvider).go(AppRoutes.automation);
    await settle(tester);
    await tapKey(tester, 'automation-project-${_Projects.b.id}');
    await tapKey(tester, 'automation-file-playbook.yml');
    final editor = tester.widget<AnsibleCodeEditor>(find.byType(AnsibleCodeEditor)).controller;
    editor.text = '---\n# PRESERVE B\n';
    await tester.pump();
    h.container.read(routerProvider).go(AppRoutes.monitoring);
    await settle(tester);
    h.container.read(routerProvider).go(AppRoutes.automation, extra: h.focus(JobContextDestination.ansibleProject));
    await settle(tester);
    h.projects.digest = List.filled(64, 'c').join();
    await tapKey(tester, 'automation-context-open');
    await tester.tap(find.widgetWithText(TextButton, 'Discard').last);
    await settle(tester);
    expect(editor.text, contains('PRESERVE B'));
    expect(find.byKey(const ValueKey('automation-context-error')), findsOneWidget);
    expect(h.projects.textRead, [_Projects.b.id]);
    expect(h.projects.saved, isEmpty);
  });
  testWidgets(
    'explicit project Open rechecks actual revision and digest before replacing B; it does not open a file or run',
    (tester) async {
      final h = await _open(tester);
      h.container.read(routerProvider).go(AppRoutes.automation);
      await settle(tester);
      await tapKey(tester, 'automation-project-${_Projects.b.id}');
      await tapKey(tester, 'automation-file-playbook.yml');
      final editor = tester.widget<AnsibleCodeEditor>(find.byType(AnsibleCodeEditor)).controller;
      editor.text = '---\n# DIRTY EXISTING B\n';
      await tester.pump();
      h.container.read(routerProvider).go(AppRoutes.monitoring);
      await settle(tester);
      h.container.read(routerProvider).go(AppRoutes.automation, extra: h.focus(JobContextDestination.ansibleProject));
      await settle(tester);
      await tapKey(tester, 'automation-context-open');
      await tester.tap(find.widgetWithText(TextButton, 'Discard').last);
      await settle(tester);
      expect(h.projects.filesRead.last, _Projects.a.id);
      expect(h.projects.textRead, [_Projects.b.id]);
      expect(h.projects.saved, isEmpty);
      expect(find.byKey(const ValueKey('automation-context-error')), findsNothing);
      expect(find.byKey(const ValueKey('automation-context-open')), findsNothing);
      expect(find.byKey(const ValueKey('automation-file-playbook.yml')), findsOneWidget);
      expect(h.metadata.metadataCalls, 4);
    },
  );
  testWidgets('project focus highlights A without discarding existing B run composer draft', (tester) async {
    final h = await _open(tester);
    h.container.read(routerProvider).go(AppRoutes.automation);
    await settle(tester);
    await tapKey(tester, 'automation-project-${_Projects.b.id}');
    await tapKey(tester, 'automation-file-playbook.yml');
    await tapKey(tester, 'automation-open-run');
    final original = tester.state(find.byType(AutomationRunDialog, skipOffstage: false));
    h.container.read(routerProvider).go(AppRoutes.monitoring);
    await settle(tester);
    h.container.read(routerProvider).go(AppRoutes.automation, extra: h.focus(JobContextDestination.ansibleProject));
    await settle(tester);
    expect(find.byKey(ValueKey('automation-context-project-${_Projects.a.id}')), findsOneWidget);
    expect(tester.state(find.byType(AutomationRunDialog, skipOffstage: false)), same(original));
    await tapKey(tester, 'automation-workspace-run');
    expect(tester.state(find.byType(AutomationRunDialog)), same(original));
    expect(h.projects.saved, isEmpty);
    expect(h.projects.textRead, [_Projects.b.id]);
  });
}

Json _metadata(JobContextFocus focus) => {
  'version': 1, 'profileId': focus.metadata.profileId, 'notificationScopeId': focus.metadata.notificationScopeId,
  'hostId': focus.metadata.hostId, 'hostRevision': focus.metadata.hostRevision, 'protocol': 'ssh',
  'sourceId': focus.metadata.sourceId, 'sourceRevision': focus.metadata.sourceRevision,
  'bindingId': focus.metadata.bindingId, 'bindingRevision': focus.metadata.bindingRevision,
};
