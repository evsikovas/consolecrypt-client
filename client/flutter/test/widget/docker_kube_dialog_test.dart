import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/ai_service.dart';
import 'package:consolecrypt/docker/docker_kube_dialog.dart';
import 'package:consolecrypt/docker/docker_kube_plan.dart';
import 'package:consolecrypt/docker/docker_providers.dart';
import 'package:consolecrypt/docker/docker_service.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_manifest_service.dart';
import 'package:consolecrypt/kubernetes/kube_mutation_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:consolecrypt/kubernetes/kube_write_access.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

const _session = KubeSession(id: 'wizard-session', contextId: 'context', serverVersion: '1', production: false);
const _container = DockerContainer(
  id: 'docker-id',
  name: 'web',
  image: 'docker.io/library/nginx:1.27',
  state: DockerContainerState.running,
);
const _target = DockerKubeTarget(name: 'web', namespace: 'default', image: 'docker.io/library/nginx:1.27', port: 80);
final _scopeProvider = NotifierProvider<_Scope, KubeScope>(_Scope.new);

class _Scope extends Notifier<KubeScope> {
  @override
  KubeScope build() => const KubeScope('profile', true);
  void lock() => state = const KubeScope('profile', false);
}

class _Kube implements KubeService, KubeBrowserService, KubeManifestService {
  final calls = <(String, bool)>[];
  final closed = <String>[];
  Completer<void>? pending;
  int inventoryCalls = 0;
  @override
  Future<List<KubeConnection>> inventory() async {
    inventoryCalls++;
    return [
      const KubeConnection(
        id: 'context',
        name: 'test-cluster',
        clusterName: 'cluster',
        server: 'https://cluster',
        namespace: 'default',
        authentication: 'token',
        production: false,
      ),
    ];
  }

  @override
  Future<KubeSession> connect(String contextId) async => _session;
  @override
  Future<void> disconnect(String sessionId) async => closed.add(sessionId);
  @override
  Future<List<String>> namespaces(String sessionId) async => ['default'];
  @override
  Future<List<KubeResourceKind>> discover(String sessionId) async => [
    for (final k in [DockerKubeParser.deploymentKind, DockerKubeParser.serviceKind, DockerKubeParser.namespaceKind])
      KubeResourceKind(
        group: k.group,
        version: k.version,
        resource: k.resource,
        kind: k.kind,
        namespaced: k.namespaced,
        verbs: const ['create', 'get', 'list'],
      ),
  ];
  @override
  Future<KubeResourceDetail> createResource(
    String sessionId,
    KubeResourceRef resource,
    SecretText manifest, {
    bool dryRun = true,
  }) async {
    expect(sessionId, _session.id);
    expect(resource.api.verbs, contains('create'));
    expect(manifest.expose(), contains(resource.name));
    manifest.wipe(); // Match the native adapter's consuming boundary.
    calls.add((resource.api.kind, dryRun));
    if (pending != null) await pending!.future;
    return const KubeResourceDetail(object: {}, yaml: '');
  }

  @override
  Future<KubeResourceDetail> getResource(String sessionId, KubeResourceRef resource) async => const KubeResourceDetail(
    object: {
      'metadata': {'generation': 1},
      'status': {'observedGeneration': 1, 'updatedReplicas': 1, 'readyReplicas': 1, 'availableReplicas': 1},
    },
    yaml: '',
  );
  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

class _Ai implements AiService {
  final prompts = <String>[];
  @override
  Stream<AiStreamEvent> chat({
    required ObjectId providerId,
    required List<ChatMessage> history,
    AiContextSelection context = AiContextSelection.none,
  }) async* {
    prompts.add(history.single.content);
    yield AiCompleted(
      fullText: _target.template,
      report: const SanitizationReport(profile: PrivacyProfile.strict, redactions: 0, remote: true),
    );
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

Future<void> _settle(WidgetTester tester) async {
  for (var i = 0; i < 4; i++) {
    await tester.pump(const Duration(milliseconds: 100));
  }
}

Future<ProviderContainer> _pump(
  WidgetTester tester,
  _Kube kube,
  _Ai ai, {
  DockerKubeBlocker? blocker,
  bool writable = true,
}) async {
  tester.view.physicalSize = const Size(1200, 1000);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  final provider = AiProviderConfig.draft(AiProviderKind.deepseek);
  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        kubeServiceProvider.overrideWithValue(kube),
        kubeScopeProvider.overrideWith((ref) => ref.watch(_scopeProvider)),
        dockerScopeProvider.overrideWithValue(const DockerScope('profile', true)),
        kubeWriteAccessProvider(_session).overrideWith((ref) => KubeWriteAccess(production: !writable)),
        aiProvidersProvider.overrideWith((ref) => Stream.value([provider])),
        aiServiceProvider.overrideWithValue(ai),
      ],
      child: MaterialApp(
        locale: const Locale('en'),
        supportedLocales: AppLocalizations.supportedLocales,
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        home: Scaffold(
          body: DockerKubeDialog(
            container: _container,
            source: DockerKubeSource(ports: const [80], blocker: blocker),
          ),
        ),
      ),
    ),
  );
  await _settle(tester);
  return ProviderScope.containerOf(tester.element(find.byType(Scaffold)));
}

Future<void> _tap(WidgetTester tester, String key) async {
  final finder = find.byKey(ValueKey(key));
  if (finder.evaluate().isEmpty || !tester.getRect(finder).overlaps(tester.getRect(find.byType(ListView).first))) {
    await tester.scrollUntilVisible(finder, 200, scrollable: find.byType(Scrollable).first);
  }
  await Scrollable.ensureVisible(tester.element(finder), alignment: 0.5);
  await _settle(tester);
  await tester.tap(finder);
  await _settle(tester);
}

Future<void> _connect(WidgetTester tester) async {
  await _tap(tester, 'docker-kube-context');
  await tester.tap(find.text('test-cluster').last);
  await _settle(tester);
}

Future<void> _templateAndAck(WidgetTester tester) async {
  await _connect(tester);
  await _tap(tester, 'docker-kube-template');
  await _tap(tester, 'docker-kube-acknowledge');
  expect(tester.widget<TextField>(find.byKey(const ValueKey('docker-kube-yaml'))).controller!.text, isNotEmpty);
  expect(tester.widget<CheckboxListTile>(find.byKey(const ValueKey('docker-kube-acknowledge'))).value, isTrue);
}

Future<void> _parse(WidgetTester tester) async {
  // YAML validation uses an actual expendable isolate, outside fake time.
  await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 300)));
  await _settle(tester);
}

bool _enabled(WidgetTester tester, String key) =>
    tester.widget<GlassButton>(find.byKey(ValueKey(key))).onPressed != null;

void main() {
  testWidgets('experimental wizard requires dry-run, invalidates edits and creates both resources', (tester) async {
    final kube = _Kube();
    await _pump(tester, kube, _Ai());
    expect(find.textContaining('Experimental'), findsWidgets);
    expect(_enabled(tester, 'docker-kube-deploy'), isFalse);
    await _templateAndAck(tester);
    await _tap(tester, 'docker-kube-validate');
    await _parse(tester);
    expect(kube.calls, [('Deployment', true), ('Service', true)]);
    expect(_enabled(tester, 'docker-kube-deploy'), isTrue);
    final yaml = find.byKey(const ValueKey('docker-kube-yaml'));
    await tester.ensureVisible(yaml);
    await tester.enterText(yaml, '${_target.template}\n');
    await _settle(tester);
    expect(_enabled(tester, 'docker-kube-deploy'), isFalse);
    await _tap(tester, 'docker-kube-validate');
    await _parse(tester);
    await _tap(tester, 'docker-kube-deploy');
    expect(kube.calls.where((c) => !c.$2), [('Deployment', false), ('Service', false)]);
    await tester.scrollUntilVisible(
      find.byKey(const ValueKey('docker-kube-status')),
      300,
      scrollable: find.byType(Scrollable).first,
    );
    expect(find.text('Deployment is ready'), findsOneWidget);
    await tester.pumpWidget(const SizedBox.shrink());
    expect(kube.closed, [_session.id]);
  });

  testWidgets('AI receives nothing before review; generation never writes to cluster', (tester) async {
    final kube = _Kube(), ai = _Ai();
    await _pump(tester, kube, ai);
    await _connect(tester);
    await _tap(tester, 'docker-kube-generate');
    expect(ai.prompts, isEmpty);
    await _tap(tester, 'docker-kube-send-ai');
    await _parse(tester);
    expect(ai.prompts, hasLength(1));
    expect(ai.prompts.single, contains(_target.image));
    expect(ai.prompts.single, isNot(contains('test-cluster')));
    expect(kube.calls, isEmpty);
    expect(_enabled(tester, 'docker-kube-deploy'), isFalse);
    await tester.pumpWidget(const SizedBox.shrink());
  });

  testWidgets('namespace creation is explicit and separate from app validation', (tester) async {
    final kube = _Kube();
    await _pump(tester, kube, _Ai());
    await _connect(tester);
    await _tap(tester, 'docker-kube-new-namespace');
    await tester.enterText(find.byKey(const ValueKey('docker-kube-namespace-name')), 'new-space');
    await _settle(tester);
    expect(kube.calls, isEmpty);
    expect(_enabled(tester, 'docker-kube-validate'), isFalse);
    await _tap(tester, 'docker-kube-create-namespace');
    expect(kube.calls, [('Namespace', true), ('Namespace', false)]);
    await tester.scrollUntilVisible(
      find.byKey(const ValueKey('docker-kube-namespace-created')),
      -200,
      scrollable: find.byType(Scrollable).first,
    );
    expect(find.text('Namespace created. It will remain if you cancel.'), findsOneWidget);
    await tester.pumpWidget(const SizedBox.shrink());
  });

  testWidgets('lock during dry-run prevents the next request and disconnects owned session', (tester) async {
    final kube = _Kube();
    final scope = await _pump(tester, kube, _Ai());
    await _templateAndAck(tester);
    kube.pending = Completer<void>();
    await _tap(tester, 'docker-kube-validate');
    await _parse(tester);
    expect(kube.calls, [('Deployment', true)]);
    scope.read(_scopeProvider.notifier).lock();
    await _settle(tester);
    kube.pending!.complete();
    await _settle(tester);
    expect(kube.calls, [('Deployment', true)]);
    expect(kube.closed, [_session.id]);
    await tester.pumpWidget(const SizedBox.shrink());
  });

  testWidgets('a container with mounted files shows only guidance and a close button', (tester) async {
    final kube = _Kube();
    await _pump(tester, kube, _Ai(), blocker: DockerKubeBlocker.mounts);
    final dialog = find.byType(GlassDialog);
    final l = tester.element(dialog).l10n;
    expect(find.text(l.dockerKubeMountsBlocked), findsOneWidget);
    expect(find.text(l.commonClose), findsOneWidget);
    expect(tester.getSize(dialog).height, lessThan(450));
    expect(find.byKey(const ValueKey('docker-kube-context')), findsNothing);
    expect(find.byKey(const ValueKey('docker-kube-validate')), findsNothing);
    expect(find.byKey(const ValueKey('docker-kube-deploy')), findsNothing);
    expect(kube.inventoryCalls, 0);
    expect(kube.calls, isEmpty);
    await tester.pumpWidget(const SizedBox.shrink());
  });

  testWidgets('production without a write window cannot validate or deploy', (tester) async {
    final kube = _Kube();
    await _pump(tester, kube, _Ai(), writable: false);
    await _templateAndAck(tester);
    expect(_enabled(tester, 'docker-kube-validate'), isFalse);
    expect(_enabled(tester, 'docker-kube-deploy'), isFalse);
    expect(kube.calls, isEmpty);
    await tester.pumpWidget(const SizedBox.shrink());
  });

  testWidgets('lock clears the pending AI review without sending the template', (tester) async {
    final kube = _Kube(), ai = _Ai();
    final scope = await _pump(tester, kube, ai);
    await _connect(tester);
    await _tap(tester, 'docker-kube-generate');
    await tester.scrollUntilVisible(
      find.byKey(const ValueKey('docker-kube-send-ai')),
      200,
      scrollable: find.byType(Scrollable).first,
    );
    scope.read(_scopeProvider.notifier).lock();
    await _settle(tester);
    expect(find.byKey(const ValueKey('docker-kube-send-ai')), findsNothing);
    expect(ai.prompts, isEmpty);
    expect(kube.calls, isEmpty);
    await tester.pumpWidget(const SizedBox.shrink());
  });
}
