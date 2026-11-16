import 'package:consolecrypt/app/gate.dart' show AppRoutes;
import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/l10n/app_localizations_en.dart';
import 'package:consolecrypt/monitoring/monitoring_binding_models.dart';
import 'package:consolecrypt/monitoring/monitoring_binding_reader.dart';
import 'package:consolecrypt/monitoring/monitoring_panel_collection_guidance.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_ui_proof.dart';
import '../helpers/monitoring_binding_fakes.dart';
import '../helpers/monitoring_binding_reads_fakes.dart';
import '../helpers/monitoring_fakes.dart';

BindingReadResult missing({
  MonitoringPanel? panel = MonitoringPanel.docker,
  BindingReadState state = BindingReadState.unavailable,
  BindingPreparationReason? reason = BindingPreparationReason.missingMetrics,
}) => BindingReadResult(
  bindingId: bindingFixture().id,
  bindingRevision: 7,
  kind: 'bindingPanel',
  state: state,
  panel: panel,
  preparationReason: reason,
  missingMetrics: const ['container_cpu_usage_seconds_total'],
);

Future<({GoRouter router, ValueNotifier<BindingReadResult> result, ValueNotifier<bool> active})> open(
  WidgetTester tester, {
  BindingReadResult? read,
  MonitoringBindingReadTestService? reader,
  Locale locale = const Locale('en'),
  Size size = const Size(1200, 1000),
  double scale = 1,
}) async {
  await tester.binding.setSurfaceSize(size);
  addTearDown(() => tester.binding.setSurfaceSize(null));
  await loadAutomationProofFonts(tester);
  final result = ValueNotifier(read ?? missing());
  final active = ValueNotifier(true);
  final router = GoRouter(
    initialLocation: '/monitoring-test',
    routes: [
      GoRoute(
        path: '/monitoring-test',
        builder: (context, _) => Scaffold(
          body: SingleChildScrollView(
            child: ValueListenableBuilder(
              valueListenable: result,
              builder: (context, value, _) => ValueListenableBuilder(
                valueListenable: active,
                builder: (context, enabled, _) => reader == null
                    ? MonitoringBindingResultView(result: value, active: enabled)
                    : MonitoringBindingReaderView(
                        reader: reader,
                        source: monitoringFixture,
                        binding: bindingFixture(),
                        active: enabled,
                      ),
              ),
            ),
          ),
        ),
      ),
      GoRoute(
        path: AppRoutes.kubernetes,
        builder: (context, _) => const Scaffold(body: Text('Existing cluster view')),
      ),
    ],
  );
  addTearDown(router.dispose);
  addTearDown(result.dispose);
  addTearDown(active.dispose);
  await tester.pumpWidget(
    MaterialApp.router(
      routerConfig: router,
      locale: locale,
      supportedLocales: AppLocalizations.supportedLocales,
      theme: AppTheme.dark(platform: TargetPlatform.windows),
      localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
      builder: (context, child) => MediaQuery(
        data: MediaQuery.of(context).copyWith(textScaler: TextScaler.linear(scale)),
        child: child!,
      ),
    ),
  );
  await tester.pumpAndSettle();
  return (router: router, result: result, active: active);
}

void main() {
  testWidgets('all ten typed panels explain the existing collector; only containers offer Kubernetes', (tester) async {
    final l = AppLocalizationsEn();
    final h = await open(tester);
    for (final panel in MonitoringPanel.values) {
      h.result.value = missing(panel: panel);
      await tester.pumpAndSettle();
      final banner = tester.widget<InfoBanner>(find.byKey(const ValueKey('monitoring-panel-collection-guidance')));
      expect(banner.tone, BannerTone.warning);
      expect(banner.message, contains(l.monitoringCollectorMissingIntro));
      final hint = switch (panel) {
        MonitoringPanel.hostOverview ||
        MonitoringPanel.cpu ||
        MonitoringPanel.memory ||
        MonitoringPanel.filesystem ||
        MonitoringPanel.diskIo ||
        MonitoringPanel.network => l.monitoringCollectorNode,
        MonitoringPanel.docker => l.monitoringCollectorContainers,
        MonitoringPanel.postgreSql => l.monitoringCollectorPostgres,
        MonitoringPanel.redis => l.monitoringCollectorRedis,
        MonitoringPanel.nginx => l.monitoringCollectorNginx,
      };
      expect(banner.message, contains(hint));
      expect(
        find.byKey(const ValueKey('monitoring-open-existing-kubernetes-metrics')),
        panel == MonitoringPanel.docker ? findsOneWidget : findsNothing,
      );
      expect(h.router.routeInformationProvider.value.uri.path, '/monitoring-test');
    }
  });

  testWidgets('probe failure, selector conflict, noData and unknown panel never imply a missing exporter', (
    tester,
  ) async {
    final h = await open(tester);
    final values = [
      for (final reason in [
        BindingPreparationReason.probeFailed,
        BindingPreparationReason.selectorConflict,
        BindingPreparationReason.queryBudget,
      ])
        missing(reason: reason),
      for (final state in [BindingReadState.ready, BindingReadState.partial, BindingReadState.noData])
        missing(state: state),
      missing(panel: null),
    ];
    for (final value in values) {
      h.result.value = value;
      await tester.pumpAndSettle();
      expect(find.byType(MonitoringPanelCollectionGuidance), findsNothing);
      expect(h.router.routeInformationProvider.value.uri.path, '/monitoring-test');
    }
  });

  testWidgets('existing Kubernetes metrics section opens only after explicit click', (tester) async {
    final h = await open(tester);
    expect(find.text('Existing cluster view'), findsNothing);
    await tester.tap(find.byKey(const ValueKey('monitoring-open-existing-kubernetes-metrics')));
    await tester.pumpAndSettle();
    expect(h.router.routeInformationProvider.value.uri.path, AppRoutes.kubernetes);
    expect(find.text('Existing cluster view'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('inactive result disables Open and an already captured callback cannot navigate', (tester) async {
    final h = await open(tester);
    final key = find.byKey(const ValueKey('monitoring-open-existing-kubernetes-metrics'));
    final oldOpen = tester.widget<TextButton>(key).onPressed!;
    h.active.value = false;
    await tester.pumpAndSettle();
    expect(tester.widget<TextButton>(key).onPressed, isNull);
    oldOpen();
    await tester.pumpAndSettle();
    expect(h.router.routeInformationProvider.value.uri.path, '/monitoring-test');
    h.active.value = true;
    await tester.pumpAndSettle();
    await tester.tap(key);
    await tester.pumpAndSettle();
    expect(h.router.routeInformationProvider.value.uri.path, AppRoutes.kubernetes);
  });

  testWidgets('replacement result retires its captured container callback', (tester) async {
    final h = await open(tester);
    final oldOpen = tester
        .widget<TextButton>(find.byKey(const ValueKey('monitoring-open-existing-kubernetes-metrics')))
        .onPressed!;
    h.result.value = missing(panel: MonitoringPanel.memory);
    await tester.pumpAndSettle();
    oldOpen();
    await tester.pumpAndSettle();
    expect(h.router.routeInformationProvider.value.uri.path, '/monitoring-test');
    expect(find.byKey(const ValueKey('monitoring-open-existing-kubernetes-metrics')), findsNothing);
  });

  testWidgets('no router leaves a passive readable explanation with disabled Open', (tester) async {
    await tester.pumpWidget(
      MaterialApp(
        theme: AppTheme.dark(platform: TargetPlatform.windows),
        supportedLocales: AppLocalizations.supportedLocales,
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        home: Scaffold(body: MonitoringBindingResultView(result: missing())),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.byType(MonitoringPanelCollectionGuidance), findsOneWidget);
    expect(
      tester.widget<TextButton>(find.byKey(const ValueKey('monitoring-open-existing-kubernetes-metrics'))).onPressed,
      isNull,
    );
  });

  testWidgets('manual typed read consumes one existing reader action; opening Kubernetes adds no PromQL', (
    tester,
  ) async {
    final reader = MonitoringBindingReadTestService()
      ..panelResult = (panel) {
        final value = bindingPanelJson(panel: panel);
        value['state'] = 'unavailable';
        value.remove('queries');
        value['preparation'] = {
          'state': 'unavailable',
          'panel': panel.name,
          'reason': 'missingMetrics',
          'missingMetrics': ['container_cpu_usage_seconds_total'],
          'probe': identityOutcome(),
          'error': null,
        };
        return value;
      };
    final h = await open(tester, reader: reader);
    expect(reader.panels, isEmpty);
    expect(reader.queries, 0);
    expect(reader.discoveries, isEmpty);
    tester.widget<GlassSelect<MonitoringPanel>>(find.byKey(const ValueKey('monitoring-binding-panel'))).onChanged!(
      MonitoringPanel.docker,
    );
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('monitoring-binding-load-panel')));
    await tester.pumpAndSettle();
    expect(reader.panels, hasLength(1));
    expect(reader.queries, 0);
    await tester.ensureVisible(find.byKey(const ValueKey('monitoring-open-existing-kubernetes-metrics')));
    await tester.tap(find.byKey(const ValueKey('monitoring-open-existing-kubernetes-metrics')));
    await tester.pumpAndSettle();
    expect(h.router.routeInformationProvider.value.uri.path, AppRoutes.kubernetes);
    expect(reader.panels, hasLength(1));
    expect(reader.queries, 0);
    expect(reader.discoveries, isEmpty);
    expect(tester.takeException(), isNull);
  });

  testWidgets('narrow enlarged Russian guidance keeps collector distinction and explicit Open reachable', (
    tester,
  ) async {
    await open(tester, locale: const Locale('ru'), size: const Size(360, 1000), scale: 1.4);
    expect(find.textContaining('существующего API метрик'), findsOneWidget);
    await tester.ensureVisible(find.byKey(const ValueKey('monitoring-open-existing-kubernetes-metrics')));
    expect(
      tester.widget<TextButton>(find.byKey(const ValueKey('monitoring-open-existing-kubernetes-metrics'))).onPressed,
      isNotNull,
    );
    expect(tester.takeException(), isNull);
  });
}
