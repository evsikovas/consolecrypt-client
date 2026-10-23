// Website guide fixtures for the Kubernetes section: a synthetic cluster
// ("shop-staging" at k8s.example.test) with demo Pods, logs, a port
// forward, a rollout history and a scripted AI answer. Nothing connects to
// a cluster, runs kubectl or reaches an AI provider.
import 'dart:async';
import 'dart:typed_data';

import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/terminal_service.dart';
import 'package:consolecrypt/kubernetes/kube_ai_service.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_forward_service.dart';
import 'package:consolecrypt/kubernetes/kube_mutation_service.dart';
import 'package:consolecrypt/kubernetes/kube_rollout_service.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';

const guideKubeContext = 'shop-staging';
const guideKubePod = 'payments-7f5d48b9c8-x2nr5';

const _pods = KubeResourceKind(
  group: '',
  version: 'v1',
  resource: 'pods',
  kind: 'Pod',
  namespaced: true,
  verbs: ['get', 'list', 'watch', 'patch', 'delete'],
);
const _deployments = KubeResourceKind(
  group: 'apps',
  version: 'v1',
  resource: 'deployments',
  kind: 'Deployment',
  namespaced: true,
  verbs: ['get', 'list', 'watch', 'patch'],
);

const guideKubeConnections = [
  KubeConnection(
    id: guideKubeContext,
    name: guideKubeContext,
    clusterName: 'staging',
    server: 'https://k8s.example.test',
    namespace: 'shop',
    authentication: 'token',
    production: false,
    tags: ['staging'],
  ),
  KubeConnection(
    id: 'shop-prod',
    name: 'shop-prod',
    clusterName: 'production',
    server: 'https://k8s-prod.example.test',
    namespace: 'shop',
    authentication: 'oidc',
    production: true,
    tags: ['prod'],
  ),
];

KubeResourceRow _row(String name, {String status = 'Running', int ageHours = 30}) => KubeResourceRow(
  name: name,
  uid: name,
  resourceVersion: '4210',
  namespace: 'shop',
  status: status,
  createdAt: DateTime.utc(2026, 10, 21, 8).subtract(Duration(hours: ageHours)),
);

final class GuideKubeService
    implements
        KubeService,
        KubeBrowserService,
        KubeWatchService,
        KubeLogService,
        KubeMutationService,
        KubeForwardService,
        KubeRolloutService,
        KubeAiService {
  GuideKubeService({required this.en});

  final bool en;
  final _watches = <StreamController<KubeWatchEvent>>[];
  final _forwards = <KubeForward>[];

  static const session = KubeSession(
    id: 'guide-kube-session',
    contextId: guideKubeContext,
    serverVersion: 'v1.32.1',
    production: false,
  );

  Future<void> dispose() async {
    for (final w in _watches) {
      await w.close();
    }
  }

  // KubeService ---------------------------------------------------------------
  @override
  Stream<void> changes() => const Stream.empty();
  @override
  Future<List<KubeConnection>> inventory() async => guideKubeConnections;
  @override
  Future<KubeImportPreview> preview(SecretText yaml, {String? sourceFile}) async {
    yaml.wipe();
    return importPreview;
  }

  @override
  Future<KubeImportPreview> previewFile(String path) async => importPreview;
  @override
  Future<void> discard(String ticket) async {}
  @override
  Future<void> commit(String ticket, List<KubeImportChoice> choices) async {}
  @override
  Future<void> deleteContext(String id) async {}

  KubeImportPreview get importPreview => KubeImportPreview(
    ticket: 'guide-ticket',
    expiresAt: DateTime.now().toUtc().add(const Duration(minutes: 10)),
    entries: const [
      KubeImportEntry(
        index: 0,
        name: 'shop-dev',
        server: 'https://k8s-dev.example.test',
        namespace: 'shop',
        userName: 'dev-user',
        authentication: 'token',
      ),
      KubeImportEntry(
        index: 1,
        name: 'analytics',
        server: 'https://k8s-analytics.example.test',
        namespace: 'default',
        userName: 'analyst',
        authentication: 'client_certificate',
        pendingFiles: ['/Users/demo/.kube/certs/analyst.crt'],
      ),
    ],
  );

  // Browser -------------------------------------------------------------------
  @override
  Future<KubeSession> connect(String contextId) async => session;
  @override
  Future<void> disconnect(String sessionId) async {}
  @override
  Future<List<KubeResourceKind>> discover(String sessionId) async => const [_pods, _deployments];
  @override
  Future<List<String>> namespaces(String sessionId) async => const ['shop', 'default', 'kube-system'];
  @override
  Future<KubeResourcePage> listResources(
    String sessionId,
    KubeResourceKind kind, {
    String? namespace,
    String? labelSelector,
    String? continueToken,
    int limit = 200,
  }) async => KubeResourcePage(
    resourceVersion: '4210',
    items: kind.resource == 'deployments'
        ? [_row('web'), _row('api'), _row('payments', status: '0/1'), _row('worker')]
        : [
            _row('web-6b9f4c6d87-jl2cm'),
            _row('web-6b9f4c6d87-mv7qn'),
            _row('api-75d8f9d4c6-q9zrw'),
            _row('worker-5bf64697d8-r2m6v'),
            _row(guideKubePod, status: 'CrashLoopBackOff', ageHours: 2),
            _row('orders-db-0', ageHours: 200),
            _row('redis-0', ageHours: 200),
          ],
  );

  @override
  Future<KubeResourceDetail> getResource(String sessionId, KubeResourceRef resource) async => KubeResourceDetail(
    object: {
      'metadata': {
        'name': resource.name,
        'namespace': 'shop',
        'uid': '${resource.name}-uid',
        'resourceVersion': '4210',
        'labels': {'app': resource.name.split('-').first},
      },
      'spec': {
        'containers': [
          {
            'name': 'app',
            'image': 'registry.example.test/shop/${resource.name.split('-').first}:2.4.1',
            'ports': [
              {'containerPort': 8080, 'protocol': 'TCP'},
            ],
          },
        ],
      },
      'status': {'phase': 'Running'},
    },
    yaml:
        'apiVersion: v1\nkind: Pod\nmetadata:\n  name: ${resource.name}\n  namespace: shop\n'
        '  labels:\n    app: ${resource.name.split('-').first}\nspec:\n  containers:\n  - name: app\n'
        '    image: registry.example.test/shop/${resource.name.split('-').first}:2.4.1\n'
        '    ports:\n    - containerPort: 8080\n',
    containers: const ['app'],
  );

  String get _log => [
    '2026-10-21T08:14:02Z INFO  payments starting version=2.4.1',
    '2026-10-21T08:14:02Z INFO  connecting to orders-db.shop.svc:5432',
    '2026-10-21T08:14:07Z WARN  database not ready, retrying in 5s',
    '2026-10-21T08:14:12Z WARN  database not ready, retrying in 5s',
    '2026-10-21T08:14:17Z ERROR missing environment variable PAYMENTS_API_URL',
    '2026-10-21T08:14:17Z INFO  shutting down',
  ].join('\n');

  @override
  Future<String> logs(
    String sessionId,
    KubeResourceRef resource, {
    String? container,
    bool previous = false,
    bool timestamps = true,
    int tailLines = 500,
  }) async => _log;

  @override
  Stream<String> followLogs(
    String sessionId,
    KubeResourceRef resource, {
    String? container,
    bool previous = false,
    bool timestamps = true,
    int tailLines = 500,
  }) => Stream.value(_log);

  @override
  Stream<KubeWatchEvent> watchResources(
    String sessionId,
    KubeResourceKind kind, {
    String? namespace,
    String? labelSelector,
    required String resourceVersion,
  }) {
    final c = StreamController<KubeWatchEvent>();
    _watches.add(c);
    return c.stream;
  }

  // Mutations (dry run only in captures) -------------------------------------
  @override
  Future<KubeWriteAccess> writeAccess(String sessionId) async => const KubeWriteAccess(production: false);
  @override
  Future<KubeWriteAccess> requestWriteWindow(String sessionId, String typedContextName) async =>
      const KubeWriteAccess(production: false);
  @override
  Future<void> closeWriteWindow(String sessionId) async {}
  @override
  Future<KubeResourceDetail> applyYaml(
    String sessionId,
    KubeResourceRef resource,
    SecretText manifest, {
    bool dryRun = true,
    bool force = false,
  }) async {
    final yaml = manifest.expose();
    manifest.wipe();
    if (!dryRun) throw StateError('Guide capture never applies YAML.'); // l10n-ignore: diagnostic
    return KubeResourceDetail(object: const {}, yaml: yaml);
  }

  @override
  Future<void> deleteResource(
    String sessionId,
    KubeResourceRef resource, {
    required String uid,
    required String resourceVersion,
  }) async => throw StateError('Guide capture never deletes.'); // l10n-ignore: diagnostic

  // Port forwarding -------------------------------------------------------------
  @override
  Future<KubeForward> startForward(
    String sessionId, {
    required String namespace,
    required String pod,
    required int remotePort,
    int localPort = 0,
  }) async {
    final forward = KubeForward(
      id: 'guide-forward',
      sessionId: sessionId,
      contextName: guideKubeContext,
      namespace: namespace,
      pod: pod,
      remotePort: remotePort,
      address: '127.0.0.1:18080',
      state: 'running',
    );
    _forwards.add(forward);
    return forward;
  }

  @override
  Future<List<KubeForward>> forwards() async => [..._forwards];
  @override
  Future<void> stopForward(String sessionId, String forwardId) async => _forwards.removeWhere((f) => f.id == forwardId);

  // Rollout ---------------------------------------------------------------------
  @override
  Future<List<KubeJson>> rolloutHistory(String sessionId, KubeResourceRef resource) async => [
    {
      'revision': 3,
      'name': 'payments-6c8d9b7f44',
      'template': {'image': 'registry.example.test/shop/payments:2.3.9'},
    },
    {
      'revision': 4,
      'name': 'payments-7f5d48b9c8',
      'template': {'image': 'registry.example.test/shop/payments:2.4.1'},
    },
  ];
  @override
  Future<KubeJson> rolloutDiff(String sessionId, KubeResourceRef resource, int before, int after) async => {
    'changes': [
      {
        'path': '/spec/template/spec/containers/0/image',
        'before': 'registry.example.test/shop/payments:2.3.9',
        'after': 'registry.example.test/shop/payments:2.4.1',
      },
      {'path': '/spec/template/spec/containers/0/env/2', 'before': 'PAYMENTS_API_URL', 'after': null},
    ],
    'truncated': false,
  };

  // AI (scripted local answer) ------------------------------------------------
  @override
  Future<KubeAiReview> prepareKubeAi(
    String sessionId, {
    String? providerId,
    required String question,
    KubeResourceRef? selected,
    bool configMapValues = false,
  }) async => const KubeAiReview(
    ticket: 'guide-ai',
    providerName: 'Ollama (local)',
    contextName: guideKubeContext,
    external: false,
    toolSupport: true,
  );

  @override
  Stream<KubeAiEvent> askKubeAi(String sessionId, KubeAiReview review, {required bool approveExternal}) =>
      Stream.fromIterable([
        const KubeAiEvent('context', {'tool': 'logs', 'content': 'payments: 6 lines', 'truncated': false}),
        const KubeAiEvent('context', {'tool': 'events', 'content': 'Back-off restarting failed container'}),
        KubeAiEvent('answer', {
          'content': en
              ? 'The Pod exits right after start: the log says **PAYMENTS_API_URL** is missing. '
                    'Revision 4 of the Deployment removed this variable. Restore it in the Deployment '
                    'or roll back to revision 3, then watch the Pod become Ready.'
              : 'Pod завершается сразу после старта: в логе нет переменной **PAYMENTS_API_URL**. '
                    'Ревизия 4 Deployment удалила эту переменную. Верните её в Deployment '
                    'или откатитесь на ревизию 3, затем проверьте, что Pod стал Ready.',
        }),
        const KubeAiEvent('done', {}),
      ]);

  @override
  Future<void> discardKubeAi(String sessionId, String ticket) async {}
  @override
  Future<void> revokeKubeAi(String sessionId) async {}
}

/// Pod shell for the guide: a terminal session with no process behind it.
final class GuideKubeTerminal implements TerminalService, KubernetesTerminalService {
  @override
  Future<TerminalSessionHandle> openKubernetes({
    required KubernetesTerminalTarget target,
    required TerminalSize size,
  }) async => TerminalSessionHandle(
    id: TerminalSessionId.generate(),
    output: const Stream.empty(),
    events: Stream.value(const TerminalStateChanged(SessionConnectionState.connected)),
  );

  @override
  Future<TerminalSessionHandle> open({required ObjectId hostId, required TerminalSize size}) =>
      throw StateError('Guide Pod terminal only.'); // l10n-ignore: diagnostic
  @override
  Future<void> write(TerminalSessionId id, Uint8List data) async {}
  @override
  Future<void> resize(TerminalSessionId id, TerminalSize size) async {}
  @override
  Future<void> close(TerminalSessionId id) async {}
  @override
  Future<void> reconnect(TerminalSessionId id) async {}
  @override
  Future<void> answerHostKey(TerminalSessionId id, HostKeyDecision decision) async {}
  @override
  Future<void> answerPassword(TerminalSessionId id, SecretText? password) async => password?.wipe();
}
