import 'dart:async';

import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_helm_comparison.dart';
import 'package:consolecrypt/kubernetes/kube_helm_comparison_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

const _session = KubeSession(id: 'session', contextId: 'context', serverVersion: 'v1.32.0', production: true);
const _kind = KubeResourceKind(
  group: 'apps',
  version: 'v1',
  resource: 'deployments',
  kind: 'Deployment',
  namespaced: true,
  verbs: ['get'],
);
const _comparison = KubeHelmComparison(
  revision: 4,
  partial: true,
  masked: true,
  items: [
    KubeHelmObjectComparison(
      reference: KubeResourceRef(api: _kind, namespace: 'apps', name: 'web'),
      state: 'changed',
      diff: {
        'changes': [
          {'path': '/spec/replicas', 'before': 1, 'after': 2},
        ],
      },
    ),
  ],
);
final _scopeProvider = NotifierProvider<_Scope, KubeScope>(_Scope.new);

class _Scope extends Notifier<KubeScope> {
  @override
  KubeScope build() => const KubeScope('profile', true);
  void lock() => state = const KubeScope('profile', false);
}

class _Service implements KubeHelmComparisonService {
  int reads = 0;
  Completer<KubeHelmComparison>? pending;
  @override
  Future<KubeHelmComparison> compareHelmLive(String sessionId, String namespace, String secretName) async {
    reads++;
    expect(sessionId, 'session');
    expect(namespace, 'apps');
    expect(secretName, 'sh.helm.release.v1.web.v4');
    return pending?.future ?? _comparison;
  }
}

Future<ProviderContainer> _pump(WidgetTester tester, _Service service) async {
  tester.view.physicalSize = const Size(412, 1100);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    ProviderScope(
      overrides: [kubeScopeProvider.overrideWith((ref) => ref.watch(_scopeProvider))],
      child: MaterialApp(
        locale: const Locale('ru'),
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        supportedLocales: AppLocalizations.supportedLocales,
        builder: (context, child) => MediaQuery(
          data: MediaQuery.of(context).copyWith(textScaler: const TextScaler.linear(1.5)),
          child: child!,
        ),
        home: Scaffold(
          body: Padding(
            padding: const EdgeInsets.all(12),
            child: KubeHelmComparisonPanel(
              session: _session,
              namespace: 'apps',
              secretName: 'sh.helm.release.v1.web.v4',
              service: service,
            ),
          ),
        ),
      ),
    ),
  );
  await settle(tester);
  return ProviderScope.containerOf(tester.element(find.byType(KubeHelmComparisonPanel)));
}

void main() {
  testWidgets('comparison waits for user action and displays partial masked read-only results in production', (
    tester,
  ) async {
    final service = _Service();
    await _pump(tester, service);
    expect(service.reads, 0);
    await tester.tap(find.byKey(const ValueKey('kube-helm-live-load')));
    await settle(tester);
    expect(service.reads, 1);
    expect(find.textContaining('Скрытые поля'), findsOneWidget);
    expect(find.textContaining('Есть отличия'), findsOneWidget);
    await tester.tap(find.textContaining('Deployment'));
    await settle(tester);
    expect(find.textContaining('/spec/replicas'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });
  testWidgets('late comparison cannot restore a locked profile', (tester) async {
    final service = _Service()..pending = Completer<KubeHelmComparison>();
    final scope = await _pump(tester, service);
    await tester.tap(find.byKey(const ValueKey('kube-helm-live-load')));
    await tester.pump();
    scope.read(_scopeProvider.notifier).lock();
    service.pending!.complete(_comparison);
    await settle(tester);
    expect(find.textContaining('Deployment'), findsNothing);
    expect(find.byKey(const ValueKey('kube-helm-live-load')), findsNothing);
    expect(tester.takeException(), isNull);
  });
}
