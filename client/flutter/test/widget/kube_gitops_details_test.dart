import 'dart:async';

import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_gitops_details.dart';
import 'package:consolecrypt/kubernetes/kube_insights_service.dart';
import 'package:consolecrypt/kubernetes/kube_mutation_service.dart';
import 'package:consolecrypt/kubernetes/kube_operations_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_write_access.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

const _session = KubeSession(id: 'session', contextId: 'context', serverVersion: 'v1.32.0', production: true);
const _source = KubeResourceKind(
  group: 'source.toolkit.fluxcd.io',
  version: 'v1',
  resource: 'gitrepositories',
  kind: 'GitRepository',
  namespaced: true,
  verbs: ['get', 'list', 'patch'],
);
const _deployment = KubeResourceKind(
  group: 'apps',
  version: 'v1',
  resource: 'deployments',
  kind: 'Deployment',
  namespaced: true,
  verbs: ['get', 'list'],
);
const _kustomization = KubeResourceKind(
  group: 'kustomize.toolkit.fluxcd.io',
  version: 'v1',
  resource: 'kustomizations',
  kind: 'Kustomization',
  namespaced: true,
  verbs: ['get', 'list', 'patch'],
);
const _helmRelease = KubeResourceKind(
  group: 'helm.toolkit.fluxcd.io',
  version: 'v2',
  resource: 'helmreleases',
  kind: 'HelmRelease',
  namespaced: true,
  verbs: ['get', 'list'],
);
final _scopeProvider = NotifierProvider<_Scope, KubeScope>(_Scope.new);

class _Scope extends Notifier<KubeScope> {
  @override
  KubeScope build() => const KubeScope('profile', true);
  void lock() => state = const KubeScope('profile', false);
}

class _Service implements KubeBrowserService, KubeMutationService, KubeInsightsService {
  Completer<List<KubeResourceKind>>? discovery;
  Completer<KubeResourceDetail>? source;
  final reads = <KubeResourceRef>[];
  bool allowed = true;
  int reconciles = 0;
  @override
  Future<List<KubeResourceKind>> discover(String sessionId) async =>
      discovery == null ? [_source, _deployment, _kustomization] : discovery!.future;
  @override
  Future<KubeWriteAccess> writeAccess(String sessionId) async =>
      KubeWriteAccess(production: true, expiresAt: allowed ? DateTime.now().add(const Duration(minutes: 5)) : null);
  @override
  Future<KubeResourceDetail> getResource(String sessionId, KubeResourceRef resource) async {
    reads.add(resource);
    if (source != null) return source!.future;
    return const KubeResourceDetail(
      object: {
        'spec': {'url': 'https://user:private@github.com/team/repo.git?token=private'},
        'status': {
          'artifact': {'revision': 'main@sha1:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb'},
        },
      },
      yaml: '',
    );
  }

  @override
  Future<void> fluxAction(String sessionId, KubeResourceRef resource, KubeFluxAction action, {String? reason}) async {
    reconciles++;
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

class _EventsService extends _Service implements KubeOperationsService {
  Completer<List<KubeResourceRow>>? events;
  @override
  Future<List<KubeResourceRow>> objectEvents(String sessionId, KubeResourceRef resource, String uid) async {
    expect(uid, 'source-id');
    if (events != null) return events!.future;
    return [
      KubeResourceRow.fromJson({
        'name': 'source.event',
        'namespace': 'team',
        'uid': 'event-id',
        'resource_version': '2',
        'kind': 'Event',
        'status': 'Unknown',
        'object': {
          'metadata': {'uid': 'event-id'},
          'involvedObject': {'uid': uid},
          'lastTimestamp': '2026-10-20T10:10:00Z',
          'reason': 'ArtifactUpToDate',
          'count': 3,
          'type': 'Normal',
          'message': 'password=never-display-unreviewed-event-text',
        },
      }, includeObject: true),
    ];
  }
}

Future<ProviderContainer> _pump(
  WidgetTester tester,
  _Service service, {
  KubeResourceKind kind = _source,
  Map<String, dynamic>? object,
  void Function(KubeResourceRef)? onOpen,
  String language = 'en',
  double textScale = 1,
}) async {
  final backend = testBackend();
  addTearDown(backend.dispose);
  tester.view.physicalSize = const Size(412, 900);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        appServicesProvider.overrideWithValue(backend.services),
        kubeScopeProvider.overrideWith((ref) => ref.watch(_scopeProvider)),
        kubeWriteAccessProvider(_session).overrideWith((ref) => service.writeAccess(_session.id)),
      ],
      child: MaterialApp(
        locale: Locale(language),
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        supportedLocales: AppLocalizations.supportedLocales,
        builder: (context, child) => MediaQuery(
          data: MediaQuery.of(context).copyWith(textScaler: TextScaler.linear(textScale)),
          child: child!,
        ),
        home: Scaffold(
          body: SingleChildScrollView(
            child: Padding(
              padding: const EdgeInsets.all(16),
              child: KubeGitOpsDetails(
                session: _session,
                resource: KubeResourceRef(api: kind, namespace: 'team', name: 'source'),
                detail: KubeResourceDetail(
                  object:
                      object ??
                      {
                        'spec': {
                          'url': 'https://user:password@git.example.test/team/repo?token=private#credentials',
                          'interval': '1m',
                          'ref': {'branch': 'main'},
                        },
                        'status': {
                          'artifact': {'revision': 'main@sha1:123', 'digest': 'sha256:abc'},
                          'conditions': [
                            {'type': 'SourceVerified', 'status': 'True', 'reason': 'Succeeded'},
                          ],
                        },
                      },
                  yaml: '',
                ),
                service: service,
                onOpen: onOpen ?? (_) {},
                onLogs: (_, _) {},
              ),
            ),
          ),
        ),
      ),
    ),
  );
  await settle(tester);
  return ProviderScope.containerOf(tester.element(find.byType(KubeGitOpsDetails)));
}

void main() {
  testWidgets('Helm history includes individual test hook status and overall TestSuccess', (tester) async {
    await _pump(
      tester,
      _Service(),
      kind: _helmRelease,
      object: {
        'status': {
          'conditions': [
            {'type': 'TestSuccess', 'status': 'False', 'reason': 'TestFailed'},
          ],
          'history': [
            {
              'name': 'web',
              'namespace': 'apps',
              'version': 4,
              'status': 'deployed',
              'testHooks': {
                'connectivity-test': {'phase': 'Failed', 'lastCompleted': '2026-10-20T10:00:00Z'},
              },
            },
          ],
        },
      },
    );
    expect(find.textContaining('TestFailed'), findsOneWidget);
    expect(find.textContaining('connectivity-test · Failed'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });
  test('commit links preserve only recognized repository paths and commit hashes', () {
    const hash = '1234567890abcdef1234567890abcdef12345678';
    expect(
      gitOpsCommitUri(
        'https://user:secret@github.com/team/repo.git?token=secret#private',
        'main@sha1:$hash',
      ).toString(),
      'https://github.com/team/repo/commit/$hash',
    );
    expect(
      gitOpsCommitUri('git@gitlab.example.test:team/nested/repo.git', hash).toString(),
      'https://gitlab.example.test/team/nested/repo/-/commit/$hash',
    );
    expect(
      gitOpsCommitUri('https://gitea.example.test/team/repo', hash).toString(),
      'https://gitea.example.test/team/repo/commit/$hash',
    );
    expect(
      gitOpsCommitUri('https://bitbucket.org/team/repo', hash).toString(),
      'https://bitbucket.org/team/repo/commits/$hash',
    );
    for (final url in [
      'javascript:alert(1)',
      'file:///team/repo',
      'https://github.com/team/%2fescape',
      'https://unknown.test/team/repo',
    ]) {
      expect(gitOpsCommitUri(url, hash), isNull);
    }
    expect(gitOpsCommitUri('https://github.com/team/repo', 'main; dangerous'), isNull);
  });

  testWidgets('event timeline shows bounded event metadata without copying controller messages', (tester) async {
    await _pump(
      tester,
      _EventsService(),
      object: {
        'metadata': {'uid': 'source-id'},
        'spec': {'url': 'https://github.com/team/repo'},
        'status': <String, dynamic>{},
      },
      language: 'ru',
      textScale: 1.6,
    );
    expect(find.textContaining('ArtifactUpToDate'), findsOneWidget);
    expect(find.textContaining('×3'), findsOneWidget);
    expect(find.textContaining('never-display'), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('source metadata hides URL credentials and fits a phone with large Russian text', (tester) async {
    await _pump(tester, _Service(), language: 'ru', textScale: 1.6);
    expect(find.textContaining('https://git.example.test/team/repo'), findsOneWidget);
    expect(find.textContaining('password'), findsNothing);
    expect(find.textContaining('token=private'), findsNothing);
    expect(find.textContaining('SourceVerified'), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('inventory opens the exact served resource and ignores malformed entries', (tester) async {
    final opened = <KubeResourceRef>[];
    await _pump(
      tester,
      _Service(),
      kind: _kustomization,
      onOpen: opened.add,
      object: {
        'spec': {
          'sourceRef': {'kind': 'GitRepository', 'name': 'git'},
        },
        'status': {
          'inventory': {
            'entries': [
              {'id': 'production_web_apps_Deployment', 'v': 'v1'},
              {'id': 'malformed_entry'},
            ],
          },
        },
      },
    );
    await tester.tap(find.byKey(const ValueKey('gitops-link-apps-Deployment-production-web')));
    await tester.pump();
    expect(opened.single.api, same(_deployment));
    expect(opened.single.namespace, 'production');
    expect(opened.single.name, 'web');
    expect(find.textContaining('malformed_entry'), findsNothing);
  });

  testWidgets('Kustomization compares applied revision with its exact source and links commits', (tester) async {
    final service = _Service();
    await _pump(
      tester,
      service,
      kind: _kustomization,
      object: {
        'spec': {
          'sourceRef': {'kind': 'GitRepository', 'namespace': 'sources', 'name': 'git'},
        },
        'status': {'lastAppliedRevision': 'main@sha1:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'},
      },
    );
    expect(service.reads, hasLength(1));
    expect(service.reads.single.api, same(_source));
    expect(service.reads.single.namespace, 'sources');
    expect(service.reads.single.name, 'git');
    expect(find.textContaining('Applied revision: main@sha1:aaaa'), findsOneWidget);
    expect(find.textContaining('Latest source revision: main@sha1:bbbb'), findsOneWidget);
    expect(
      find.byKey(const ValueKey('gitops-open-commit-main@sha1:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa')),
      findsOneWidget,
    );
    expect(
      find.byKey(const ValueKey('gitops-open-commit-main@sha1:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb')),
      findsOneWidget,
    );
    expect(find.textContaining('private'), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('a source response cannot restore revisions after profile lock', (tester) async {
    final service = _Service()..source = Completer<KubeResourceDetail>();
    final scope = await _pump(
      tester,
      service,
      kind: _kustomization,
      object: {
        'spec': {
          'sourceRef': {'kind': 'GitRepository', 'name': 'git'},
        },
      },
    );
    scope.read(_scopeProvider.notifier).lock();
    service.source!.complete(
      const KubeResourceDetail(
        object: {
          'status': {
            'artifact': {'revision': 'private-revision'},
          },
        },
        yaml: '',
      ),
    );
    await settle(tester);
    expect(find.textContaining('private-revision'), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('expired production access after confirmation prevents reconciliation', (tester) async {
    final service = _Service();
    await _pump(tester, service);
    await tester.tap(find.byKey(const ValueKey('gitops-reconcile-source')));
    await settle(tester);
    await tester.enterText(find.byKey(const ValueKey('kube-confirm-name')), 'source');
    service.allowed = false;
    await tester.pump();
    await tester.tap(find.byKey(const ValueKey('kube-confirm-action')));
    await settle(tester);
    expect(service.reconciles, 0);
    expect(tester.takeException(), isNull);
  });

  testWidgets('late discovery does not restore data after profile lock', (tester) async {
    final service = _Service()..discovery = Completer<List<KubeResourceKind>>();
    final scope = await _pump(tester, service);
    scope.read(_scopeProvider.notifier).lock();
    service.discovery!.complete([_source, _deployment]);
    await settle(tester);
    expect(find.textContaining('git.example.test'), findsNothing);
    expect(find.byKey(const ValueKey('gitops-reconcile-source')), findsNothing);
    expect(tester.takeException(), isNull);
  });
}
