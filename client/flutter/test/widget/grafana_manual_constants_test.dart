import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/monitoring/grafana_constant_review.dart';
import 'package:consolecrypt/monitoring/monitoring_grafana_constants.dart';
import 'package:consolecrypt/monitoring/monitoring_grafana_import.dart';
import 'package:consolecrypt/monitoring/monitoring_grafana_worker.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_providers.dart';
import 'package:consolecrypt/monitoring/monitoring_saved_panel_models.dart';
import 'package:consolecrypt/monitoring/monitoring_screen.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_ui_proof.dart';
import '../helpers/monitoring_fakes.dart';
import '../helpers/monitoring_saved_panel_fakes.dart';
import '../helpers/test_app.dart';

String dashboard() => jsonEncode({
  'schemaVersion': 42,
  'title': 'Owned manual review dashboard',
  'templating': {
    'list': [
      {
        'name': 'site',
        'type': 'constant',
        'current': {'value': 'not-a-default'},
        'query': 'not-a-default',
      },
    ],
  },
  'panels': [
    for (final name in ['First manual query', 'Second manual query'])
      {
        'title': name,
        'type': 'timeseries',
        'fieldConfig': {
          'defaults': {'unit': 'bytes'},
        },
        'datasource': {'type': 'prometheus', 'uid': 'not-a-native-source'},
        'targets': [
          {'expr': r'up{site="$site"}'},
        ],
      },
    {
      'title': 'Unsupported table remains',
      'type': 'table',
      'targets': [
        {'expr': 'up'},
      ],
    },
  ],
});

const otherSource = MonitoringSource(
  id: '22222222-2222-4222-8222-222222222222',
  revision: 3,
  name: 'Other explicitly chosen source',
  endpoint: 'https://other.invalid',
);

class _Inspector implements GrafanaDashboardInspector, GrafanaManualConstantInspector {
  final delegate = IsolatedGrafanaDashboardInspector();
  Future<GrafanaInspection>? inspection;
  Future<GrafanaConstantReview>? review;
  Completer<GrafanaConstantReview>? pendingReview;
  @override
  Future<GrafanaInspection> inspect(String input) => inspection = delegate.inspect(input);
  @override
  Future<GrafanaConstantReview> reviewConstants(String input, String path, Map<String, String> values) =>
      review = pendingReview?.future ?? delegate.reviewConstants(input, path, values);
  @override
  void cancel() => delegate.cancel();
  @override
  void dispose() => delegate.dispose();
}

Future<ProviderContainer> _open(
  WidgetTester tester,
  _Inspector inspector,
  SavedPanelTestService service,
  SavedPanelTestStore store, {
  double width = 1500,
  bool russian = false,
}) async {
  await loadAutomationProofFonts(tester);
  final backend = testBackend();
  addTearDown(backend.dispose);
  await backend.debugCreateUnlockedLocalProfile(name: 'Owned explicit constants profile');
  if (russian) {
    final settings = backend.services.settings;
    await settings.updateLocal(settings.currentLocal.copyWith(themeMode: AppThemeMode.dark));
  }
  await pumpApp(
    tester,
    backend,
    size: Size(width, 1100),
    locale: russian ? AppLocale.ru : AppLocale.en,
    captureBoundary: true,
    overrides: [
      monitoringBackendProvider.overrideWithValue(MonitoringTestBackend(store: store, service: service)),
      grafanaDashboardInspectorFactoryProvider.overrideWithValue(() => inspector),
    ],
  );
  final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
  container.read(routerProvider).go(AppRoutes.monitoring);
  await settle(tester);
  await tapKey(tester, 'saved-panel-import-grafana');
  await enterKey(tester, 'grafana-import-json', dashboard());
  final inspect = find.byKey(const ValueKey('grafana-import-inspect'));
  await tester.ensureVisible(inspect);
  await settle(tester);
  await tester.runAsync(() async {
    await tester.tap(inspect);
    await inspector.inspection!.timeout(const Duration(seconds: 5));
  });
  await settle(tester);
  await tapKey(tester, 'grafana-import-choose-panels/0');
  return container;
}

Future<void> chooseSource(WidgetTester tester, {String name = 'Fixture metrics'}) async {
  final field = find.descendant(
    of: find.byType(MonitoringGrafanaImport),
    matching: find.byType(DropdownButtonFormField<String>),
  );
  await tester.ensureVisible(field);
  await tester.tap(field);
  await settle(tester);
  await tester.tap(find.textContaining(name).last);
  await settle(tester);
}

Future<void> _review(WidgetTester tester, _Inspector inspector) async {
  final action = find.byKey(const ValueKey('grafana-constants-review'));
  await tester.ensureVisible(action);
  await settle(tester);
  await tester.runAsync(() async {
    await tester.tap(action);
    await inspector.review!.timeout(const Duration(seconds: 5));
  });
  await settle(tester);
}

void main() {
  for (final keyboard in [0.0, 220.0]) {
    testWidgets('manual constant review keeps actions reachable at 420x760 scale 1.5 keyboard $keyboard', (
      tester,
    ) async {
      final inspector = _Inspector(), service = SavedPanelTestService(), store = SavedPanelTestStore()..panels = [];
      await _open(tester, inspector, service, store, width: 420, russian: true);
      await tapKey(tester, 'grafana-constant-use-site');
      await enterKey(tester, 'grafana-constant-value-site', 'owned-small-screen');
      await chooseSource(tester);
      await _review(tester, inspector);
      tester.view.physicalSize = const Size(420, 760);
      tester.view.viewInsets = FakeViewPadding(bottom: keyboard);
      tester.platformDispatcher.textScaleFactorTestValue = 1.5;
      addTearDown(tester.platformDispatcher.clearTextScaleFactorTestValue);
      await settle(tester);
      expect(tester.takeException(), isNull);
      expect(find.byKey(const ValueKey('grafana-import-continue')).hitTestable(), findsOneWidget);
      expect(find.byKey(const ValueKey('grafana-import-cancel')).hitTestable(), findsOneWidget);
      final query = find.byKey(const ValueKey('grafana-constants-query'));
      await Scrollable.ensureVisible(tester.element(query), alignment: .5);
      await settle(tester);
      expect(query.hitTestable(), findsOneWidget);
      expect(store.saves, isEmpty);
      expect(service.queries, 0);
      expect(service.reads, isEmpty);
      await tapKey(tester, 'grafana-import-cancel');
      expect(find.byType(MonitoringGrafanaImport), findsNothing);
      expect(tester.takeException(), isNull);
    });
  }

  testWidgets('fields start unacknowledged and empty; original unsupported rows and JSON stay available', (
    tester,
  ) async {
    final inspector = _Inspector(), service = SavedPanelTestService(), store = SavedPanelTestStore()..panels = [];
    await _open(tester, inspector, service, store);
    expect(tester.widget<Checkbox>(find.byKey(const ValueKey('grafana-constant-use-site'))).value, isFalse);
    expect(find.byKey(const ValueKey('grafana-constant-value-site')), findsNothing);
    final l = tester.element(find.byType(MonitoringGrafanaImport)).l10n;
    expect(find.text(l.grafanaImportCounts(0, 3)), findsOneWidget);
    expect(tester.widget<TextField>(find.byKey(const ValueKey('grafana-import-json'))).controller!.text, dashboard());
    expect(tester.widget<GlassButton>(find.byKey(const ValueKey('grafana-constants-review'))).onPressed, isNull);
    expect(tester.widget<GlassButton>(find.byKey(const ValueKey('grafana-import-continue'))).onPressed, isNull);
    expect(store.saves, isEmpty);
    expect(service.queries, 0);
    expect(service.reads, isEmpty);
  });

  testWidgets('explicit empty differs from untouched and prefills only after review with separate manual Save', (
    tester,
  ) async {
    final inspector = _Inspector(), service = SavedPanelTestService(), store = SavedPanelTestStore()..panels = [];
    await _open(tester, inspector, service, store);
    await tapKey(tester, 'grafana-constant-use-site');
    expect(tester.widget<TextField>(find.byKey(const ValueKey('grafana-constant-value-site'))).controller!.text, '');
    final l = tester.element(find.byType(MonitoringGrafanaImport)).l10n;
    expect(find.text(l.grafanaConstantExplicitEmpty), findsOneWidget);
    await chooseSource(tester);
    await _review(tester, inspector);
    expect(find.byKey(const ValueKey('grafana-constants-reviewed')), findsOneWidget);
    expect(find.text(l.grafanaImportCounts(0, 3)), findsOneWidget);
    expect(store.saves, isEmpty);
    expect(service.queries, 0);
    await tapKey(tester, 'grafana-import-continue');
    expect(tester.widget<TextField>(find.byKey(const ValueKey('saved-panel-query'))).controller!.text, 'up{site=""}');
    // The imported mode is a hint; it does not change the current-value
    // workspace choice or trigger a read.
    expect(tester.widget<CheckboxListTile>(find.byKey(const ValueKey('monitoring-range'))).value, isFalse);
    expect(store.saves, isEmpty);
    expect(service.queries, 0);
    await tapKey(tester, 'saved-panel-save');
    expect(store.saves.single.query, 'up{site=""}');
    expect(store.saves.single.sourceId, monitoringFixture.id);
    expect(store.saves.single.sourceRevision, monitoringFixture.revision);
    expect(store.saves.single.unitHint, SavedPanelUnitHint.unknown);
    expect(service.queries, 0);
    expect(service.reads, isEmpty);
  });

  testWidgets('any manual value edit revokes review and requires another explicit Review', (tester) async {
    final inspector = _Inspector(), service = SavedPanelTestService(), store = SavedPanelTestStore()..panels = [];
    await _open(tester, inspector, service, store);
    await tapKey(tester, 'grafana-constant-use-site');
    await enterKey(tester, 'grafana-constant-value-site', 'owned-first');
    await chooseSource(tester);
    await _review(tester, inspector);
    await enterKey(tester, 'grafana-constant-value-site', 'owned-later');
    expect(find.byKey(const ValueKey('grafana-constants-reviewed')), findsNothing);
    expect(tester.widget<GlassButton>(find.byKey(const ValueKey('grafana-import-continue'))).onPressed, isNull);
    await _review(tester, inspector);
    expect(find.text('up{site="owned-later"}'), findsOneWidget);
    expect(store.saves, isEmpty);
    expect(service.queries, 0);
  });

  testWidgets('changing panel clears field acknowledgement and cannot reuse a prior reviewed query', (tester) async {
    final inspector = _Inspector(), service = SavedPanelTestService(), store = SavedPanelTestStore()..panels = [];
    await _open(tester, inspector, service, store);
    await tapKey(tester, 'grafana-constant-use-site');
    await chooseSource(tester);
    await _review(tester, inspector);
    await tapKey(tester, 'grafana-import-choose-panels/1');
    expect(tester.widget<Checkbox>(find.byKey(const ValueKey('grafana-constant-use-site'))).value, isFalse);
    expect(find.byKey(const ValueKey('grafana-constant-value-site')), findsNothing);
    expect(find.byKey(const ValueKey('grafana-constants-reviewed')), findsNothing);
    expect(tester.widget<GlassButton>(find.byKey(const ValueKey('grafana-import-continue'))).onPressed, isNull);
    expect(store.saves, isEmpty);
    expect(service.queries, 0);
  });

  testWidgets('choosing another source revokes review; editing input discards the complete old review', (tester) async {
    final inspector = _Inspector(), service = SavedPanelTestService(), store = SavedPanelTestStore()..panels = [];
    store.sources = [monitoringFixture, otherSource];
    await _open(tester, inspector, service, store);
    await tapKey(tester, 'grafana-constant-use-site');
    await chooseSource(tester);
    await _review(tester, inspector);
    await chooseSource(tester, name: otherSource.name);
    expect(find.byKey(const ValueKey('grafana-constants-reviewed')), findsNothing);
    expect(tester.widget<GlassButton>(find.byKey(const ValueKey('grafana-import-continue'))).onPressed, isNull);
    await _review(tester, inspector);
    await enterKey(tester, 'grafana-import-json', '{}');
    expect(find.byKey(const ValueKey('grafana-constants-reviewed')), findsNothing);
    expect(find.byKey(const ValueKey('grafana-constant-fields')), findsNothing);
    expect(tester.widget<GlassButton>(find.byKey(const ValueKey('grafana-import-continue'))).onPressed, isNull);
    expect(store.saves, isEmpty);
    expect(service.queries, 0);
  });

  testWidgets('locking while manual review is pending denies late completion and never prefills', (tester) async {
    final inspector = _Inspector()..pendingReview = Completer<GrafanaConstantReview>();
    final service = SavedPanelTestService(), store = SavedPanelTestStore()..panels = [];
    final container = await _open(tester, inspector, service, store);
    await tapKey(tester, 'grafana-constant-use-site');
    await chooseSource(tester);
    await tapKey(tester, 'grafana-constants-review');
    await container.read(vaultServiceProvider).lock();
    await settle(tester);
    inspector.pendingReview!.complete(reviewGrafanaConstants(dashboard(), 'panels/0', {'site': ''}));
    await settle(tester);
    expect(find.byType(MonitoringGrafanaImport), findsNothing);
    expect(find.byKey(const ValueKey('saved-panel-query')), findsNothing);
    expect(store.saves, isEmpty);
    expect(service.queries, 0);
  });

  testWidgets('editing a value while review is pending denies the old resolved query', (tester) async {
    final inspector = _Inspector()..pendingReview = Completer<GrafanaConstantReview>();
    final service = SavedPanelTestService(), store = SavedPanelTestStore()..panels = [];
    await _open(tester, inspector, service, store);
    await tapKey(tester, 'grafana-constant-use-site');
    await enterKey(tester, 'grafana-constant-value-site', 'owned-old');
    await chooseSource(tester);
    await tapKey(tester, 'grafana-constants-review');
    await enterKey(tester, 'grafana-constant-value-site', 'owned-new');
    inspector.pendingReview!.complete(reviewGrafanaConstants(dashboard(), 'panels/0', {'site': 'owned-old'}));
    await settle(tester);
    expect(find.byKey(const ValueKey('grafana-constants-reviewed')), findsNothing);
    expect(tester.widget<GlassButton>(find.byKey(const ValueKey('grafana-import-continue'))).onPressed, isNull);
    expect(store.saves, isEmpty);
    expect(service.queries, 0);
  });

  testWidgets('fresh Native source revision rejects a reviewed constant prefill without saving', (tester) async {
    final inspector = _Inspector(), service = SavedPanelTestService(), store = SavedPanelTestStore()..panels = [];
    await _open(tester, inspector, service, store);
    await tapKey(tester, 'grafana-constant-use-site');
    await chooseSource(tester);
    await _review(tester, inspector);
    store.sources = [monitoringFixture.withRecord(revision: 2)];
    await tapKey(tester, 'grafana-import-continue');
    expect(find.byType(MonitoringGrafanaImport), findsNothing);
    expect(find.byKey(const ValueKey('monitoring-error')), findsOneWidget);
    expect(find.byKey(const ValueKey('saved-panel-query')), findsNothing);
    expect(store.saves, isEmpty);
    expect(service.queries, 0);
  });

  testWidgets('retained old field callback cannot populate another panel or revive a locked dialog', (tester) async {
    final inspector = _Inspector(), service = SavedPanelTestService(), store = SavedPanelTestStore()..panels = [];
    final container = await _open(tester, inspector, service, store);
    final oldChanged = tester.widget<MonitoringGrafanaConstants>(find.byType(MonitoringGrafanaConstants)).onChanged;
    await tapKey(tester, 'grafana-import-choose-panels/1');
    oldChanged({'site': 'obsolete-old-form'});
    await settle(tester);
    expect(tester.widget<GlassButton>(find.byKey(const ValueKey('grafana-constants-review'))).onPressed, isNull);
    expect(tester.widget<Checkbox>(find.byKey(const ValueKey('grafana-constant-use-site'))).value, isFalse);
    await container.read(vaultServiceProvider).lock();
    await settle(tester);
    oldChanged({'site': 'obsolete-locked-form'});
    await settle(tester);
    expect(tester.takeException(), isNull);
    expect(find.byType(MonitoringGrafanaImport), findsNothing);
    expect(store.saves, isEmpty);
    expect(service.queries, 0);
  });

  testWidgets('invalid manual literal is a framed typed error and never creates a draft', (tester) async {
    final inspector = _Inspector(), service = SavedPanelTestService(), store = SavedPanelTestStore()..panels = [];
    await _open(tester, inspector, service, store);
    await tapKey(tester, 'grafana-constant-use-site');
    await enterKey(tester, 'grafana-constant-value-site', r'$anotherVariable');
    await chooseSource(tester);
    final action = find.byKey(const ValueKey('grafana-constants-review'));
    await tester.ensureVisible(action);
    await settle(tester);
    await tester.runAsync(() async {
      await tester.tap(action);
      try {
        await inspector.review!.timeout(const Duration(seconds: 5));
        fail('Invalid literal must not produce a typed review');
      } on GrafanaConstantException catch (error) {
        expect(error.problem, GrafanaConstantProblem.invalidBindings);
      }
    });
    await settle(tester);
    expect(find.byKey(const ValueKey('grafana-constants-error')), findsOneWidget);
    expect(tester.widget<GlassButton>(find.byKey(const ValueKey('grafana-import-continue'))).onPressed, isNull);
    expect(store.saves, isEmpty);
    expect(service.queries, 0);
  });

  for (final width in [580.0, 1500.0]) {
    testWidgets('Russian manual constants review at width $width', (tester) async {
      final inspector = _Inspector(), service = SavedPanelTestService(), store = SavedPanelTestStore()..panels = [];
      await _open(tester, inspector, service, store, width: width, russian: true);
      await tapKey(tester, 'grafana-constant-use-site');
      await enterKey(tester, 'grafana-constant-value-site', 'owned-manual');
      await chooseSource(tester);
      await _review(tester, inspector);
      await tester.ensureVisible(find.byKey(const ValueKey('grafana-constants-reviewed')));
      await settle(tester);
      await captureAutomationWorkspace(tester, 'grafana-manual-constants-${width < 800 ? 'narrow' : 'wide'}-ru');
      expect(tester.takeException(), isNull);
      expect(store.saves, isEmpty);
      expect(service.queries, 0);
    }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  }
}
