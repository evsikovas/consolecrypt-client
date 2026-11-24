import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/mock/mock_backend.dart';
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
import 'package:consolecrypt/kubernetes/kube_operations_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:consolecrypt/kubernetes/kube_write_access.dart';
import 'package:flutter/foundation.dart' show precisionErrorTolerance;
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

const _session = KubeSession(id: 'wizard-session', contextId: 'context', serverVersion: '1', production: false);
const _container = DockerContainer(
  id: 'docker-id',
  name: 'web',
  image: 'nginx:1.27',
  state: DockerContainerState.running,
);
const _published = 'docker.io/library/nginx@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef';
const _target = DockerKubeTarget(name: 'web', namespace: 'default', image: _published, port: 80);
const _rs = KubeResourceKind(
  group: 'apps',
  version: 'v1',
  resource: 'replicasets',
  kind: 'ReplicaSet',
  namespaced: true,
);
final _scopeProvider = NotifierProvider<_Scope, KubeScope>(_Scope.new);

class _Scope extends Notifier<KubeScope> {
  @override
  KubeScope build() => const KubeScope('profile', true);
  void lock() => state = const KubeScope('profile', false);
}

class _Kube implements KubeService, KubeBrowserService, KubeManifestService, KubeOperationsService {
  final calls = <(String, bool)>[];
  final closed = <String>[];
  Completer<void>? pending;
  int inventoryCalls = 0;
  int statusReads = 0;
  bool ready = true;
  Map<String, dynamic>? containerStatus;
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
    for (final k in [
      DockerKubeParser.deploymentKind,
      DockerKubeParser.serviceKind,
      DockerKubeParser.namespaceKind,
      _rs,
    ])
      KubeResourceKind(
        group: k.group,
        version: k.version,
        resource: k.resource,
        kind: k.kind,
        namespaced: k.namespaced,
        verbs: const ['create', 'get', 'list'],
      ),
  ];
  Map<String, dynamic> _owner(String kind, String name, String uid) => {
    'apiVersion': 'apps/v1',
    'kind': kind,
    'name': name,
    'uid': uid,
    'controller': true,
  };
  @override
  Future<List<KubeResourceRow>> workloadPods(String sessionId, KubeResourceRef resource) async => [
    if (containerStatus case final status?)
      KubeResourceRow(
        name: 'web-1',
        namespace: resource.namespace,
        uid: 'pod',
        resourceVersion: '2',
        object: {
          'metadata': {
            'namespace': resource.namespace,
            'ownerReferences': [_owner('ReplicaSet', 'web-rs', 'rs')],
          },
          'status': {
            'containerStatuses': [status],
          },
        },
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
    return KubeResourceDetail(
      object: {
        'metadata': {'name': resource.name, 'namespace': resource.namespace, 'uid': 'test-${resource.api.kind}'},
      },
      yaml: '',
    );
  }

  @override
  Future<KubeResourceDetail> getResource(String sessionId, KubeResourceRef resource) async {
    statusReads++;
    if (resource.api.id == _rs.id) {
      return KubeResourceDetail(
        yaml: '',
        object: {
          'metadata': {
            'name': 'web-rs',
            'namespace': resource.namespace,
            'uid': 'rs',
            'ownerReferences': [_owner('Deployment', 'web', 'test-Deployment')],
          },
        },
      );
    }
    return KubeResourceDetail(
      object: {
        'metadata': {'generation': 1, 'name': resource.name, 'namespace': resource.namespace, 'uid': 'test-Deployment'},
        'spec': {'replicas': 1},
        'status': {
          'observedGeneration': 1,
          'replicas': 1,
          'updatedReplicas': 1,
          'readyReplicas': ready ? 1 : 0,
          'availableReplicas': ready ? 1 : 0,
        },
      },
      yaml: '',
    );
  }

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
  List<String> startup = const [],
  String? publishedImage = _published,
  Future<DockerKubeSource> Function()? resolveImage,
  bool writable = true,
  MockBackend? backend,
  Future<String> Function(Credential, String, String)? publishImage,
}) async {
  tester.view.physicalSize = const Size(1200, 1000);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  final provider = AiProviderConfig.draft(AiProviderKind.deepseek);
  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        if (backend != null) appServicesProvider.overrideWithValue(backend.services),
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
            source: DockerKubeSource(
              ports: const [80],
              sourceImage: _container.image,
              image: publishedImage,
              blocker: blocker,
              startup: startup,
            ),
            resolveImage: resolveImage,
            engineName: 'build-host',
            publishImage: publishImage,
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
  if (finder.evaluate().isEmpty || !tester.getRect(finder).overlaps(tester.getRect(find.byType(Scrollable).first))) {
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
  testWidgets('save a reusable registry, publish, and pin its digest in the deployment template', (tester) async {
    final backend = MockBackend(config: const MockConfig.test(seedDemoData: false));
    await backend.debugCreateUnlockedLocalProfile();
    addTearDown(backend.dispose);
    final digest = _published.split('@').last;
    final image = 'registry.example.org/team/web@$digest';
    final submissions = <String>[];
    final scope = await _pump(
      tester,
      _Kube(),
      _Ai(),
      backend: backend,
      publishImage: (credential, repo, tag) async {
        submissions.add('${credential.registry}/$repo:$tag');
        return image;
      },
    );
    await _templateAndAck(tester);
    await _tap(tester, 'registry-add');
    await tester.enterText(find.byKey(const ValueKey('registry-address')), 'registry.example.org');
    await tester.enterText(find.byKey(const ValueKey('registry-name')), 'Build registry');
    await tester.enterText(find.byKey(const ValueKey('registry-user')), 'builder');
    await tester.enterText(
      find.descendant(of: find.byKey(const ValueKey('registry-token')), matching: find.byType(EditableText)),
      'fixture-token',
    );
    await _settle(tester);
    await _tap(tester, 'registry-save');
    final saved = scope.read(credentialsProvider).requireValue.single;
    expect(saved.registry, 'registry.example.org');
    expect(saved.username, 'builder');
    await tester.enterText(find.byKey(const ValueKey('registry-repo')), 'team/web');
    await tester.enterText(find.byKey(const ValueKey('registry-tag')), 'v1');
    await _tap(tester, 'registry-publish');
    expect(submissions, ['registry.example.org/team/web:v1']);
    String field(String key) => tester.widget<TextField>(find.byKey(ValueKey('docker-kube-$key'))).controller!.text;
    expect(field('image'), image);
    expect(field('yaml'), isEmpty);
    await _tap(tester, 'docker-kube-template');
    expect(field('yaml'), contains(image));
    expect(field('yaml'), isNot(contains('fixture-token')));
    await tester.pumpWidget(const SizedBox.shrink());
    await _pump(tester, _Kube(), _Ai(), backend: backend, publishImage: (_, _, _) async => image);
    await _tap(tester, 'registry-picker-null');
    await tester.tap(find.text('Build registry · registry.example.org').last);
    await _settle(tester);
    expect(find.byKey(const ValueKey('registry-publish')), findsOneWidget);
    await tester.pumpWidget(const SizedBox.shrink());
  });

  testWidgets('a failed push keeps the previous image and reviewed manifest', (tester) async {
    final backend = MockBackend(config: const MockConfig.test(seedDemoData: false));
    await backend.debugCreateUnlockedLocalProfile();
    addTearDown(backend.dispose);
    final login = await backend.inventory.createPasswordCredential(
      name: 'Test Hub',
      username: 'builder',
      registry: 'docker.io',
      password: SecretText('fixture'),
    );
    await _pump(tester, _Kube(), _Ai(), backend: backend, publishImage: (_, _, _) async => throw StateError('fixture'));
    await _templateAndAck(tester);
    String field(String key) => tester.widget<TextField>(find.byKey(ValueKey('docker-kube-$key'))).controller!.text;
    final yaml = field('yaml');
    await _tap(tester, 'registry-picker-null');
    await tester.tap(find.text('${login.name} · docker.io').last);
    await _settle(tester);
    await _tap(tester, 'registry-publish');
    expect(field('image'), _published);
    expect(field('yaml'), yaml);
    expect(find.byKey(const ValueKey('docker-kube-error')), findsOneWidget);
    await tester.pumpWidget(const SizedBox.shrink());
  });

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
    kube.ready = false;
    await _tap(tester, 'docker-kube-deploy');
    expect(kube.calls.where((c) => !c.$2), [('Deployment', false), ('Service', false)]);
    expect(find.text('Deployment default/web'), findsOneWidget);
    expect(find.text('Service default/web'), findsOneWidget);
    expect(find.textContaining('Waiting for the Deployment'), findsOneWidget);
    kube.ready = true;
    await tester.pump(const Duration(seconds: 5));
    await _settle(tester);
    expect(find.text('Deployment is ready'), findsOneWidget);
    final reads = kube.statusReads;
    await tester.pump(const Duration(seconds: 6));
    expect(kube.statusReads, reads);
    await tester.pumpWidget(const SizedBox.shrink());
    expect(kube.closed, [_session.id]);
  });

  testWidgets('AI receives nothing before review; generation never writes to cluster', (tester) async {
    final kube = _Kube(), ai = _Ai();
    const startup = ['nginx', '-g', 'daemon off;'];
    await _pump(tester, kube, ai, startup: startup);
    await _connect(tester);
    Rect bounds(String key) => tester.getRect(find.byKey(ValueKey('docker-kube-$key')));
    for (final (upper, lower) in [
      ('name', 'image'),
      ('image', 'port'),
      ('provider', 'generate'),
      ('generate', 'yaml'),
    ]) {
      expect(
        bounds(lower).top - bounds(upper).bottom,
        greaterThanOrEqualTo(8 - precisionErrorTolerance),
        reason: '$upper / $lower',
      );
    }
    final yamlField = tester.widget<TextField>(find.byKey(const ValueKey('docker-kube-yaml')));
    expect(find.text('AI provider'), findsOneWidget);
    expect(yamlField.decoration!.labelText, 'Deployment manifest (YAML)');
    expect(tester.widget<TextField>(find.byKey(const ValueKey('docker-kube-image'))).controller!.text, _target.image);
    await _tap(tester, 'docker-kube-generate');
    expect(ai.prompts, isEmpty);
    expect(find.textContaining('daemon off;'), findsNothing);
    final send = find.byKey(const ValueKey('docker-kube-send-ai'));
    expect(send.hitTestable(), findsOneWidget);
    await tester.tap(send);
    await _settle(tester);
    await _parse(tester);
    expect(ai.prompts, hasLength(1));
    expect(yamlField.controller!.text, contains('daemon off;'));
    expect(yamlField.controller!.text, contains('kind: Deployment'));
    expect(ai.prompts.single, contains(_target.image));
    expect(ai.prompts.single, isNot(contains('daemon off;')));
    expect(ai.prompts.single, isNot(contains('test-cluster')));
    expect(kube.calls, isEmpty);
    expect(_enabled(tester, 'docker-kube-deploy'), isFalse);
    await tester.pumpWidget(const SizedBox.shrink());
  });

  testWidgets('the wizard preselects a confirmed digest from the image lookup', (tester) async {
    await _pump(
      tester,
      _Kube(),
      _Ai(),
      publishedImage: null,
      resolveImage: () async => const DockerKubeSource(ports: [80], sourceImage: 'nginx:1.27', image: _published),
    );
    expect(tester.widget<TextField>(find.byKey(const ValueKey('docker-kube-image'))).controller!.text, _published);
    await tester.pumpWidget(const SizedBox.shrink());
  });

  testWidgets('a lookup failure leaves local Docker tags out of generated manifests; manual private images work', (
    tester,
  ) async {
    final kube = _Kube(), ai = _Ai();
    await _pump(
      tester,
      kube,
      ai,
      publishedImage: null,
      resolveImage: () async => throw StateError('registry unavailable'),
    );
    final image = find.byKey(const ValueKey('docker-kube-image'));
    final yaml = find.byKey(const ValueKey('docker-kube-yaml'));
    await _connect(tester);
    expect(tester.widget<TextField>(image).controller!.text, isEmpty);
    expect(find.textContaining('Could not confirm a registry digest'), findsOneWidget);
    await tester.ensureVisible(image);
    await tester.enterText(image, 'docker.io/library/nginx:1.27');
    await _tap(tester, 'docker-kube-template');
    expect(tester.widget<TextField>(yaml).controller!.text, isEmpty);
    await tester.ensureVisible(image);
    await tester.enterText(image, 'registry.example.org/team/web:v1');
    final secret = find.byKey(const ValueKey('docker-kube-pull-secret'));
    await tester.ensureVisible(secret);
    await tester.enterText(secret, 'registry-login');
    await _tap(tester, 'docker-kube-template');
    final manifest = tester.widget<TextField>(yaml).controller!.text;
    expect(manifest, contains('registry.example.org/team/web:v1'));
    expect(manifest, contains('registry-login'));
    expect(kube.calls, isEmpty);
    expect(ai.prompts, isEmpty);
    await tester.pumpWidget(const SizedBox.shrink());
  });

  testWidgets('local-only tag stays empty; a late digest lookup preserves the manually entered image', (tester) async {
    final kube = _Kube();
    final pending = Completer<DockerKubeSource>();
    await _pump(tester, kube, _Ai(), publishedImage: null, resolveImage: () => pending.future);
    final field = find.byKey(const ValueKey('docker-kube-image'));
    expect(tester.widget<TextField>(field).controller!.text, isEmpty);
    await _connect(tester);
    await tester.ensureVisible(field);
    await tester.enterText(field, 'registry.example.org/nginx:published');
    pending.complete(const DockerKubeSource(ports: [80], sourceImage: 'nginx:1.27', image: _published));
    await _settle(tester);
    expect(tester.widget<TextField>(field).controller!.text, 'registry.example.org/nginx:published');
    await _tap(tester, 'docker-kube-template');
    expect(
      tester.widget<TextField>(find.byKey(const ValueKey('docker-kube-yaml'))).controller!.text,
      contains('registry.example.org/nginx:published'),
    );
    expect(kube.calls, isEmpty);
    await tester.pumpWidget(const SizedBox.shrink());
  });

  testWidgets('a failing rollout shows the cause and keeps it through a transient pending status', (tester) async {
    final kube = _Kube()..ready = false;
    await _pump(tester, kube, _Ai());
    await _templateAndAck(tester);
    await _tap(tester, 'docker-kube-validate');
    await _parse(tester);
    kube.containerStatus = {
      'image': _published,
      'state': {
        'waiting': {'reason': 'ErrImagePull', 'message': 'pull access denied: PRIVATE-ENGINE-OUTPUT'},
      },
    };
    await _tap(tester, 'docker-kube-deploy');
    expect(find.textContaining('ErrImagePull'), findsOneWidget);
    expect(find.textContaining(_published), findsOneWidget);
    expect(find.textContaining('The registry denied access'), findsOneWidget);
    expect(find.textContaining('PRIVATE-ENGINE-OUTPUT'), findsNothing);
    expect(find.byKey(const ValueKey('docker-kube-open-deployment')), findsOneWidget);
    kube.containerStatus = {
      'restartCount': 51,
      'state': {
        'waiting': {'reason': 'CrashLoopBackOff'},
      },
      'lastState': {
        'terminated': {'reason': 'Completed', 'exitCode': 0, 'message': 'PRIVATE-ENGINE-OUTPUT'},
      },
    };
    await tester.pump(const Duration(seconds: 5));
    await _settle(tester);
    expect(find.textContaining('CrashLoopBackOff · Completed'), findsOneWidget);
    expect(find.textContaining('51'), findsOneWidget);
    expect(find.textContaining('foreground server'), findsOneWidget);
    expect(find.textContaining('PRIVATE-ENGINE-OUTPUT'), findsNothing);
    expect(find.byKey(const ValueKey('docker-kube-deploy')), findsNothing);
    kube.containerStatus = null;
    await tester.pump(const Duration(seconds: 5));
    await _settle(tester);
    expect(find.textContaining('Last observed problem ·'), findsOneWidget);
    kube.ready = true;
    await tester.pump(const Duration(seconds: 5));
    await _settle(tester);
    expect(find.textContaining('Last observed problem'), findsNothing);
    expect(find.text('Deployment is ready'), findsOneWidget);
    expect(kube.calls.length, 4);
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
    expect(find.byKey(const ValueKey('docker-kube-send-ai')).hitTestable(), findsOneWidget);
    scope.read(_scopeProvider.notifier).lock();
    await _settle(tester);
    expect(find.byKey(const ValueKey('docker-kube-send-ai')), findsNothing);
    expect(ai.prompts, isEmpty);
    expect(kube.calls, isEmpty);
    await tester.pumpWidget(const SizedBox.shrink());
  });
}
