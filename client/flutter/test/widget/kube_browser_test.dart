import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/terminal_service.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_ai_history.dart';
import 'package:consolecrypt/kubernetes/kube_ai_service.dart';
import 'package:consolecrypt/kubernetes/kube_browser.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_bulk_dialog.dart';
import 'package:consolecrypt/kubernetes/kube_create_dialog.dart';
import 'package:consolecrypt/kubernetes/kube_forward_panel.dart';
import 'package:consolecrypt/kubernetes/kube_forward_restore.dart';
import 'package:consolecrypt/kubernetes/kube_forward_service.dart';
import 'package:consolecrypt/kubernetes/kube_forward_targets_dialog.dart';
import 'package:consolecrypt/kubernetes/kube_forward_targets_service.dart';
import 'package:consolecrypt/kubernetes/kube_gitops_graph.dart';
import 'package:consolecrypt/kubernetes/kube_gitops_timeline.dart';
import 'package:consolecrypt/kubernetes/kube_insights_service.dart';
import 'package:consolecrypt/kubernetes/kube_manifest_service.dart';
import 'package:consolecrypt/kubernetes/kube_mutation_service.dart';
import 'package:consolecrypt/kubernetes/kube_navigation.dart';
import 'package:consolecrypt/kubernetes/kube_operations_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_resource_summary.dart';
import 'package:consolecrypt/kubernetes/kube_rollout_panel.dart';
import 'package:consolecrypt/kubernetes/kube_rollout_service.dart';
import 'package:consolecrypt/kubernetes/kube_schema_fields.dart';
import 'package:consolecrypt/kubernetes/kube_secret_dialog.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:consolecrypt/kubernetes/kube_terminal_panel.dart';
import 'package:consolecrypt/kubernetes/kube_write_access.dart';
import 'package:consolecrypt/kubernetes/kubernetes_screen.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

final _scope = NotifierProvider<_Scope, KubeScope>(_Scope.new);

class _Scope extends Notifier<KubeScope> {
  @override
  KubeScope build() => const KubeScope('profile', true);
  void lock() => state = const KubeScope('profile', false);
  void changeProfile() => state = const KubeScope('another-profile', true);
}

const _pods = KubeResourceKind(
  group: '',
  version: 'v1',
  resource: 'pods',
  kind: 'Pod',
  namespaced: true,
  verbs: ['get', 'list', 'watch'],
);
const _nodes = KubeResourceKind(
  group: '',
  version: 'v1',
  resource: 'nodes',
  kind: 'Node',
  namespaced: false,
  verbs: ['get', 'list', 'watch'],
);
const _connection = KubeConnection(
  id: 'context',
  name: 'demo-cluster',
  clusterName: 'demo',
  server: 'https://demo.example.test',
  namespace: 'default',
  authentication: 'token',
  production: true,
);
const _session = KubeSession(
  id: 'opaque-native-session',
  contextId: 'context',
  serverVersion: 'v1.32.1',
  production: true,
);

KubeResourceRow _row(String name, {String status = 'Running'}) =>
    KubeResourceRow(name: name, uid: name, resourceVersion: '10', namespace: 'default', status: status);
KubeResourcePage _page(List<KubeResourceRow> rows, {String? next}) =>
    KubeResourcePage(items: rows, resourceVersion: '10', continueToken: next);

Future<ProviderContainer> _pump(
  WidgetTester tester,
  _Service service, {
  String language = 'en',
  Size size = const Size(1100, 900),
  double textScale = 1,
  TerminalService? terminalService,
}) async {
  final backend = testBackend();
  addTearDown(backend.dispose);
  if (terminalService != null) await backend.debugSignInDemoAndUnlock();
  addTearDown(service.dispose);
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        appServicesProvider.overrideWithValue(backend.services),
        if (terminalService != null) ...[
          terminalServiceProvider.overrideWithValue(terminalService),
          activeProfileProvider.overrideWithValue(backend.profiles.currentProfiles.active),
        ],
        kubeServiceProvider.overrideWithValue(service),
        kubeScopeProvider.overrideWith((ref) => ref.watch(_scope)),
      ],
      child: MaterialApp(
        locale: Locale(language),
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        supportedLocales: AppLocalizations.supportedLocales,
        builder: (context, child) => MediaQuery(
          data: MediaQuery.of(context).copyWith(textScaler: TextScaler.linear(textScale)),
          child: child!,
        ),
        home: const Scaffold(body: KubernetesScreen()),
      ),
    ),
  );
  await settle(tester);
  final container = ProviderScope.containerOf(tester.element(find.byType(KubernetesScreen)));
  if (terminalService != null) {
    container.read(activeProfileProvider);
    container.read(vaultStatusProvider);
    await settle(tester);
  }
  await tester.tap(find.byKey(const ValueKey('kube-connect-context')));
  await settle(tester);
  return container;
}

void main() {
  testWidgets('cold connection completes discovery before resolving namespaces', (tester) async {
    final service = _ColdDiscoveryService();
    await _pump(tester, service);
    expect(service.namespaceCalls, 0);
    expect(service.requests, isEmpty);
    service.discovery.complete();
    await settle(tester);
    expect(service.namespaceCalls, 1);
    expect(service.requests, isNotEmpty);
    await tester.tap(find.byKey(const ValueKey('kube-namespace')));
    await settle(tester);
    expect(find.text('kube-system'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('locking during discovery prevents subsequent namespace requests', (tester) async {
    final service = _ColdDiscoveryService();
    final container = await _pump(tester, service);
    container.read(_scope.notifier).lock();
    await settle(tester);
    service.discovery.complete();
    await settle(tester);
    expect(service.namespaceCalls, 0);
    expect(service.requests, isEmpty);
    expect(service.disconnected, contains(_session.id));
    expect(tester.takeException(), isNull);
  });

  testWidgets('mobile navigation opens a full-screen resource on the selected logs tab', (tester) async {
    final service = _LogService();
    final container = await _pump(tester, service, size: const Size(412, 915), textScale: 1.3);
    final nav = tester.widget<NavigationBar>(find.byKey(const ValueKey('kube-mobile-navigation')));
    expect((nav.destinations.last as NavigationDestination).enabled, isFalse);
    nav.onDestinationSelected!(3);
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('kube-resource-demo-pod')));
    await settle(tester);
    final details = find.byKey(const ValueKey('kube-resource-details'));
    expect(ModalRoute.of(tester.element(details)), isA<MaterialPageRoute<void>>());
    expect(tester.getSize(details).width, 412);
    expect(find.byKey(const ValueKey('kube-log-lines')), findsOneWidget);
    expect(service.logsCalls, isNotEmpty);
    container.read(_scope.notifier).lock();
    await settle(tester);
    expect(details, findsNothing);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.android));

  testWidgets('mobile Pod swipe opens logs or shell review without opening a terminal', (tester) async {
    final service = _LogService()
      ..session = const KubeSession(
        id: 'opaque-native-session',
        contextId: 'context',
        serverVersion: 'v1',
        production: false,
      );
    final terminal = _PodTerminalService();
    await _pump(tester, service, size: const Size(412, 915), terminalService: terminal);
    final swipe = find.byKey(const ValueKey('kube-resource-swipe-demo-pod'));
    await tester.drag(swipe, const Offset(320, 0));
    await settle(tester);
    expect(find.byKey(const ValueKey('kube-log-lines')), findsOneWidget);
    expect(terminal.targets, isEmpty);
    await tester.tap(find.text('Close').last);
    await settle(tester);
    await tester.drag(swipe, const Offset(-320, 0));
    await settle(tester);
    expect(find.byKey(const ValueKey('kube-terminal-dialog')), findsOneWidget);
    expect(terminal.targets, isEmpty);
    await tester.tap(find.text('Cancel').last);
    await settle(tester);
    expect(terminal.targets, isEmpty);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.android));

  testWidgets('mobile workload restart swipe requires typed approval and fresh resource version', (tester) async {
    final service = _WorkloadSwipeService();
    await _pump(tester, service, size: const Size(412, 915));
    await tester.drag(find.byKey(const ValueKey('kube-resource-swipe-demo-pod')), const Offset(-320, 0));
    await settle(tester);
    expect(find.byKey(const ValueKey('kube-confirm-name')), findsOneWidget);
    expect(service.restarted, isEmpty);
    expect(isEnabled(tester, 'kube-confirm-action'), isFalse);
    await tester.enterText(find.byKey(const ValueKey('kube-confirm-name')), 'demo-pod');
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('kube-confirm-action')));
    await settle(tester);
    expect(service.restarted, [('demo-pod', 'latest-version')]);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.android));

  testWidgets('tablet details retain the wide protected dialog over the resource table', (tester) async {
    await _pump(tester, _Service(), size: const Size(1024, 900));
    await tester.tap(find.byKey(const ValueKey('kube-resource-demo-pod')));
    await settle(tester);
    final details = find.byKey(const ValueKey('kube-resource-details'));
    expect(ModalRoute.of(tester.element(details)), isA<PopupRoute<bool>>());
    expect(tester.getSize(details).width, lessThan(1024));
    expect(tester.getSize(details).width, greaterThan(700));
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.android));

  testWidgets('mobile Flux swipe opens explicit review and never submits by itself', (tester) async {
    final service = _InsightsService()
      ..session = const KubeSession(
        id: 'opaque-native-session',
        contextId: 'context',
        serverVersion: 'v1',
        production: false,
      )
      ..access = const KubeWriteAccess(production: false);
    await _pump(tester, service, size: const Size(412, 915));
    tester.widget<NavigationBar>(find.byKey(const ValueKey('kube-mobile-navigation'))).onDestinationSelected!(4);
    await settle(tester);
    final swipe = find.byType(Dismissible);
    expect(swipe, findsOneWidget);
    await tester.drag(swipe, const Offset(320, 0));
    await settle(tester);
    expect(find.byKey(const ValueKey('kube-gitops-confirm-name')), findsOneWidget);
    expect(service.fluxActions, isEmpty);
    expect(tester.widget<GlassButton>(find.byKey(const ValueKey('kube-gitops-confirm-action'))).onPressed, isNull);
    await tester.tap(find.text('Cancel').last);
    await settle(tester);
    expect(swipe, findsOneWidget);
    expect(service.fluxActions, isEmpty);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.android));

  testWidgets('mobile Flux details keep native actions in a lower toolbar', (tester) async {
    final service = _FluxDetailService();
    await _pump(tester, service, size: const Size(412, 915), language: 'ru', textScale: 1.5);
    tester.widget<NavigationBar>(find.byKey(const ValueKey('kube-mobile-navigation'))).onDestinationSelected!(4);
    await settle(tester);
    await tester.ensureVisible(find.text('demo-flux'));
    await tester.tap(find.text('demo-flux'));
    await settle(tester);
    final bottom = find.byKey(const ValueKey('kube-gitops-bottom-actions'));
    expect(bottom, findsOneWidget);
    expect(
      tester.getTopLeft(bottom).dy,
      greaterThan(tester.getTopLeft(find.byKey(const ValueKey('kube-yaml-tab'))).dy),
    );
    expect(tester.widget<GlassButton>(find.byKey(const ValueKey('kube-gitops-detail-reconcile'))).onPressed, isNull);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.android));

  testWidgets('dependency view is a layered graph on a wide window', (tester) async {
    await _pump(tester, _InsightsService());
    await tester.tap(find.byKey(const ValueKey('kube-view-gitops')));
    await settle(tester);
    await tester.ensureVisible(find.byKey(const ValueKey('kube-gitops-dependencies')));
    await tester.tap(find.byKey(const ValueKey('kube-gitops-dependencies')));
    await settle(tester);
    expect(find.byKey(const ValueKey('kube-dependency-list')), findsOneWidget);
    expect(tester.widget<KubeGitOpsGraph>(find.byType(KubeGitOpsGraph)).dependencyLayout, isTrue);
    expect(find.byKey(const ValueKey('kube-gitops-flow')), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('schema field insertion preserves surrounding source and stale suggestions vanish on lock', (
    tester,
  ) async {
    final service = _ManifestService()
      ..schema = const {
        'schema': {
          'properties': {
            'replicas': {'type': 'integer'},
          },
        },
      };
    final container = await _pump(tester, service);
    final editor = TextEditingController(text: 'spec:\n  \n')..selection = const TextSelection.collapsed(offset: 8);
    addTearDown(editor.dispose);
    var edits = 0;
    unawaited(
      showKubeSchemaFields(
        tester.element(find.byType(KubernetesScreen)),
        session: service.session,
        kind: _pods,
        service: service,
        editor: editor,
        onChanged: () => edits++,
      ),
    );
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('kube-schema-insert-replicas')));
    await settle(tester);
    expect(editor.text, 'spec:\n  replicas: \n');
    expect(edits, 1);
    expect(service.applies, isEmpty);
    unawaited(
      showKubeSchemaFields(
        tester.element(find.byType(KubernetesScreen)),
        session: service.session,
        kind: _pods,
        service: service,
        editor: editor,
        onChanged: () => edits++,
      ),
    );
    await settle(tester);
    container.read(_scope.notifier).lock();
    await settle(tester);
    expect(find.byKey(const ValueKey('kube-schema-fields')), findsNothing);
    expect(edits, 1);
    expect(tester.takeException(), isNull);
  });

  testWidgets('overview timeline reads bounded UID-scoped events only after expansion', (tester) async {
    final service = _TimelineService();
    final container = await _pump(tester, service);
    final resources = (await service.insights(service.session.id)).resources;
    unawaited(
      showAppDialog<void>(
        tester.element(find.byType(KubernetesScreen)),
        secure: true,
        builder: (_) => GlassDialog(
          title: 'Timeline',
          content: SizedBox(
            height: 450,
            child: KubeGitOpsTimeline(session: service.session, resources: resources, service: service, onOpen: (_) {}),
          ),
        ),
      ),
    );
    await settle(tester);
    expect(service.eventReads, isEmpty);
    await tester.tap(find.byKey(const ValueKey('kube-gitops-timeline')));
    await settle(tester);
    expect(service.eventReads, [('demo-flux', 'demo-flux')]);
    expect(find.textContaining('ReconciliationSucceeded'), findsOneWidget);
    expect(find.textContaining('main@sha1:event-revision'), findsOneWidget);
    container.read(_scope.notifier).lock();
    await settle(tester);
    expect(find.textContaining('main@sha1:event-revision'), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('masked YAML cannot be edited even when production writes are unlocked', (tester) async {
    final service = _MaskedService()..access = const KubeWriteAccess(production: false);
    await _pump(tester, service);
    await tester.tap(find.byKey(const ValueKey('kube-resource-demo-pod')));
    await settle(tester);
    expect(isEnabled(tester, 'kube-edit-yaml'), isFalse);
    expect(find.textContaining('masked values'), findsOneWidget);
    expect(service.applies, isEmpty);
    expect(tester.takeException(), isNull);
  });

  testWidgets('network forwarding requires explicit consent and resets it when address changes', (tester) async {
    final service = _NetworkForwardService();
    await _pump(tester, service);
    unawaited(
      showKubeForwardDialog(
        tester.element(find.byType(KubernetesScreen)),
        session: service.session,
        resource: const KubeResourceRef(api: _pods, name: 'demo-pod', namespace: 'default'),
        service: service,
        detail: const KubeResourceDetail(object: {}, yaml: '', containers: []),
      ),
    );
    await settle(tester);
    await tester.enterText(find.byKey(const ValueKey('kube-forward-remote')), '8080');
    await tester.tap(find.byType(SwitchListTile));
    await settle(tester);
    expect(isEnabled(tester, 'kube-forward-start'), isFalse);
    await tester.tap(find.byKey(const ValueKey('kube-forward-network-approve')));
    await settle(tester);
    expect(isEnabled(tester, 'kube-forward-start'), isTrue);
    await tester.enterText(find.byKey(const ValueKey('kube-forward-bind-address')), '192.0.2.10');
    await settle(tester);
    expect(isEnabled(tester, 'kube-forward-start'), isFalse);
    await tester.tap(find.byKey(const ValueKey('kube-forward-network-approve')));
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('kube-forward-start')));
    await settle(tester);
    expect(service.addresses, ['192.0.2.10']);
    expect(service.started, isEmpty);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('opted-in iOS forwarding restores only within unlocked context and clears on lock', (tester) async {
    final service = _ForwardService();
    final container = await _pump(tester, service);
    container
        .read(kubeForwardRestoreProvider.notifier)
        .remember(
          const KubeForwardRestorePlan(
            contextId: 'context',
            namespace: 'default',
            pod: 'demo-pod',
            remotePort: 8080,
            localPort: 34567,
            sessionId: 'opaque-native-session',
            forwardId: 'old-forward',
          ),
        );
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.inactive);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.hidden);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.paused);
    await settle(tester);
    service.session = const KubeSession(id: 'resumed', contextId: 'context', serverVersion: 'v1', production: false);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.hidden);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.inactive);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
    await settle(tester);
    expect(service.started, [(remote: 8080, local: 34567, pod: 'demo-pod')]);
    container.read(_scope.notifier).lock();
    await settle(tester);
    expect(container.read(kubeForwardRestoreProvider), isEmpty);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.iOS));

  testWidgets('structural workload details exclude environment and probe credentials on a phone', (tester) async {
    await _pump(tester, _Service(), size: const Size(412, 915), language: 'ru', textScale: 1.3);
    unawaited(
      showAppDialog<void>(
        tester.element(find.byType(KubernetesScreen)),
        secure: true,
        builder: (_) => const GlassDialog(
          title: 'Details',
          content: SizedBox(
            height: 460,
            child: SingleChildScrollView(
              child: KubeResourceSummary(
                resource: KubeResourceRef(api: _pods, name: 'demo-pod'),
                detail: KubeResourceDetail(
                  yaml: '',
                  containers: [],
                  object: {
                    'spec': {
                      'containers': [
                        {
                          'name': 'app',
                          'image': 'example/image:v1',
                          'env': [
                            {'name': 'TOKEN', 'value': 'never-show-token'},
                          ],
                          'readinessProbe': {
                            'httpGet': {
                              'port': 8080,
                              'httpHeaders': [
                                {'name': 'Authorization', 'value': 'never-show-header'},
                              ],
                            },
                            'periodSeconds': 10,
                          },
                          'resources': {
                            'requests': {'cpu': '100m', 'memory': '64Mi'},
                          },
                        },
                      ],
                    },
                  },
                ),
              ),
            ),
          ),
        ),
      ),
    );
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('kube-structural-summary')));
    await settle(tester);
    await tester.tap(find.text('Контейнер · app'));
    await settle(tester);
    expect(find.textContaining('100m'), findsOneWidget);
    expect(find.textContaining('never-show'), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('bulk delete requires explicit context name and exact per-row preconditions', (tester) async {
    final service = _MutationService()..access = const KubeWriteAccess(production: false);
    await _pump(tester, service);
    const kind = KubeResourceKind(
      group: '',
      version: 'v1',
      resource: 'pods',
      kind: 'Pod',
      namespaced: true,
      verbs: ['delete'],
    );
    const session = KubeSession(id: 'safe', contextId: 'context', serverVersion: 'v1', production: false);
    unawaited(
      showAppDialog<bool>(
        tester.element(find.byType(KubernetesScreen)),
        secure: true,
        builder: (_) => KubeBulkDialog(
          session: session,
          contextName: 'demo-cluster',
          kind: kind,
          rows: [_row('one'), _row('two')],
          service: service,
        ),
      ),
    );
    await settle(tester);
    expect(service.deleted, isEmpty);
    await tester.enterText(find.byKey(const ValueKey('kube-bulk-confirm')), 'demo-cluster');
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('kube-bulk-run')));
    await settle(tester);
    expect(service.deleted, [(name: 'one', uid: 'one', version: '10'), (name: 'two', uid: 'two', version: '10')]);
    expect(isEnabled(tester, 'kube-bulk-run'), isFalse);
    expect(tester.takeException(), isNull);
  });
  testWidgets('bulk remains disabled in production even with an open write window', (tester) async {
    final service = _MutationService()
      ..access = KubeWriteAccess(production: true, expiresAt: DateTime.now().add(const Duration(minutes: 10)));
    await _pump(tester, service);
    const kind = KubeResourceKind(
      group: '',
      version: 'v1',
      resource: 'pods',
      kind: 'Pod',
      namespaced: true,
      verbs: ['delete'],
    );
    unawaited(
      showAppDialog<bool>(
        tester.element(find.byType(KubernetesScreen)),
        secure: true,
        builder: (_) => KubeBulkDialog(
          session: _session,
          contextName: 'demo-cluster',
          kind: kind,
          rows: [_row('one')],
          service: service,
        ),
      ),
    );
    await settle(tester);
    await tester.enterText(find.byKey(const ValueKey('kube-bulk-confirm')), 'demo-cluster');
    await settle(tester);
    expect(isEnabled(tester, 'kube-bulk-run'), isFalse);
    expect(service.deleted, isEmpty);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.android, TargetPlatform.windows}));
  testWidgets('Service forwarding chooses a Pod and mapped target port before starting', (tester) async {
    final service = _ForwardTargetService()
      ..session = const KubeSession(id: 'session', contextId: 'context', serverVersion: 'v1', production: false);
    await _pump(tester, service, size: const Size(412, 915), language: 'ru', textScale: 1.3);
    unawaited(
      showKubeResourceForwardDialog(
        tester.element(find.byType(KubernetesScreen)),
        session: service.session,
        resource: const KubeResourceRef(api: _pods, name: 'service', namespace: 'default'),
        service: service,
        detail: const KubeResourceDetail(object: {}, yaml: '', containers: []),
      ),
    );
    await settle(tester);
    expect(service.started, isEmpty);
    await tester.tap(find.byKey(const ValueKey('kube-forward-target-api-pod')));
    await settle(tester);
    expect(service.started, isEmpty);
    await tester.tap(find.byKey(const ValueKey('kube-forward-start')));
    await settle(tester);
    expect(service.started.single, (remote: 9090, local: 0, pod: 'api-pod'));
    expect(tester.takeException(), isNull);
  });
  testWidgets('iOS releases the native session in background and reconnects after resume', (tester) async {
    final service = _Service();
    await _pump(tester, service);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.inactive);
    await settle(tester);
    expect(service.disconnected, isEmpty);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.hidden);
    await settle(tester);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.paused);
    await settle(tester);
    expect(service.disconnected, hasLength(1));
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.hidden);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.inactive);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
    await settle(tester);
    expect(service.connected, hasLength(2));
    expect(find.text('demo-pod'), findsOneWidget);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.iOS));
  testWidgets('Secret reveal requires explicit action, wipes transport and expires after thirty seconds', (
    tester,
  ) async {
    final service = _ManifestService();
    final container = await _pump(tester, service, size: const Size(412, 915), language: 'ru', textScale: 1.3);
    unawaited(
      showAppDialog<void>(
        tester.element(find.byType(KubernetesScreen)),
        secure: true,
        builder: (_) => KubeSecretDialog(
          session: service.session,
          resource: const KubeResourceRef(api: _pods, name: 'secret', namespace: 'default'),
          service: service,
        ),
      ),
    );
    await settle(tester);
    expect(service.revealed, isEmpty);
    await tester.tap(find.byKey(const ValueKey('kube-secret-reveal')));
    await settle(tester);
    expect(find.text('private-value'), findsOneWidget);
    expect(service.revealed.single.isWiped, isTrue);
    await tester.pump(const Duration(seconds: 31));
    await settle(tester);
    expect(find.text('private-value'), findsNothing);
    await tester.tap(find.byKey(const ValueKey('kube-secret-reveal')));
    await settle(tester);
    expect(find.text('private-value'), findsOneWidget);
    container.read(_scope.notifier).lock();
    await settle(tester);
    expect(find.text('private-value'), findsNothing);
    expect(tester.takeException(), isNull);
  });
  testWidgets('create resource requires dry run and exact-name confirmation, then wipes manifest', (tester) async {
    final service = _ManifestService();
    await _pump(tester, service);
    unawaited(
      showAppDialog<bool>(
        tester.element(find.byType(KubernetesScreen)),
        secure: true,
        builder: (_) => KubeCreateDialog(
          session: service.session,
          contextName: 'demo',
          kind: _pods,
          namespace: 'default',
          service: service,
        ),
      ),
    );
    await settle(tester);
    await tester.enterText(find.byKey(const ValueKey('kube-create-name')), 'new-pod');
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('kube-create-template')));
    await settle(tester);
    expect(isEnabled(tester, 'kube-create-apply'), isFalse);
    await tester.tap(find.byKey(const ValueKey('kube-create-validate')));
    await settle(tester);
    expect(service.creates.single.dryRun, isTrue);
    expect(service.creates.single.manifest.isWiped, isTrue);
    await tester.tap(find.byKey(const ValueKey('kube-create-apply')));
    await settle(tester);
    expect(service.creates, hasLength(1));
    await tester.enterText(find.byKey(const ValueKey('kube-confirm-name')), 'wrong');
    await settle(tester);
    expect(isEnabled(tester, 'kube-confirm-action'), isFalse);
    await tester.enterText(find.byKey(const ValueKey('kube-confirm-name')), 'new-pod');
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('kube-confirm-action')));
    await settle(tester);
    expect(service.creates.last.dryRun, isFalse);
    expect(service.creates.last.manifest.isWiped, isTrue);
    expect(tester.takeException(), isNull);
  });
  testWidgets('rollout comparison requests selected revisions and clears on lock', (tester) async {
    final service = _RolloutService();
    final container = await _pump(tester, service, size: const Size(412, 915), language: 'ru', textScale: 1.3);
    unawaited(
      showAppDialog<void>(
        tester.element(find.byType(KubernetesScreen)),
        secure: true,
        builder: (_) => KubeRolloutDialog(
          sessionId: service.session.id,
          resource: const KubeResourceRef(api: _pods, name: 'web', namespace: 'default'),
          service: service,
        ),
      ),
    );
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('kube-rollout-compare')));
    await settle(tester);
    expect(service.compared, [(1, 2)]);
    expect(find.text('/spec/containers/0/image'), findsOneWidget);
    container.read(_scope.notifier).lock();
    await settle(tester);
    expect(find.byType(KubeRolloutDialog), findsNothing);
    expect(tester.takeException(), isNull);
  });
  testWidgets('AI saved history opens locally and is removed on lock', (tester) async {
    final service = _AiHistoryService();
    final container = await _pump(tester, service, size: const Size(412, 915), language: 'ru', textScale: 1.3);
    unawaited(
      showAppDialog<void>(
        tester.element(find.byType(KubernetesScreen)),
        secure: true,
        builder: (_) => KubeAiHistoryDialog(contextId: 'context', service: service),
      ),
    );
    await settle(tester);
    await tester.tap(find.text('Saved incident'));
    await settle(tester);
    expect(find.text('Private incident answer'), findsOneWidget);
    container.read(_scope.notifier).lock();
    await settle(tester);
    expect(find.text('Private incident answer'), findsNothing);
    expect(tester.takeException(), isNull);
  });
  testWidgets('palette request selects a namespace without opening a second cluster session', (tester) async {
    final service = _Service();
    final container = await _pump(tester, service);
    container
        .read(kubeNavigationProvider.notifier)
        .open(
          KubeNavigationRequest(
            scope: container.read(kubeScopeProvider),
            contextId: 'context',
            namespace: 'kube-system',
          ),
        );
    await settle(tester);
    expect(service.connected, hasLength(1));
    expect(service.requests.last.namespace, 'kube-system');
    expect(container.read(kubeNavigationProvider), isNull);
    expect(container.read(kubeActiveNavigationProvider)?.resources.single.name, 'demo-pod');
    expect(tester.takeException(), isNull);
  });

  testWidgets('graph keeps API group identities distinct and filters observed namespace and labels', (tester) async {
    await _pump(tester, _Service(), size: const Size(412, 915));
    const a = KubeObjectIdentity(group: 'one.example', kind: 'Widget', namespace: 'first', name: 'same');
    const b = KubeObjectIdentity(group: 'two.example', kind: 'Widget', namespace: 'second', name: 'same');
    KubeInsightResource resource(KubeObjectIdentity object, String label, String condition) => KubeInsightResource(
      api: KubeResourceKind(
        group: object.group,
        version: 'v1',
        resource: 'widgets',
        kind: object.kind,
        namespaced: true,
      ),
      resource: KubeResourceRow(
        name: object.name,
        namespace: object.namespace,
        uid: object.group,
        resourceVersion: '1',
        object: {
          'metadata': {
            'labels': {'app': label},
          },
          'status': {
            'conditions': [
              {'type': condition, 'status': 'True'},
            ],
          },
        },
      ),
    );
    final opened = <String>[];
    unawaited(
      showAppDialog<void>(
        tester.element(find.byType(KubernetesScreen)),
        secure: true,
        builder: (_) => GlassDialog(
          title: 'Graph',
          content: SizedBox(
            height: 500,
            child: KubeGitOpsGraph(
              flow: const KubeGitOpsFlow(
                nodes: [
                  KubeFlowNode(object: a, ready: false),
                  KubeFlowNode(object: b, ready: false),
                ],
              ),
              resources: [resource(a, 'web', 'Reconciling'), resource(b, 'worker', 'Stalled')],
              canOpen: (_) => true,
              open: (object) => opened.add(object.group),
            ),
          ),
        ),
      ),
    );
    await settle(tester);
    final first = find.byKey(const ValueKey('kube-gitops-tree-one.example/Widget/first/same'));
    final second = find.byKey(const ValueKey('kube-gitops-tree-two.example/Widget/second/same'));
    expect(first, findsOneWidget);
    expect(second, findsOneWidget);
    expect(find.descendant(of: first, matching: find.byIcon(Icons.sync)), findsOneWidget);
    await tester.tap(second);
    await settle(tester);
    expect(opened, ['two.example']);
    await tester.ensureVisible(find.byKey(const ValueKey('kube-graph-namespace')));
    await tester.tap(find.byKey(const ValueKey('kube-graph-namespace')));
    await settle(tester);
    await tester.tap(find.text('first').last);
    await settle(tester);
    expect(first, findsOneWidget);
    expect(second, findsNothing);
    expect(tester.takeException(), isNull);
  });
  testWidgets('GitOps graph renders cycles and provides a keyboard-accessible list', (tester) async {
    final service = _Service();
    await _pump(tester, service, size: const Size(412, 915), language: 'ru');
    const a = KubeObjectIdentity(kind: 'GitRepository', namespace: 'flux-system', name: 'source');
    const b = KubeObjectIdentity(kind: 'Kustomization', namespace: 'flux-system', name: 'workload');
    final opened = <String>[];
    unawaited(
      showAppDialog<void>(
        tester.element(find.byType(KubernetesScreen)),
        secure: true,
        builder: (_) => GlassDialog(
          title: 'Graph',
          content: SizedBox(
            width: 600,
            height: 400,
            child: KubeGitOpsGraph(
              flow: const KubeGitOpsFlow(
                nodes: [
                  KubeFlowNode(object: a, ready: true),
                  KubeFlowNode(object: b, ready: false),
                ],
                edges: [
                  KubeFlowEdge(from: a, to: b, relation: 'source'),
                  KubeFlowEdge(from: b, to: a, relation: 'dependency'),
                ],
              ),
              canOpen: (_) => true,
              open: (id) => opened.add(id.name),
            ),
          ),
        ),
      ),
    );
    await settle(tester);
    expect(find.byKey(const ValueKey('kube-gitops-flow')), findsNothing);
    await tester.tap(find.text('Список'));
    await settle(tester);
    expect(find.byKey(const ValueKey('kube-gitops-flow')), findsOneWidget);
    await tester.tap(find.text('Список'));
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('kube-gitops-tree-GitRepository/flux-system/source')));
    await settle(tester);
    expect(opened, isNotEmpty);
    expect(tester.takeException(), isNull);
  });
  testWidgets('AI asks external-provider consent for each question and revokes sensitive context on lock', (
    tester,
  ) async {
    final service = _AiService();
    final container = await _pump(tester, service, size: const Size(412, 915), language: 'ru', textScale: 1.3);
    await tester.tap(find.byKey(const ValueKey('kube-open-ai')));
    await settle(tester);
    await tester.enterText(find.byKey(const ValueKey('kube-ai-question')), 'Why is this Pod restarting?');
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('kube-ai-send')));
    await settle(tester);
    expect(service.questions.single, 'Why is this Pod restarting?');
    expect(service.asked, 0);
    expect(find.byKey(const ValueKey('kube-ai-consent')), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('kube-ai-consent')));
    await settle(tester);
    expect(service.asked, 1);
    service.events.add(
      const KubeAiEvent('context', {'tool': 'events', 'content': 'Warning BackOff', 'truncated': true}),
    );
    service.events.add(
      const KubeAiEvent('answer', {
        'content': 'Check the container exit status.',
        'unresolved': ['metrics'],
      }),
    );
    service.events.add(const KubeAiEvent('done', {}));
    await settle(tester);
    expect(find.text('Check the container exit status.'), findsOneWidget);
    container.read(_scope.notifier).lock();
    await settle(tester);
    expect(find.text('Check the container exit status.'), findsNothing);
    expect(service.revoked, greaterThan(1));
    expect(tester.takeException(), isNull);
  });

  testWidgets('Pod shell opens selected container in a scoped bottom panel and sends keyboard input', (tester) async {
    final service = _Service()
      ..session = const KubeSession(
        id: 'opaque-native-session',
        contextId: 'context',
        serverVersion: 'v1.32.1',
        production: false,
      );
    final terminal = _PodTerminalService();
    final container = await _pump(tester, service, terminalService: terminal);
    await tester.tap(find.byKey(const ValueKey('kube-resource-demo-pod')));
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('kube-pod-terminal')));
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('kube-terminal-container')));
    await settle(tester);
    await tester.tap(find.text('sidecar').last);
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('kube-terminal-open')));
    await settle(tester);
    expect(terminal.targets.single.container, 'sidecar');
    expect(terminal.targets.single.command, ['/bin/sh']);
    expect(terminal.targets.single.sessionId, service.session.id);
    expect(find.byType(KubeTerminalPanel), findsOneWidget);
    final tab = container.read(terminalTabsProvider).tabs.single;
    tab.terminal.textInput('pwd');
    await settle(tester);
    expect(terminal.written, containsAll([112, 119, 100]));
    await tester.pumpWidget(const SizedBox());
    await settle(tester);
    expect(terminal.closed, contains(tab.sessionId));
    expect(tester.takeException(), isNull);
  });

  testWidgets('production Pod terminal and forwarding stay disabled without explicit context flags', (tester) async {
    final service = _ForwardService()..session = _session;
    await _pump(tester, service);
    await tester.tap(find.byKey(const ValueKey('kube-resource-demo-pod')));
    await settle(tester);
    expect(isEnabled(tester, 'kube-pod-terminal'), isFalse);
    expect(isEnabled(tester, 'kube-pod-forward'), isFalse);
    expect(service.started, isEmpty);
  });

  testWidgets('port forwarding validates ports and stops a late result after profile change', (tester) async {
    final service = _ForwardService()..pending = Completer<KubeForward>();
    final container = await _pump(tester, service, size: const Size(412, 915), language: 'ru', textScale: 1.3);
    await tester.tap(find.byKey(const ValueKey('kube-resource-demo-pod')));
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('kube-pod-forward')));
    await settle(tester);
    await tester.enterText(find.byKey(const ValueKey('kube-forward-remote')), '65536');
    await settle(tester);
    expect(isEnabled(tester, 'kube-forward-start'), isFalse);
    await tester.enterText(find.byKey(const ValueKey('kube-forward-remote')), '8080');
    await settle(tester);
    expect(isEnabled(tester, 'kube-forward-start'), isTrue);
    await tester.tap(find.byKey(const ValueKey('kube-forward-start')));
    await settle(tester);
    expect(service.started.single, (remote: 8080, local: 0, pod: 'demo-pod'));
    container.read(_scope.notifier).changeProfile();
    await settle(tester);
    service.pending!.complete(_forward);
    await settle(tester);
    expect(service.stopped, ['forward-one']);
    expect(find.byKey(const ValueKey('kube-forward-dialog')), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('port forward appears in cluster panel and stop revokes exact native identity', (tester) async {
    final service = _ForwardService();
    await _pump(tester, service);
    await tester.tap(find.byKey(const ValueKey('kube-resource-demo-pod')));
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('kube-pod-forward')));
    await settle(tester);
    await tester.enterText(find.byKey(const ValueKey('kube-forward-remote')), '8080');
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('kube-forward-start')));
    await settle(tester);
    expect(find.text('127.0.0.1:34567'), findsOneWidget);
    await tester.tap(find.text('Close').last);
    await settle(tester);
    await tester.tap(find.text('Close').last);
    await settle(tester);
    final row = find.byKey(const ValueKey('kube-forward-forward-one'));
    expect(row, findsOneWidget);
    await tester.tap(find.descendant(of: row, matching: find.byType(GlassIconButton)));
    await settle(tester);
    expect(service.stopped, ['forward-one']);
    expect(row, findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('debug container creation validates image and exact Pod name and sends version precondition', (
    tester,
  ) async {
    final service = _OperationsService();
    await _pump(tester, service);
    await tester.tap(find.byKey(const ValueKey('kube-resource-demo-pod')));
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('kube-operation-debug')));
    await settle(tester);
    await tester.enterText(find.byKey(const ValueKey('kube-debug-image')), 'busybox:1.36');
    await tester.enterText(find.byKey(const ValueKey('kube-operation-confirm-name')), 'wrong');
    await settle(tester);
    expect(isEnabled(tester, 'kube-operation-confirm'), isFalse);
    await tester.enterText(find.byKey(const ValueKey('kube-operation-confirm-name')), 'demo-pod');
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('kube-operation-confirm')));
    await settle(tester);
    expect(service.debugged.single.version, '10');
    expect(service.debugged.single.container.image, 'busybox:1.36');
    expect(service.debugged.single.container.targetContainer, 'web');
    expect(tester.takeException(), isNull);
  });

  test('native Table columns preserve values without retaining full objects in row memory', () {
    final table = KubeResourceTable.fromJson({
      'columns': [
        {'name': 'Name', 'type': 'string', 'priority': 0},
        {'name': 'Ready', 'type': 'string', 'priority': 0},
      ],
      'rows': [
        {
          'cells': ['demo', true],
          'resource': {
            'name': 'demo',
            'uid': 'u',
            'resource_version': '7',
            'object': {'large': 'discard'},
          },
        },
      ],
      'resource_version': '7',
      'continue_token': 'next',
    });
    expect(table.columns.last.name, 'Ready');
    expect(table.page.items.single.cells, ['demo', 'true']);
    expect(table.page.items.single.object, isEmpty);
    expect(table.page.continueToken, 'next');
  });

  for (final language in ['en', 'ru']) {
    testWidgets('GitOps, partial diagnostics and Helm history remain usable on a phone in $language', (tester) async {
      final service = _InsightsService();
      await _pump(tester, service, size: const Size(412, 915), language: language, textScale: 1.3);
      await tester.ensureVisible(find.byKey(const ValueKey('kube-view-problems')));
      await tester.tap(find.byKey(const ValueKey('kube-view-problems')));
      await settle(tester);
      expect(find.byKey(const ValueKey('kube-problems-list')), findsOneWidget);
      expect(find.textContaining(language == 'ru' ? 'Результаты неполные' : 'Results are incomplete'), findsOneWidget);
      await tester.ensureVisible(find.byKey(const ValueKey('kube-view-gitops')));
      await tester.tap(find.byKey(const ValueKey('kube-view-gitops')));
      await settle(tester);
      expect(find.text('demo-flux'), findsOneWidget);
      expect(
        tester.widget<PopupMenuButton<String>>(find.byKey(const ValueKey('kube-gitops-action-demo-flux'))).enabled,
        isFalse,
      );
      await tester.ensureVisible(find.byKey(const ValueKey('kube-gitops-dependencies')));
      await tester.tap(find.byKey(const ValueKey('kube-gitops-dependencies')));
      await settle(tester);
      expect(find.byKey(const ValueKey('kube-dependency-list')), findsOneWidget);
      await tester.ensureVisible(find.byKey(const ValueKey('kube-view-helm')));
      await tester.tap(find.byKey(const ValueKey('kube-view-helm')));
      await settle(tester);
      expect(find.byKey(const ValueKey('kube-helm-default/sh.helm.release.v1.web.v1')), findsNothing);
      await tester.tap(find.byKey(const ValueKey('kube-helm-default/sh.helm.release.v1.web.v2')));
      await settle(tester);
      expect(find.byKey(const ValueKey('kube-helm-details')), findsOneWidget);
      expect(find.textContaining('[redacted]'), findsOneWidget);
      await tester.ensureVisible(find.byKey(const ValueKey('kube-helm-diff-tab')));
      await tester.tap(find.byKey(const ValueKey('kube-helm-diff-tab')));
      await settle(tester);
      await tester.tap(find.byKey(const ValueKey('kube-helm-compare')));
      await settle(tester);
      await tester.tap(find.text(language == 'ru' ? 'Ревизия 1' : 'Revision 1').last);
      await settle(tester);
      expect(find.byKey(const ValueKey('kube-yaml-diff')), findsOneWidget);
      expect(service.releaseReads.last, 'sh.helm.release.v1.web.v1');
      expect(tester.takeException(), isNull);
    });
  }

  testWidgets('Flux suspend requires an exact resource name and an audit reason', (tester) async {
    final service = _InsightsService()..access = const KubeWriteAccess(production: false);
    await _pump(tester, service);
    await tester.tap(find.byKey(const ValueKey('kube-view-gitops')));
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('kube-gitops-action-demo-flux')));
    await settle(tester);
    await tester.tap(find.text('Suspend reconciliation'));
    await settle(tester);
    await tester.enterText(find.byKey(const ValueKey('kube-gitops-confirm-name')), 'demo-flux');
    await settle(tester);
    expect(tester.widget<GlassButton>(find.byKey(const ValueKey('kube-gitops-confirm-action'))).onPressed, isNull);
    await tester.enterText(find.byKey(const ValueKey('kube-gitops-reason')), 'Maintenance window');
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('kube-gitops-confirm-action')));
    await settle(tester);
    expect(service.fluxActions.single, (
      name: 'demo-flux',
      action: KubeFluxAction.suspend,
      reason: 'Maintenance window',
    ));
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
  testWidgets('live logs are bounded, pause upstream and are revoked on lock', (tester) async {
    final service = _LogService();
    final container = await _pump(tester, service, size: const Size(412, 915), language: 'ru', textScale: 1.3);
    await tester.tap(find.byKey(const ValueKey('kube-resource-demo-pod')));
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('kube-logs-tab')));
    await settle(tester);
    await tester.ensureVisible(find.byKey(const ValueKey('kube-log-follow')));
    await tester.tap(find.byKey(const ValueKey('kube-log-follow')));
    await settle(tester);
    expect(service.streams, hasLength(1));
    service.streams.single.add(List.generate(20005, (i) => 'line-${i.toString().padLeft(5, '0')}').join('\n'));
    await settle(tester);
    expect(find.text('line-00000'), findsNothing);
    final logList = tester.widget<ListView>(find.byKey(const ValueKey('kube-log-lines')));
    final delegate = logList.childrenDelegate as SliverChildBuilderDelegate;
    expect(delegate.estimatedChildCount, 20000);
    await tester.ensureVisible(find.byKey(const ValueKey('kube-log-pause')));
    await tester.tap(find.byKey(const ValueKey('kube-log-pause')));
    await settle(tester);
    expect(service.streams.single.hasListener, isFalse);
    await tester.tap(find.byKey(const ValueKey('kube-log-pause')));
    await settle(tester);
    expect(service.streams, hasLength(2));
    expect(service.streams.last.hasListener, isTrue);
    expect(tester.takeException(), isNull);
    container.read(_scope.notifier).lock();
    await settle(tester);
    expect(service.streams.last.hasListener, isFalse);
    expect(find.text('line-00005'), findsNothing);
    expect(tester.takeException(), isNull);
  });
  testWidgets('production write window requires an exact context name and can be closed', (tester) async {
    final service = _MutationService();
    await _pump(tester, service);
    await tester.tap(find.byKey(const ValueKey('kube-write-window')));
    await settle(tester);
    expect(tester.widget<GlassButton>(find.byKey(const ValueKey('kube-confirm-action'))).onPressed, isNull);
    await tester.enterText(find.byKey(const ValueKey('kube-confirm-name')), 'demo-cluster ');
    await settle(tester);
    expect(tester.widget<GlassButton>(find.byKey(const ValueKey('kube-confirm-action'))).onPressed, isNull);
    await tester.enterText(find.byKey(const ValueKey('kube-confirm-name')), 'demo-cluster');
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('kube-confirm-action')));
    await settle(tester);
    expect(service.openedNames, ['demo-cluster']);
    expect(find.textContaining('changes allowed'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('kube-write-window')));
    await settle(tester);
    expect(service.closedWindows, 1);
    expect(find.text('Production · read only'), findsOneWidget);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('mobile cannot request a production write window', (tester) async {
    final service = _MutationService();
    await _pump(tester, service, size: const Size(412, 915));
    expect(find.byKey(const ValueKey('kube-write-window')), findsNothing);
    await tester.tap(find.byKey(const ValueKey('kube-resource-demo-pod')));
    await settle(tester);
    expect(tester.widget<GlassButton>(find.byKey(const ValueKey('kube-edit-yaml'))).onPressed, isNull);
    expect(service.openedNames, isEmpty);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.android));

  testWidgets('YAML requires server validation, reviewed source and typed confirmation before apply', (tester) async {
    final service = _MutationService()..access = const KubeWriteAccess(production: false);
    await _pump(tester, service);
    await tester.tap(find.byKey(const ValueKey('kube-resource-demo-pod')));
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('kube-edit-yaml')));
    await settle(tester);
    expect(tester.widget<GlassButton>(find.byKey(const ValueKey('kube-yaml-apply'))).onPressed, isNull);
    await tester.enterText(
      find.byKey(const ValueKey('kube-yaml-input')),
      'kind: Pod\nmetadata:\n  name: demo-pod\n  resourceVersion: "10"\n',
    );
    await tester.tap(find.byKey(const ValueKey('kube-yaml-validate')));
    await settle(tester);
    expect(service.applies.map((a) => a.dryRun), [true]);
    expect(service.applies.single.manifest.isWiped, isTrue);
    expect(find.byKey(const ValueKey('kube-yaml-diff')), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('kube-yaml-apply')));
    await settle(tester);
    await tester.enterText(find.byKey(const ValueKey('kube-confirm-name')), 'demo-pod');
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('kube-confirm-action')));
    await settle(tester);
    expect(service.applies.map((a) => a.dryRun), [true, false]);
    expect(service.applies.last.manifest.isWiped, isTrue);
    expect(find.byKey(const ValueKey('kube-yaml-editor')), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('revoked write access prevents an already reviewed YAML from being applied', (tester) async {
    final service = _MutationService()..access = const KubeWriteAccess(production: false);
    final container = await _pump(tester, service);
    await tester.tap(find.byKey(const ValueKey('kube-resource-demo-pod')));
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('kube-edit-yaml')));
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('kube-yaml-validate')));
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('kube-yaml-apply')));
    await settle(tester);
    service.access = const KubeWriteAccess(production: true);
    container.invalidate(kubeWriteAccessProvider);
    await settle(tester);
    await tester.enterText(find.byKey(const ValueKey('kube-confirm-name')), 'demo-pod');
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('kube-confirm-action')));
    await settle(tester);
    expect(service.applies.map((a) => a.dryRun), [true]);
    expect(tester.widget<GlassButton>(find.byKey(const ValueKey('kube-yaml-apply'))).onPressed, isNull);
    expect(tester.takeException(), isNull);
  });

  testWidgets('resource deletion sends exact identity preconditions only after confirmation', (tester) async {
    final service = _MutationService()..access = const KubeWriteAccess(production: false);
    await _pump(tester, service);
    await tester.tap(find.byKey(const ValueKey('kube-resource-demo-pod')));
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('kube-delete-resource')));
    await settle(tester);
    expect(service.deleted, isEmpty);
    await tester.enterText(find.byKey(const ValueKey('kube-confirm-name')), 'demo-pod');
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('kube-confirm-action')));
    await settle(tester);
    expect(service.deleted.single, (name: 'demo-pod', uid: 'demo-pod-uid', version: '10'));
    expect(find.byKey(const ValueKey('kube-resource-details')), findsNothing);
    expect(tester.takeException(), isNull);
  });
  for (final language in ['en', 'ru']) {
    for (final width in [412.0, 1100.0]) {
      testWidgets('cluster browser $language at $width shows details, redacted YAML and pod logs', (tester) async {
        final service = _Service();
        await _pump(tester, service, language: language, size: Size(width, 915), textScale: width == 412 ? 1.3 : 1);
        expect(tester.takeException(), isNull, reason: 'initial browser');
        expect(find.text('demo-pod'), findsOneWidget);
        expect(service.requests.single.namespace, 'default');
        expect(service.connected, ['context']);
        expect(find.text(language == 'ru' ? 'Прод · только чтение' : 'Production · read only'), findsOneWidget);
        await tester.tap(find.byKey(const ValueKey('kube-resource-demo-pod')));
        await settle(tester);
        expect(find.byKey(const ValueKey('kube-resource-details')), findsOneWidget);
        expect(tester.takeException(), isNull, reason: 'resource overview');
        await tester.tap(find.byKey(const ValueKey('kube-yaml-tab')));
        await settle(tester);
        expect(find.text('kind: Pod\nmetadata:\n  name: demo-pod\n'), findsOneWidget);
        expect(tester.takeException(), isNull, reason: 'resource YAML');
        await tester.tap(find.byKey(const ValueKey('kube-logs-tab')));
        await settle(tester);
        expect(tester.takeException(), isNull, reason: 'resource logs');
        expect(service.logsCalls.length, 1);
        expect(service.logsCalls.single.container, 'web');
        expect(find.text('2026-10-20T10:00:00Z ready'), findsOneWidget);
        await tester.enterText(find.byKey(const ValueKey('kube-log-filter')), 'failed');
        await settle(tester);
        expect(find.text('2026-10-20T10:00:00Z ready'), findsNothing);
        await tester.ensureVisible(find.byKey(const ValueKey('kube-log-previous')));
        await tester.tap(find.byKey(const ValueKey('kube-log-previous')));
        await settle(tester);
        expect(service.logsCalls.last.previous, isTrue);
        expect(tester.takeException(), isNull);
        await tester.pumpWidget(const SizedBox.shrink());
        await settle(tester);
        expect(service.disconnected, ['opaque-native-session']);
      });
    }
  }

  testWidgets('late connection is revoked after profile switch and never discovered', (tester) async {
    final service = _Service()..connectPending = Completer<KubeSession>();
    final container = await _pump(tester, service);
    expect(find.byType(KubeBrowser), findsOneWidget);
    container.read(_scope.notifier).changeProfile();
    await settle(tester);
    expect(find.byType(KubeBrowser), findsNothing);
    service.connectPending!.complete(_session);
    await settle(tester);
    expect(service.disconnected, ['opaque-native-session']);
    expect(service.discoverCalls, 0);
    expect(find.text('demo-pod'), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('lock closes detail dialog, native session and live subscription', (tester) async {
    final service = _Service();
    final container = await _pump(tester, service);
    expect(service.watches.single.hasListener, isTrue);
    await tester.tap(find.byKey(const ValueKey('kube-resource-demo-pod')));
    await settle(tester);
    container.read(_scope.notifier).lock();
    await settle(tester);
    expect(find.byKey(const ValueKey('kube-resource-details')), findsNothing);
    expect(find.text('demo-pod'), findsNothing);
    expect(service.watches.single.hasListener, isFalse);
    expect(service.disconnected, ['opaque-native-session']);
    expect(tester.takeException(), isNull);
  });

  testWidgets('pagination automatically merges the snapshot before watching and list stays virtual', (tester) async {
    final service = _Service()
      ..pages.addAll([
        _page([_row('demo-pod')], next: 'next-page'),
        _page([
          _row('demo-pod', status: 'Ready'),
          ...List.generate(9999, (i) => _row('pod-${i.toString().padLeft(5, '0')}')),
        ]),
      ]);
    await _pump(tester, service);
    expect(service.requests.last.continueToken, 'next-page');
    expect(service.watches, hasLength(1));
    expect(service.watchVersions, ['10']);
    expect(
      find.text('10,000 resources loaded'),
      findsNothing,
    ); // Localized message is intentionally not locale-formatted.
    expect(find.text('10000 resources loaded'), findsOneWidget);
    expect(find.byType(ListTile).evaluate().length, lessThan(50));
    expect(tester.takeException(), isNull);
  });

  testWidgets('pagination shows the first page while the next page waits and prevents duplicate requests', (
    tester,
  ) async {
    final pending = Completer<KubeResourcePage>();
    final service = _Service()
      ..pages.add(_page([_row('first-pod')], next: 'next-page'))
      ..continuationPending = pending;
    await _pump(tester, service);
    expect(find.text('first-pod'), findsOneWidget);
    expect(service.requests, hasLength(2));
    expect(service.watches, isEmpty);
    expect(isEnabled(tester, 'kube-load-more'), isFalse);
    await settle(tester);
    expect(service.requests, hasLength(2));
    pending.complete(_page([_row('second-pod')]));
    await settle(tester);
    expect(find.text('second-pod'), findsOneWidget);
    expect(service.watchVersions, ['10']);
    expect(tester.takeException(), isNull);
  });

  for (final cancellation in ['lock', 'background', 'view']) {
    testWidgets('pagination drops pending pages after $cancellation', (tester) async {
      final pending = Completer<KubeResourcePage>();
      final service = _InsightsService()
        ..pages.add(_page([_row('first-pod')], next: 'next-page'))
        ..continuationPending = pending;
      final container = await _pump(tester, service);
      if (cancellation == 'lock') {
        container.read(_scope.notifier).lock();
      } else if (cancellation == 'background') {
        tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.paused);
        addTearDown(() => tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed));
      } else {
        await tester.tap(find.byKey(const ValueKey('kube-view-gitops')));
      }
      await settle(tester);
      pending.complete(_page([_row('stale-pod')], next: 'another-page'));
      await settle(tester);
      expect(service.requests, hasLength(2));
      expect(service.watches, isEmpty);
      expect(find.text('stale-pod'), findsNothing);
      expect(tester.takeException(), isNull);
    });
  }

  testWidgets('pagination errors stop automatic retries while preserving the first page', (tester) async {
    final pending = Completer<KubeResourcePage>();
    final service = _Service()
      ..pages.add(_page([_row('first-pod')], next: 'next-page'))
      ..continuationPending = pending;
    await _pump(tester, service);
    pending.completeError(StateError('private-server-error'));
    await settle(tester);
    expect(service.requests, hasLength(2));
    expect(service.watches, isEmpty);
    expect(find.text('first-pod'), findsOneWidget);
    expect(find.textContaining('private-server-error'), findsNothing);
    expect(isEnabled(tester, 'kube-load-more'), isTrue);
    await settle(tester);
    expect(service.requests, hasLength(2));
    expect(tester.takeException(), isNull);
  });

  testWidgets('pagination rejects a changing snapshot version instead of merging or watching it', (tester) async {
    final service = _Service()
      ..pages.addAll([
        _page([_row('first-pod')], next: 'next-page'),
        KubeResourcePage(items: [_row('inconsistent-pod')], resourceVersion: '20'),
      ]);
    await _pump(tester, service);
    expect(service.requests, hasLength(2));
    expect(service.watches, isEmpty);
    expect(find.text('first-pod'), findsOneWidget);
    expect(find.text('inconsistent-pod'), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('pagination stops repeated tokens without looping', (tester) async {
    final service = _Service()
      ..pages.addAll([
        _page([_row('first-pod')], next: 'repeated'),
        _page([_row('second-pod')], next: 'repeated'),
      ]);
    await _pump(tester, service);
    expect(service.requests, hasLength(2));
    expect(service.watches, isEmpty);
    expect(find.text('second-pod'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('pagination bounds empty unique pages as well as loaded resource count', (tester) async {
    final service = _Service()..pages.addAll(List.generate(110, (i) => _page(const [], next: 'page-$i')));
    await _pump(tester, service);
    expect(service.requests, hasLength(100));
    expect(service.watches, isEmpty);
    expect(find.byKey(const ValueKey('kube-load-more')), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('watch updates and deletes rows and relists after expiration', (tester) async {
    final service = _Service();
    await _pump(tester, service);
    // Closed by service.dispose registered in _pump.
    // ignore: close_sinks
    final watch = service.watches.single;
    watch.add(
      KubeWatchEvent(
        type: KubeWatchEventType.modified,
        resource: _row('demo-pod', status: 'Pending'),
      ),
    );
    await settle(tester);
    expect(find.text('Pending'), findsOneWidget);
    watch.add(KubeWatchEvent(type: KubeWatchEventType.deleted, resource: _row('demo-pod')));
    await settle(tester);
    expect(find.text('demo-pod'), findsNothing);
    service.pages.add(_page([_row('relisted-pod')]));
    watch.add(const KubeWatchEvent(type: KubeWatchEventType.relist));
    await settle(tester);
    expect(find.text('relisted-pod'), findsOneWidget);
    expect(service.requests, hasLength(2));
    expect(watch.hasListener, isFalse);
    expect(tester.takeException(), isNull);
  });

  testWidgets('background stops watch and foreground refreshes instead of reusing stale rows', (tester) async {
    final service = _Service();
    await _pump(tester, service);
    // Closed by service.dispose registered in _pump.
    // ignore: close_sinks
    final watch = service.watches.single;
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.inactive);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.hidden);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.paused);
    await settle(tester);
    expect(watch.hasListener, isFalse);
    service.pages.add(_page([_row('fresh-pod')]));
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.hidden);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.inactive);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
    await settle(tester);
    expect(find.text('fresh-pod'), findsOneWidget);
    expect(find.text('demo-pod'), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('connection and request errors never display backend text', (tester) async {
    final service = _Service()..connectError = StateError('synthetic-secret-token');
    await _pump(tester, service);
    expect(find.textContaining('synthetic-secret-token'), findsNothing);
    expect(find.byKey(const ValueKey('kube-reconnect')), findsOneWidget);
    service.connectError = null;
    service.listError = StateError('private-server-detail');
    await tester.tap(find.byKey(const ValueKey('kube-reconnect')));
    await settle(tester);
    expect(find.textContaining('private-server-detail'), findsNothing);
    expect(find.text('Could not load cluster data. Check the connection and Kubernetes permissions.'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('cluster-scoped kind omits namespace and stale page is ignored', (tester) async {
    final service = _Service();
    await _pump(tester, service);
    final pending = Completer<KubeResourcePage>();
    service.listPending = pending;
    await tester.tap(find.byKey(const ValueKey('kube-refresh-resources')));
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('kube-kind')));
    await settle(tester);
    service.listPending = null;
    service.pages.add(_page([_row('node-current')]));
    await tester.tap(find.text('Node · v1').last);
    await settle(tester);
    expect(service.requests.last.namespace, isNull);
    expect(service.requests.last.kind.resource, 'nodes');
    pending.complete(_page([_row('stale-pod')]));
    await settle(tester);
    expect(find.text('node-current'), findsOneWidget);
    expect(find.text('stale-pod'), findsNothing);
    expect(tester.takeException(), isNull);
  });
}

/// Mirrors native ResourceApi: namespace lookup resolves the discovery registry,
/// which is empty until the asynchronous discovery request has completed.
class _ColdDiscoveryService extends _Service {
  final discovery = Completer<void>();
  bool _discovered = false;
  int namespaceCalls = 0;

  @override
  Future<List<KubeResourceKind>> discover(String sessionId) async {
    await discovery.future;
    final kinds = await super.discover(sessionId);
    _discovered = true;
    return kinds;
  }

  @override
  Future<List<String>> namespaces(String sessionId) async {
    namespaceCalls++;
    if (!_discovered) throw StateError('UnknownResource: namespace discovery is incomplete');
    return super.namespaces(sessionId);
  }
}

class _Service implements KubeService, KubeBrowserService, KubeWatchService {
  final connected = <String>[];
  final disconnected = <String>[];
  final requests = <({KubeResourceKind kind, String? namespace, String? continueToken})>[];
  final logsCalls = <({String? container, bool previous})>[];
  final pages = <KubeResourcePage>[];
  final watches = <StreamController<KubeWatchEvent>>[];
  final watchVersions = <String>[];
  Completer<KubeSession>? connectPending;
  Completer<KubeResourcePage>? listPending;
  Completer<KubeResourcePage>? continuationPending;
  StateError? connectError, listError;
  int discoverCalls = 0;
  KubeSession session = _session;

  Future<void> dispose() async {
    for (final watch in watches) {
      await watch.close();
    }
  }

  @override
  Future<KubeSession> connect(String contextId) async {
    connected.add(contextId);
    if (connectError != null) throw connectError!;
    return connectPending?.future ?? session;
  }

  @override
  Future<void> disconnect(String sessionId) async => disconnected.add(sessionId);
  @override
  Future<List<KubeResourceKind>> discover(String sessionId) async {
    discoverCalls++;
    return [_pods, _nodes];
  }

  @override
  Future<List<String>> namespaces(String sessionId) async => ['default', 'kube-system'];
  @override
  Future<KubeResourcePage> listResources(
    String sessionId,
    KubeResourceKind kind, {
    String? namespace,
    String? labelSelector,
    String? continueToken,
    int limit = 200,
  }) async {
    requests.add((kind: kind, namespace: namespace, continueToken: continueToken));
    if (listError != null) throw listError!;
    if (continueToken != null && continuationPending != null) return continuationPending!.future;
    return listPending?.future ?? (pages.isNotEmpty ? pages.removeAt(0) : _page([_row('demo-pod')]));
  }

  @override
  Future<KubeResourceDetail> getResource(String sessionId, KubeResourceRef resource) async => const KubeResourceDetail(
    object: {
      'metadata': {'name': 'demo-pod', 'namespace': 'default', 'uid': 'demo-pod-uid', 'resourceVersion': '10'},
      'status': {'phase': 'Running'},
    },
    yaml: 'kind: Pod\nmetadata:\n  name: demo-pod\n',
    containers: ['web', 'sidecar'],
  );
  @override
  Future<String> logs(
    String sessionId,
    KubeResourceRef resource, {
    String? container,
    bool previous = false,
    bool timestamps = true,
    int tailLines = 500,
  }) async {
    logsCalls.add((container: container, previous: previous));
    return '2026-10-20T10:00:00Z ready';
  }

  @override
  Stream<KubeWatchEvent> watchResources(
    String sessionId,
    KubeResourceKind kind, {
    String? namespace,
    String? labelSelector,
    required String resourceVersion,
  }) {
    watchVersions.add(resourceVersion);
    final controller = StreamController<KubeWatchEvent>();
    watches.add(controller);
    return controller.stream;
  }

  @override
  Stream<void> changes() => const Stream.empty();
  @override
  Future<List<KubeConnection>> inventory() async => [_connection];
  @override
  Future<void> commit(String ticket, List<KubeImportChoice> choices) async {}
  @override
  Future<void> deleteContext(String id) async {}
  @override
  Future<void> discard(String ticket) async {}
  @override
  Future<KubeImportPreview> preview(SecretText yaml, {String? sourceFile}) => throw UnimplementedError();
  @override
  Future<KubeImportPreview> previewFile(String path) => throw UnimplementedError();
}

class _MutationService extends _Service implements KubeMutationService {
  KubeWriteAccess access = const KubeWriteAccess(production: true, canRequestWindow: true);
  final openedNames = <String>[];
  final applies = <({bool dryRun, bool force, SecretText manifest})>[];
  final deleted = <({String name, String uid, String version})>[];
  int closedWindows = 0;
  @override
  Future<KubeWriteAccess> writeAccess(String sessionId) async => access;
  @override
  Future<KubeWriteAccess> requestWriteWindow(String sessionId, String typedContextName) async {
    openedNames.add(typedContextName);
    return access = KubeWriteAccess(
      production: true,
      canRequestWindow: true,
      expiresAt: DateTime.now().toUtc().add(const Duration(minutes: 10)),
    );
  }

  @override
  Future<void> closeWriteWindow(String sessionId) async {
    closedWindows++;
    access = const KubeWriteAccess(production: true, canRequestWindow: true);
  }

  @override
  Future<KubeResourceDetail> applyYaml(
    String sessionId,
    KubeResourceRef resource,
    SecretText manifest, {
    bool dryRun = true,
    bool force = false,
  }) async {
    applies.add((dryRun: dryRun, force: force, manifest: manifest));
    return KubeResourceDetail(object: const {}, yaml: manifest.expose());
  }

  @override
  Future<void> deleteResource(
    String sessionId,
    KubeResourceRef resource, {
    required String uid,
    required String resourceVersion,
  }) async {
    deleted.add((name: resource.name, uid: uid, version: resourceVersion));
  }
}

class _LogService extends _Service implements KubeLogService {
  final streams = <StreamController<String>>[];
  int pauses = 0, resumes = 0;
  @override
  Stream<String> followLogs(
    String sessionId,
    KubeResourceRef resource, {
    String? container,
    bool previous = false,
    bool timestamps = true,
    int tailLines = 500,
  }) {
    final stream = StreamController<String>(onPause: () => pauses++, onResume: () => resumes++);
    streams.add(stream);
    return stream.stream;
  }

  @override
  Future<void> dispose() async {
    await super.dispose();
    for (final stream in streams) {
      await stream.close();
    }
  }
}

const _flux = KubeResourceKind(
  group: 'kustomize.toolkit.fluxcd.io',
  version: 'v1',
  resource: 'kustomizations',
  kind: 'Kustomization',
  namespaced: true,
  verbs: ['get', 'list', 'watch', 'patch'],
);

class _InsightsService extends _MutationService implements KubeInsightsService {
  final releaseReads = <String>[];
  final fluxActions = <({String name, KubeFluxAction action, String? reason})>[];
  @override
  Future<List<KubeResourceKind>> discover(String sessionId) async => [_pods, _nodes, _flux];
  @override
  Future<KubeInsights> insights(String sessionId, {String? namespace}) async => KubeInsights(
    partial: true,
    issues: const [
      KubeIssue(code: 'pod_crash_loop', severity: 'error', kind: 'Pod', name: 'demo-pod', namespace: 'default'),
    ],
    resources: [
      KubeInsightResource(api: _pods, resource: _row('demo-pod')),
      const KubeInsightResource(
        api: _flux,
        resource: KubeResourceRow(
          name: 'demo-flux',
          uid: 'demo-flux',
          resourceVersion: '11',
          namespace: 'default',
          object: {
            'status': {
              'conditions': [
                {'type': 'Ready', 'status': 'True'},
              ],
            },
          },
        ),
      ),
    ],
    gitops: const KubeGitOpsOverview(
      ready: 1,
      dependencies: [
        KubeDependency(
          from: KubeObjectIdentity(kind: 'Kustomization', namespace: 'default', name: 'demo-flux'),
          to: KubeObjectIdentity(kind: 'Kustomization', namespace: 'default', name: 'missing-source'),
          ready: false,
          missing: true,
        ),
      ],
    ),
  );
  @override
  Future<List<KubeHelmSummary>> helmHistory(String sessionId, {String? namespace, String? releaseName}) async => [
    for (final revision in [1, 2])
      KubeHelmSummary(
        name: 'web',
        namespace: 'default',
        revision: revision,
        status: revision == 2 ? 'deployed' : 'superseded',
        chart: 'web-chart',
        chartVersion: '1.$revision',
        appVersion: '2.0',
        secretName: 'sh.helm.release.v1.web.v$revision',
      ),
  ];
  @override
  Future<KubeHelmRelease> helmRelease(String sessionId, String namespace, String secretName) async {
    releaseReads.add(secretName);
    final revision = secretName.endsWith('v1') ? 1 : 2;
    return KubeHelmRelease(
      name: 'web',
      namespace: 'default',
      revision: revision,
      status: 'deployed',
      chart: 'web-chart',
      chartVersion: '1.$revision',
      appVersion: '2.0',
      values: const {'password': '[redacted]'},
      chartValues: const {},
      manifest: 'kind: Deployment\nspec:\n  replicas: $revision\n',
    );
  }

  @override
  Future<void> fluxAction(String sessionId, KubeResourceRef resource, KubeFluxAction action, {String? reason}) async =>
      fluxActions.add((name: resource.name, action: action, reason: reason));
  @override
  Future<void> argoAction(String sessionId, KubeResourceRef resource, KubeArgoAction action) async {}
}

const _forward = KubeForward(
  id: 'forward-one',
  sessionId: 'opaque-native-session',
  contextName: 'demo-cluster',
  namespace: 'default',
  pod: 'demo-pod',
  remotePort: 8080,
  address: '127.0.0.1:34567',
  state: 'running',
);

class _ForwardService extends _Service implements KubeForwardService {
  _ForwardService() {
    session = const KubeSession(
      id: 'opaque-native-session',
      contextId: 'context',
      serverVersion: 'v1.32.1',
      production: false,
    );
  }
  final started = <({int remote, int local, String pod})>[];
  final stopped = <String>[];
  final active = <KubeForward>[];
  Completer<KubeForward>? pending;
  @override
  Future<KubeForward> startForward(
    String sessionId, {
    required String namespace,
    required String pod,
    required int remotePort,
    int localPort = 0,
  }) async {
    started.add((remote: remotePort, local: localPort, pod: pod));
    final result = await (pending?.future ?? Future.value(_forward));
    active.add(result);
    return result;
  }

  @override
  Future<List<KubeForward>> forwards() async => [...active];
  @override
  Future<void> stopForward(String sessionId, String forwardId) async {
    stopped.add(forwardId);
    active.removeWhere((f) => f.id == forwardId);
  }
}

class _PodTerminalService implements TerminalService, KubernetesTerminalService {
  final targets = <KubernetesTerminalTarget>[];
  final written = <int>[];
  final closed = <TerminalSessionId>[];
  @override
  Future<TerminalSessionHandle> openKubernetes({
    required KubernetesTerminalTarget target,
    required TerminalSize size,
  }) async {
    targets.add(target);
    return TerminalSessionHandle(
      id: TerminalSessionId.generate(),
      output: const Stream.empty(),
      events: Stream.value(const TerminalStateChanged(SessionConnectionState.connected)),
    );
  }

  @override
  Future<TerminalSessionHandle> open({required ObjectId hostId, required TerminalSize size}) =>
      throw UnimplementedError();
  @override
  Future<void> write(TerminalSessionId id, Uint8List data) async => written.addAll(data);
  @override
  Future<void> resize(TerminalSessionId id, TerminalSize size) async {}
  @override
  Future<void> close(TerminalSessionId id) async => closed.add(id);
  @override
  Future<void> reconnect(TerminalSessionId id) async {}
  @override
  Future<void> answerHostKey(TerminalSessionId id, HostKeyDecision decision) async {}
  @override
  Future<void> answerPassword(TerminalSessionId id, SecretText? password) async => password?.wipe();
}

class _OperationsService extends _MutationService implements KubeOperationsService {
  _OperationsService() {
    access = const KubeWriteAccess(production: false);
  }
  final debugged = <({String version, KubeDebugContainer container})>[];
  @override
  Future<KubePermissionReport> permissions(String sessionId, String namespace) async =>
      const KubePermissionReport(incomplete: false, canMutate: true, rules: []);
  @override
  Future<void> scale(String sessionId, KubeResourceRef resource, String resourceVersion, int replicas) async {}
  @override
  Future<void> restart(String sessionId, KubeResourceRef resource, String resourceVersion) async {}
  @override
  Future<void> cordon(String sessionId, String node, String resourceVersion, bool unschedulable) async {}
  @override
  Future<List<KubeDrainCandidate>> drainPlan(String sessionId, String node) async => [];
  @override
  Future<void> evictPod(String sessionId, KubeResourceRef resource, String uid, String resourceVersion) async {}
  @override
  Future<void> debugContainer(
    String sessionId,
    KubeResourceRef resource,
    String resourceVersion,
    KubeDebugContainer container,
  ) async => debugged.add((version: resourceVersion, container: container));
  @override
  Future<List<KubeResourceRow>> workloadPods(String sessionId, KubeResourceRef resource) async => [];
  @override
  Future<List<KubeResourceRow>> objectEvents(String sessionId, KubeResourceRef resource, String uid) async => [];
}

class _AiService extends _Service implements KubeAiService {
  final questions = <String>[];
  final events = StreamController<KubeAiEvent>.broadcast();
  int asked = 0, revoked = 0;
  @override
  Future<KubeAiReview> prepareKubeAi(
    String sessionId, {
    String? providerId,
    required String question,
    KubeResourceRef? selected,
    bool configMapValues = false,
  }) async {
    questions.add(question);
    return const KubeAiReview(
      ticket: 'review',
      providerName: 'External test provider',
      contextName: 'demo-cluster',
      external: true,
      toolSupport: true,
    );
  }

  @override
  Stream<KubeAiEvent> askKubeAi(String sessionId, KubeAiReview review, {required bool approveExternal}) {
    expect(approveExternal, isTrue);
    asked++;
    return events.stream;
  }

  @override
  Future<void> discardKubeAi(String sessionId, String ticket) async {}
  @override
  Future<void> revokeKubeAi(String sessionId) async => revoked++;
  @override
  Future<void> dispose() async {
    await super.dispose();
    await events.close();
  }
}

class _ManifestService extends _MutationService implements KubeManifestService {
  _ManifestService() {
    access = const KubeWriteAccess(production: false);
  }
  final revealed = <SecretText>[];
  final creates = <({bool dryRun, SecretText manifest})>[];
  KubeJson schema = const {
    'schema': {'type': 'object'},
  };
  @override
  Future<KubeJson> resourceSchema(String sessionId, KubeResourceKind kind) async => schema;
  @override
  Future<KubeJson> compareManifest(String sessionId, KubeResourceRef resource, SecretText manifest) async => {
    'changes': <KubeJson>[],
    'truncated': false,
  };
  @override
  Future<KubeResourceDetail> createResource(
    String sessionId,
    KubeResourceRef resource,
    SecretText manifest, {
    bool dryRun = true,
  }) async {
    creates.add((dryRun: dryRun, manifest: manifest));
    return KubeResourceDetail(object: const {}, yaml: manifest.expose(), containers: const []);
  }

  @override
  Future<SecretText> revealSecret(String sessionId, KubeResourceRef resource) async {
    final secret = SecretText(jsonEncode({'password': base64Encode(utf8.encode('private-value'))}));
    revealed.add(secret);
    return secret;
  }
}

class _RolloutService extends _Service implements KubeRolloutService {
  final compared = <(int, int)>[];
  @override
  Future<List<KubeJson>> rolloutHistory(String sessionId, KubeResourceRef resource) async => [
    {
      'revision': 1,
      'name': 'rs1',
      'template': {'image': 'old'},
    },
    {
      'revision': 2,
      'name': 'rs2',
      'template': {'image': 'new'},
    },
  ];
  @override
  Future<KubeJson> rolloutDiff(String sessionId, KubeResourceRef resource, int before, int after) async {
    compared.add((before, after));
    return {
      'changes': [
        {'path': '/spec/containers/0/image', 'before': 'old', 'after': 'new'},
      ],
      'truncated': false,
    };
  }
}

class _AiHistoryService extends _Service implements KubeAiHistoryService {
  @override
  Future<List<KubeJson>> kubeAiHistory(String contextId) async => [
    {
      'context_id': contextId,
      'synced': false,
      'conversation': {
        'id': 'saved',
        'title': 'Saved incident',
        'messages': [
          {'role': 'assistant', 'content': 'Private incident answer'},
        ],
      },
    },
  ];
  @override
  Future<void> deleteKubeAiConversation(String conversationId) async {}
}

class _ForwardTargetService extends _ForwardService implements KubeForwardTargetsService {
  @override
  Future<KubeForwardTargets> forwardTargets(String sessionId, KubeResourceRef resource) async =>
      const KubeForwardTargets(
        items: [
          KubeForwardTarget(
            pod: KubeResourceRef(api: _pods, name: 'api-pod', namespace: 'default'),
            uid: 'pod-uid',
            ready: true,
            ports: [KubeForwardPort(name: 'http', port: 9090)],
          ),
        ],
      );
}

class _NetworkForwardService extends _ForwardService implements KubeNetworkForwardService {
  final addresses = <String>[];
  @override
  Future<KubeForward> startNetworkForward(
    String sessionId, {
    required String namespace,
    required String pod,
    required int remotePort,
    required String localAddress,
    required bool approveNetwork,
    int localPort = 0,
  }) async {
    expect(approveNetwork, isTrue);
    addresses.add(localAddress);
    return _forward;
  }
}

class _MaskedService extends _MutationService {
  @override
  Future<KubeResourceDetail> getResource(String sessionId, KubeResourceRef resource) async => const KubeResourceDetail(
    object: {
      'metadata': {'uid': 'demo-pod', 'resourceVersion': '10'},
    },
    yaml: 'apiVersion: v1\nkind: Pod\nspec:\n  env: "[redacted]"\n',
    containers: [],
  );
}

class _TimelineService extends _InsightsService implements KubeOperationsService {
  final eventReads = <(String, String)>[];
  @override
  Future<List<KubeResourceRow>> objectEvents(String sessionId, KubeResourceRef resource, String uid) async {
    eventReads.add((resource.name, uid));
    return const [
      KubeResourceRow(
        name: 'reconcile-event',
        uid: 'event-uid',
        resourceVersion: '3',
        object: {
          'lastTimestamp': '2026-10-20T10:00:00Z',
          'reason': 'ReconciliationSucceeded',
          'type': 'Normal',
          'metadata': {
            'annotations': {'reconcile.fluxcd.io/revision': 'main@sha1:event-revision'},
          },
        },
      ),
    ];
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

class _FluxDetailService extends _InsightsService {
  @override
  Future<KubeResourceDetail> getResource(String sessionId, KubeResourceRef resource) async => KubeResourceDetail(
    object: {
      'apiVersion': resource.api.apiVersion,
      'kind': resource.api.kind,
      'metadata': {
        'name': resource.name,
        'namespace': resource.namespace,
        'uid': resource.name,
        'resourceVersion': '11',
      },
      'spec': {'interval': '5m'},
      'status': {
        'conditions': [
          {'type': 'Ready', 'status': 'True'},
        ],
      },
    },
    yaml: 'kind: Kustomization',
    containers: const [],
  );
}

class _WorkloadSwipeService extends _OperationsService {
  _WorkloadSwipeService() {
    session = const KubeSession(
      id: 'opaque-native-session',
      contextId: 'context',
      serverVersion: 'v1',
      production: false,
    );
  }
  final restarted = <(String, String)>[];
  @override
  Future<List<KubeResourceKind>> discover(String sessionId) async => const [
    KubeResourceKind(
      group: 'apps',
      version: 'v1',
      resource: 'deployments',
      kind: 'Deployment',
      namespaced: true,
      verbs: ['get', 'list', 'watch', 'patch'],
    ),
  ];
  @override
  Future<KubeResourceDetail> getResource(String sessionId, KubeResourceRef resource) async => KubeResourceDetail(
    object: {
      'apiVersion': resource.api.apiVersion,
      'kind': resource.api.kind,
      'metadata': {
        'name': resource.name,
        'namespace': resource.namespace,
        'uid': resource.name,
        'resourceVersion': 'latest-version',
      },
      'spec': {'replicas': 1},
    },
    yaml: 'kind: Deployment',
    containers: const [],
  );
  @override
  Future<void> restart(String sessionId, KubeResourceRef resource, String resourceVersion) async =>
      restarted.add((resource.name, resourceVersion));
}
