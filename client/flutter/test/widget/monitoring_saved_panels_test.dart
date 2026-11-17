import 'dart:async';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_providers.dart';
import 'package:consolecrypt/monitoring/monitoring_saved_panel_models.dart';
import 'package:consolecrypt/monitoring/monitoring_saved_panels.dart';
import 'package:consolecrypt/monitoring/monitoring_workspace.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_ui_proof.dart';
import '../helpers/monitoring_fakes.dart';
import '../helpers/monitoring_saved_panel_fakes.dart';
import '../helpers/test_app.dart';

Future<(ProviderContainer, MockBackend)> open(
  WidgetTester tester,
  SavedPanelTestService service,
  SavedPanelTestStore store, {
  Size size = const Size(1500, 1100),
  AppLocale locale = AppLocale.en,
  bool dark = false,
}) async {
  final backend = testBackend();
  addTearDown(backend.dispose);
  await backend.debugCreateUnlockedLocalProfile(name: 'Owned saved panel profile');
  if (dark) {
    final settings = backend.services.settings;
    await settings.updateLocal(settings.currentLocal.copyWith(themeMode: AppThemeMode.dark));
  }
  await pumpApp(
    tester,
    backend,
    size: size,
    locale: locale,
    captureBoundary: true,
    overrides: [monitoringBackendProvider.overrideWithValue(MonitoringTestBackend(service: service, store: store))],
  );
  final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
  container.read(routerProvider).go(AppRoutes.monitoring);
  await settle(tester);
  // Source editing and custom PromQL are now explicitly in Advanced.
  await tapKey(tester, 'monitoring-view-toggle');
  // Dashboard autoload is verified separately; these assertions begin after
  // switching to the explicit Advanced custom-panel editor/reader.
  service.reads.clear();
  return (container, backend);
}

Future<void> read(WidgetTester tester) => tapKey(tester, 'saved-panel-read-$savedPanelId');

Future<void> previewDraft(WidgetTester tester) async {
  final button = find.byKey(const ValueKey('saved-panel-preview'));
  await Scrollable.ensureVisible(tester.element(button), alignment: .5);
  await settle(tester);
  expect(button.hitTestable(), findsOneWidget);
  await tester.tap(button);
  await settle(tester);
}

class _GraphPreviewService extends SavedPanelTestService {
  @override
  Future<QueryOutcome> query(MonitoringSource source, MonitoringQuery query) async {
    queries++;
    last = query;
    final start = query.start!, end = query.end!;
    return QueryOutcome(
      status: 'success',
      httpStatus: 200,
      partial: false,
      warnings: const [],
      infos: const [],
      effectiveStepSeconds: (end - start) / 12,
      data: QueryData(
        'matrix',
        series: [
          MonitoringSeries(
            const {'phase': 'Running'},
            [
              for (var i = 0; i <= 12; i++)
                MonitoringSample(start + (end - start) * i / 12, SampleKind.finite, (12 + i % 5).toDouble()),
            ],
          ),
        ],
      ),
    );
  }
}

class _AdmissionStore extends SavedPanelTestStore {
  @override
  Future<SavedMonitoringPanel> savePanel(SavedPanelDefinition definition, {SavedMonitoringPanel? previous}) async {
    throw const AppException(
      AppErrorCode.validation,
      'Never render raw admission diagnostics',
      reason: AppErrorReason.invalidField,
      args: {'field': 'monitoring_panels', 'rule': 'panel_sync_admission_required'},
    );
  }
}

void main() {
  testWidgets('unsaved draft explicitly previews a range graph without save or changing exploration', (tester) async {
    final service = SavedPanelTestService(), store = _AdmissionStore()..panels = [];
    await open(tester, service, store);
    await tapKey(tester, 'saved-panel-new');
    await tester.enterText(
      find.byKey(const ValueKey('saved-panel-query')),
      'sum(kube_pod_status_phase{phase="Running"})',
    );
    expect(service.queries, 0);
    await previewDraft(tester);
    expect(service.queries, 1);
    expect(service.last!.expression, 'sum(kube_pod_status_phase{phase="Running"})');
    expect(service.last!.start, isNotNull);
    expect(service.last!.end, greaterThan(service.last!.start!));
    expect(service.reads, isEmpty);
    expect(store.saves, isEmpty);
    expect(store.panels, isEmpty);
    expect(find.byType(MonitoringResultView), findsOneWidget);
    expect(tester.widget<TextField>(find.byKey(const ValueKey('monitoring-expression'))).controller!.text, 'up');
    expect(find.byKey(const ValueKey('saved-panel-query')), findsOneWidget);
    expect(tester.takeException(), isNull);
  });
  for (final width in [580.0, 1500.0]) {
    testWidgets('Russian unsaved graph remains readable at width $width', (tester) async {
      await loadAutomationProofFonts(tester);
      final service = _GraphPreviewService(), store = _AdmissionStore()..panels = [];
      await open(tester, service, store, size: Size(width, 1100), locale: AppLocale.ru, dark: true);
      await tapKey(tester, 'saved-panel-new');
      await tester.enterText(find.byKey(const ValueKey('saved-panel-title')), 'Running pods');
      await tester.enterText(
        find.byKey(const ValueKey('saved-panel-query')),
        'sum(kube_pod_status_phase{phase="Running"})',
      );
      await previewDraft(tester);
      final chart = find.byType(MonitoringResultView);
      await Scrollable.ensureVisible(tester.element(chart), alignment: .5);
      await settle(tester);
      expect(service.queries, 1);
      expect(store.saves, isEmpty);
      expect(tester.widget<MonitoringResultView>(chart).result.data!.series.single.samples, hasLength(13));
      await captureAutomationWorkspace(tester, 'monitoring-draft-graph-${width < 800 ? 'narrow' : 'wide'}-ru');
      expect(tester.takeException(), isNull);
    }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  }
  for (final change in ['hide', 'source']) {
    testWidgets('$change discards an unfinished draft graph without a replacement request', (tester) async {
      final service = SavedPanelTestService()..pending = Completer<QueryOutcome>();
      final store = SavedPanelTestStore()..panels = [];
      if (change == 'source') {
        store.sources = [
          monitoringFixture,
          monitoringFixture.withRecord(id: '22222222-2222-4222-8222-222222222222', revision: 1),
        ];
      }
      final (container, _) = await open(tester, service, store);
      await tapKey(tester, 'saved-panel-new');
      await tester.enterText(find.byKey(const ValueKey('saved-panel-query')), 'up');
      await previewDraft(tester);
      expect(service.queries, 1);
      if (change == 'hide') {
        container.read(routerProvider).go(AppRoutes.terminal);
      } else {
        tester.widget<GlassSelect<String?>>(find.byType(GlassSelect<String?>).first).onChanged!(store.sources.last.id);
      }
      await settle(tester);
      service.pending!.complete(service.result);
      await settle(tester);
      expect(find.byType(MonitoringResultView), findsNothing);
      expect(service.queries, 1);
      expect(store.saves, isEmpty);
      expect(tester.takeException(), isNull);
    });
  }

  testWidgets('exact synced panel admission refusal keeps editor draft and does not show a successful save', (
    tester,
  ) async {
    final service = SavedPanelTestService(), store = _AdmissionStore()..panels = [];
    await open(tester, service, store);
    await tapKey(tester, 'saved-panel-new');
    await tester.enterText(find.byKey(const ValueKey('saved-panel-title')), 'Retained draft');
    await tester.enterText(find.byKey(const ValueKey('saved-panel-query')), 'up');
    await Scrollable.ensureVisible(tester.element(find.byKey(const ValueKey('saved-panel-save'))), alignment: 0.5);
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('saved-panel-save')));
    await settle(tester);
    final l = tester.element(find.byType(MonitoringSavedPanels)).l10n;
    expect(
      tester.widget<SelectableText>(find.byKey(const ValueKey('monitoring-error'))).data,
      l.monitoringSavedPanelSyncAdmission,
    );
    expect(find.byKey(const ValueKey('saved-panel-save')), findsOneWidget);
    expect(store.panels, isEmpty);
    expect(store.saves, isEmpty);
    expect(service.reads, isEmpty);
    expect(find.textContaining('Never render raw'), findsNothing);
    for (final error in [
      const AppException(
        AppErrorCode.validation,
        'other',
        reason: AppErrorReason.invalidField,
        args: {'field': 'other', 'rule': 'panel_sync_admission_required'},
      ),
      const AppException(
        AppErrorCode.validation,
        'other',
        reason: AppErrorReason.invalidField,
        args: {'field': 'monitoring_panels', 'rule': 'other'},
      ),
      const AppException(
        AppErrorCode.internal,
        'other',
        args: {'field': 'monitoring_panels', 'rule': 'panel_sync_admission_required'},
      ),
    ]) {
      expect(savedPanelAdmissionMessage(l, error), isNull);
    }
    expect(tester.takeException(), isNull);
  });
  testWidgets('saved panel reads explicitly with user hint while chart keeps unknown proven units and partial NaN', (
    tester,
  ) async {
    final service = SavedPanelTestService(), store = SavedPanelTestStore();
    await open(tester, service, store);
    expect(service.reads, isEmpty);
    await read(tester);
    expect(service.reads, hasLength(1));
    expect(service.reads.single.$3, isNull);
    expect(service.queries, 0);
    final chart = tester.widget<MonitoringResultView>(find.byType(MonitoringResultView));
    expect(chart.unit, isNull);
    expect(chart.result.partial, isTrue);
    expect(chart.result.data!.series.single.samples[1].kind, SampleKind.nan);
    final l = tester.element(find.byType(MonitoringSavedPanels)).l10n;
    expect(find.textContaining(l.monitoringSavedPanelHint), findsWidgets);
    expect(tester.takeException(), isNull);
  });
  testWidgets('editor save binds current source revision and preserves exact PromQL in shared store', (tester) async {
    final service = SavedPanelTestService(), store = SavedPanelTestStore()..panels = [];
    await open(tester, service, store);
    await tapKey(tester, 'saved-panel-new');
    await tester.enterText(find.byKey(const ValueKey('saved-panel-title')), 'Reviewed custom panel');
    await tester.enterText(find.byKey(const ValueKey('saved-panel-query')), '  up{job="quoted"}\n');
    await Scrollable.ensureVisible(tester.element(find.byKey(const ValueKey('saved-panel-save'))), alignment: 0.5);
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('saved-panel-save')));
    await settle(tester);
    expect(store.saves.single.query, '  up{job="quoted"}\n');
    expect(store.saves.single.sourceId, monitoringFixture.id);
    expect(store.saves.single.sourceRevision, 1);
    expect(service.reads, isEmpty);
    expect(find.text('Reviewed custom panel'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });
  testWidgets('deletion requires explicit confirmation and refreshes same catalog list without query execution', (
    tester,
  ) async {
    final service = SavedPanelTestService(), store = SavedPanelTestStore();
    await open(tester, service, store);
    final l = tester.element(find.byType(MonitoringSavedPanels)).l10n;
    await tester.ensureVisible(find.text(l.monitoringSavedPanelDelete));
    await tester.tap(find.text(l.monitoringSavedPanelDelete));
    await settle(tester);
    expect(store.deletions, 0);
    await tester.ensureVisible(find.text(l.monitoringConfirmDelete));
    await tester.tap(find.text(l.monitoringConfirmDelete));
    await settle(tester);
    expect(store.deletions, 1);
    expect(store.panels, isEmpty);
    expect(find.text(l.monitoringSavedPanelEmpty), findsOneWidget);
    expect(service.reads, isEmpty);
    expect(tester.takeException(), isNull);
  });
  testWidgets('stale source definition cannot read until explicitly rereviewed saved', (tester) async {
    final service = SavedPanelTestService(),
        store = SavedPanelTestStore()
          ..panels = [
            const SavedMonitoringPanel(
              id: savedPanelId,
              revision: 1,
              definition: savedDefinition,
              state: SavedPanelState.staleSource,
            ),
          ];
    await open(tester, service, store);
    expect(tester.widget<TextButton>(find.byKey(const ValueKey('saved-panel-read-$savedPanelId'))).onPressed, isNull);
    final l = tester.element(find.byType(MonitoringSavedPanels)).l10n;
    await tester.ensureVisible(find.text(l.monitoringSavedPanelEdit));
    await tester.tap(find.text(l.monitoringSavedPanelEdit));
    await settle(tester);
    await Scrollable.ensureVisible(tester.element(find.byKey(const ValueKey('saved-panel-save'))), alignment: 0.5);
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('saved-panel-save')));
    await settle(tester);
    expect(store.saves, hasLength(1));
    expect(store.panels.single.revision, 2);
    expect(service.reads, isEmpty);
    await read(tester);
    expect(service.reads.single.$2.revision, 2);
    expect(tester.takeException(), isNull);
  });
  testWidgets('orphan definition can be explicitly deleted when no source exists and source editor is open', (
    tester,
  ) async {
    final service = SavedPanelTestService(),
        store = SavedPanelTestStore()
          ..sources = []
          ..panels = [
            const SavedMonitoringPanel(
              id: savedPanelId,
              revision: 1,
              definition: savedDefinition,
              state: SavedPanelState.missingSource,
            ),
          ];
    await open(tester, service, store);
    final l = tester.element(find.byType(MonitoringSavedPanels)).l10n;
    await tester.ensureVisible(find.text(l.monitoringSavedPanelDelete));
    await tester.tap(find.text(l.monitoringSavedPanelDelete));
    await settle(tester);
    expect(store.deletions, 0);
    await tester.ensureVisible(find.text(l.monitoringConfirmDelete));
    await tester.tap(find.text(l.monitoringConfirmDelete));
    await settle(tester);
    expect(store.deletions, 1);
    expect(store.panels, isEmpty);
    expect(service.reads, isEmpty);
    expect(tester.takeException(), isNull);
  });
  for (final change in ['cancel', 'hide', 'lock', 'profile', 'source', 'revision']) {
    testWidgets('$change while saved read pending discards late result and never rereads automatically', (
      tester,
    ) async {
      final service = SavedPanelTestService()..pendingSaved = Completer<SavedPanelRead>();
      final store = SavedPanelTestStore();
      if (change == 'source') {
        store.sources = [
          monitoringFixture,
          monitoringFixture.withRecord(id: '22222222-2222-4222-8222-222222222222', revision: 1),
        ];
      }
      final (container, backend) = await open(tester, service, store);
      await read(tester);
      switch (change) {
        case 'cancel':
          await tapKey(tester, 'monitoring-cancel');
        case 'hide':
          container.read(routerProvider).go(AppRoutes.terminal);
          await settle(tester);
        case 'lock':
          await container.read(vaultServiceProvider).lock();
          await settle(tester);
        case 'profile':
          await backend.debugCreateUnlockedLocalProfile(name: 'Other owned panel profile');
          // Native catalogs belong to profiles. The replacement fixture must
          // not expose the old profile's custom panels to the new dashboard.
          store.panels = [];
          store.sources = [];
          await settle(tester);
        case 'source':
          final selector = tester.widget<GlassSelect<String?>>(find.byType(GlassSelect<String?>).first);
          selector.onChanged!(store.sources.last.id);
          await settle(tester);
        case 'revision':
          final l = tester.element(find.byType(MonitoringWorkspace)).l10n;
          await tester.ensureVisible(find.text(l.monitoringEditSource));
          await tester.tap(find.text(l.monitoringEditSource));
          await settle(tester);
          await tapKey(tester, 'monitoring-save-source');
      }
      service.pendingSaved!.complete(SavedPanelRead(service.result, SavedPanelUnitHint.seconds));
      await settle(tester);
      expect(find.byType(MonitoringResultView), findsNothing);
      expect(service.reads, hasLength(1));
      expect(service.cancellations, greaterThan(0));
      expect(tester.takeException(), isNull);
    });
  }
  for (final width in [580.0, 1500.0]) {
    testWidgets('Russian saved panels keep explicit user hints and native output readable at width $width', (
      tester,
    ) async {
      await loadAutomationProofFonts(tester);
      final service = SavedPanelTestService(), store = SavedPanelTestStore();
      await open(tester, service, store, size: Size(width, 1100), locale: AppLocale.ru, dark: true);
      expect(service.reads, isEmpty);
      await read(tester);
      final l = tester.element(find.byType(MonitoringSavedPanels)).l10n;
      expect(find.text(l.monitoringChartUnit(l.monitoringChartUnknownUnit)), findsOneWidget);
      expect(find.text('NaN'), findsOneWidget);
      await tester.ensureVisible(find.byType(MonitoringSavedPanels));
      await settle(tester);
      await captureAutomationWorkspace(tester, 'monitoring-saved-panels-${width < 800 ? 'narrow' : 'wide'}-ru');
      expect(service.reads.length, 1);
      expect(service.queries, 0);
      expect(tester.takeException(), isNull);
    }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  }
}
