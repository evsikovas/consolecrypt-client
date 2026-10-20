import 'dart:async';
import 'dart:typed_data';

import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_logs_panel.dart';
import 'package:consolecrypt/kubernetes/kube_logs_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

final _scope = NotifierProvider<_Scope, KubeScope>(_Scope.new);

class _Scope extends Notifier<KubeScope> {
  @override
  KubeScope build() => const KubeScope('fixture', true);
  void lock() => state = const KubeScope('fixture', false);
}

const _deployment = KubeResourceRef(
  api: KubeResourceKind(group: 'apps', version: 'v1', resource: 'deployments', kind: 'Deployment', namespaced: true),
  namespace: 'default',
  name: 'api',
);

class _Service implements KubeBrowserService, KubeAdvancedLogService {
  int started = 0, stopped = 0;
  final streams = <StreamController<String>>[];
  Completer<String>? snapshot;
  final since = <int?>[];
  @override
  Future<KubeLogTargets> logTargets(String sessionId, KubeResourceRef resource) async => const KubeLogTargets(
    targets: [
      KubeLogTarget(pod: 'api-a', containers: ['main']),
      KubeLogTarget(pod: 'api-b', containers: ['main']),
    ],
  );
  @override
  Future<String> logsSince(
    String sessionId,
    KubeResourceRef resource, {
    String? container,
    bool previous = false,
    bool timestamps = true,
    int tailLines = 1000,
    int? sinceSeconds,
  }) async {
    since.add(sinceSeconds);
    return snapshot?.future ?? 'INFO ${resource.name} ready\nERROR ${resource.name} failed';
  }

  @override
  Stream<String> followLogsSince(
    String sessionId,
    KubeResourceRef resource, {
    String? container,
    bool previous = false,
    bool timestamps = true,
    int tailLines = 1000,
    int? sinceSeconds,
  }) {
    final stream = StreamController<String>(onListen: () => started++, onCancel: () => stopped++);
    streams.add(stream);
    return stream.stream;
  }

  @override
  Future<void> exportLogs(String sessionId, String path, Uint8List contents, {required bool mask}) async {}
  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
  Future<void> dispose() async {
    for (final stream in streams) {
      await stream.close();
    }
  }
}

Future<ProviderContainer> _pump(WidgetTester tester, _Service service, String language) async {
  tester.view.physicalSize = const Size(412, 800);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  addTearDown(service.dispose);
  await tester.pumpWidget(
    ProviderScope(
      overrides: [kubeScopeProvider.overrideWith((ref) => ref.watch(_scope))],
      child: MaterialApp(
        locale: Locale(language),
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        supportedLocales: AppLocalizations.supportedLocales,
        builder: (context, child) => MediaQuery(
          data: MediaQuery.of(context).copyWith(textScaler: const TextScaler.linear(1.5)),
          child: child!,
        ),
        home: Scaffold(
          body: SizedBox(
            height: 580,
            child: KubeLogsPanel(
              session: const KubeSession(id: 'session', contextId: 'context', serverVersion: 'v1', production: false),
              resource: _deployment,
              service: service,
            ),
          ),
        ),
      ),
    ),
  );
  await tester.pump();
  await tester.pump(const Duration(milliseconds: 100));
  await tester.pump();
  return ProviderScope.containerOf(tester.element(find.byType(Scaffold)));
}

Future<void> _tapText(WidgetTester tester, String text) async {
  final finder = find.text(text);
  await tester.ensureVisible(finder);
  await tester.tap(finder);
  await tester.pump();
  await tester.pump(const Duration(milliseconds: 100));
}

void main() {
  for (final language in ['en', 'ru']) {
    testWidgets('aggregate log snapshots fit mobile at 150% ($language)', (tester) async {
      final service = _Service();
      await _pump(tester, service, language);
      expect(find.text('[api-a/main] INFO api-a ready'), findsOneWidget);
      expect(find.text('[api-b/main] ERROR api-b failed'), findsOneWidget);
      expect(tester.takeException(), isNull);
    });
  }
  testWidgets('follow pause cancels every upstream stream and lock clears visible logs', (tester) async {
    final service = _Service();
    final container = await _pump(tester, service, 'en');
    await _tapText(tester, 'Follow new log lines');
    expect(service.started, 2);
    service.streams.first.add('ERROR private fixture line');
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 100));
    await tester.pump();
    expect(find.textContaining('private fixture line'), findsOneWidget);
    await _tapText(tester, 'Pause');
    expect(service.stopped, 2);
    await _tapText(tester, 'Resume');
    expect(service.started, 4);
    container.read(_scope.notifier).lock();
    await tester.pumpAndSettle();
    expect(service.stopped, 4);
    expect(find.textContaining('private fixture line'), findsNothing);
  });
  testWidgets('late snapshots after profile lock never publish data', (tester) async {
    final service = _Service()..snapshot = Completer<String>();
    final container = await _pump(tester, service, 'en');
    container.read(_scope.notifier).lock();
    await tester.pump();
    service.snapshot!.complete('private late fixture');
    await tester.pumpAndSettle();
    expect(find.textContaining('private late fixture'), findsNothing);
  });
}
