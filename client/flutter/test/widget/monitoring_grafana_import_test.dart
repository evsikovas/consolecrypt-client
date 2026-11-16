import 'dart:async';
import 'dart:io';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/services/file_dialog_service.dart';
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
import '../helpers/grafana_import_fixture.dart';
import '../helpers/monitoring_fakes.dart';
import '../helpers/monitoring_saved_panel_fakes.dart';
import '../helpers/test_app.dart';

class _ObservedInspector implements GrafanaDashboardInspector {
  final delegate = IsolatedGrafanaDashboardInspector();
  Future<GrafanaInspection>? result;
  @override
  Future<GrafanaInspection> inspect(String input) => result = delegate.inspect(input);
  @override
  void cancel() => delegate.cancel();
  @override
  void dispose() => delegate.dispose();
}

_ObservedInspector? _lastInspector;

class _Files implements FileDialogService {
  Completer<String?>? pending;
  String? path;
  int reads = 0;
  @override
  Future<String?> chooseOpenFile({List<String> extensions = const []}) async {
    expect(extensions, ['json']);
    reads++;
    return pending?.future ?? path;
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => throw StateError('Unexpected file action');
}

class _AdmissionStore extends SavedPanelTestStore {
  @override
  Future<SavedMonitoringPanel> savePanel(SavedPanelDefinition definition, {SavedMonitoringPanel? previous}) async {
    throw const AppException(
      AppErrorCode.validation,
      'private diagnostics never render',
      reason: AppErrorReason.invalidField,
      args: {'field': 'monitoring_panels', 'rule': 'panel_sync_admission_required'},
    );
  }
}

class _PendingCatalogStore extends SavedPanelTestStore {
  Completer<List<MonitoringSource>>? pending;
  @override
  Future<List<MonitoringSource>> load() => pending?.future ?? super.load();
}

Future<(ProviderContainer, MockBackend)> _open(
  WidgetTester tester,
  SavedPanelTestService service,
  SavedPanelTestStore store, {
  _Files? files,
  double width = 1500,
  bool russian = false,
}) async {
  _lastInspector = null;
  final backend = testBackend();
  addTearDown(backend.dispose);
  await backend.debugCreateUnlockedLocalProfile(name: 'Owned import profile');
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
      grafanaDashboardInspectorFactoryProvider.overrideWithValue(() => _lastInspector = _ObservedInspector()),
      monitoringBackendProvider.overrideWithValue(MonitoringTestBackend(service: service, store: store)),
      if (files != null) fileDialogServiceProvider.overrideWithValue(files),
    ],
  );
  final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
  container.read(routerProvider).go(AppRoutes.monitoring);
  await settle(tester);
  return (container, backend);
}

Future<void> inspect(WidgetTester tester, {bool unsupported = false, String query = 'up'}) async {
  await enterKey(tester, 'grafana-import-json', grafanaDashboard(unsupported: unsupported, query: query));
  final button = find.byKey(const ValueKey('grafana-import-inspect'));
  await Scrollable.ensureVisible(tester.element(button), alignment: .5);
  await settle(tester);
  await tester.runAsync(() async {
    await tester.tap(button);
    await _lastInspector!.result!.timeout(const Duration(seconds: 5));
  });
  await settle(tester);
}

Future<void> chooseSource(WidgetTester tester) async {
  final field = find.descendant(
    of: find.byType(MonitoringGrafanaImport),
    matching: find.byType(DropdownButtonFormField<String>),
  );
  await tester.ensureVisible(field);
  await tester.tap(field);
  await settle(tester);
  await tester.tap(find.textContaining(monitoringFixture.name).last);
  await settle(tester);
}

void main() {
  testWidgets('review retains unsupported counts/reasons and never guesses a source or executes a query', (
    tester,
  ) async {
    final service = SavedPanelTestService(), store = SavedPanelTestStore()..panels = [];
    await _open(tester, service, store);
    await tapKey(tester, 'saved-panel-import-grafana');
    await inspect(tester, unsupported: true);
    final l = tester.element(find.byType(MonitoringGrafanaImport)).l10n;
    expect(find.text(l.grafanaImportCounts(1, 1)), findsOneWidget);
    expect(find.text(l.grafanaImportUnsupportedPanel), findsNothing);
    final unsupportedRow = find.byKey(const ValueKey('grafana-import-row-panels/1'));
    await tester.ensureVisible(unsupportedRow);
    await tester.tap(unsupportedRow);
    await settle(tester);
    expect(find.text(l.grafanaImportUnsupportedPanel), findsOneWidget);
    expect(find.byKey(const ValueKey('grafana-import-choose-panels/1')), findsNothing);
    final action = tester.widget<GlassButton>(find.byKey(const ValueKey('grafana-import-continue')));
    expect(action.onPressed, isNull);
    expect(store.saves, isEmpty);
    expect(service.queries, 0);
    expect(service.reads, isEmpty);
    expect(tester.takeException(), isNull);
  });
  testWidgets('Continue prefills existing editor only; explicit manual Save preserves expression and Unknown unit', (
    tester,
  ) async {
    const query = 'sum(rate(owned_total[5m]))\n';
    final service = SavedPanelTestService(), store = SavedPanelTestStore()..panels = [];
    await _open(tester, service, store);
    await tapKey(tester, 'saved-panel-import-grafana');
    await inspect(tester, query: query);
    await tapKey(tester, 'grafana-import-choose-panels/0');
    // Choosing a query still does not silently choose the source.
    expect(tester.widget<GlassButton>(find.byKey(const ValueKey('grafana-import-continue'))).onPressed, isNull);
    await chooseSource(tester);
    await tapKey(tester, 'grafana-import-continue');
    expect(find.byType(MonitoringGrafanaImport), findsNothing);
    expect(tester.widget<TextField>(find.byKey(const ValueKey('saved-panel-query'))).controller!.text, query);
    expect(store.saves, isEmpty);
    expect(service.queries, 0);
    await tapKey(tester, 'saved-panel-save');
    expect(store.saves, hasLength(1));
    final saved = store.saves.single;
    expect(saved.sourceId, monitoringFixture.id);
    expect(saved.sourceRevision, monitoringFixture.revision);
    expect(saved.query, query);
    expect(saved.unitHint, SavedPanelUnitHint.unknown);
    expect(service.queries, 0);
    expect(service.reads, isEmpty);
    expect(tester.takeException(), isNull);
  });
  testWidgets('dirty editor requires replacement approval; Keep and canceled import both preserve draft', (
    tester,
  ) async {
    final service = SavedPanelTestService(), store = SavedPanelTestStore()..panels = [];
    await _open(tester, service, store);
    await tapKey(tester, 'saved-panel-new');
    await enterKey(tester, 'saved-panel-title', 'Keep this title');
    await enterKey(tester, 'saved-panel-query', 'owned_original');
    await tapKey(tester, 'saved-panel-import-grafana');
    expect(find.byType(MonitoringGrafanaImport), findsNothing);
    await tapKey(tester, 'saved-panel-import-keep');
    expect(
      tester.widget<TextField>(find.byKey(const ValueKey('saved-panel-query'))).controller!.text,
      'owned_original',
    );
    await tapKey(tester, 'saved-panel-import-grafana');
    await tapKey(tester, 'saved-panel-import-replace');
    await inspect(tester);
    final l = tester.element(find.byType(MonitoringGrafanaImport)).l10n;
    await tester.tap(find.descendant(of: find.byType(MonitoringGrafanaImport), matching: find.text(l.commonCancel)));
    await settle(tester);
    expect(
      tester.widget<TextField>(find.byKey(const ValueKey('saved-panel-title'))).controller!.text,
      'Keep this title',
    );
    expect(
      tester.widget<TextField>(find.byKey(const ValueKey('saved-panel-query'))).controller!.text,
      'owned_original',
    );
    expect(store.saves, isEmpty);
    expect(service.queries, 0);
    expect(tester.takeException(), isNull);
  });
  testWidgets('explicit user-picked local file loads exact JSON and never auto-inspects or saves', (tester) async {
    final directory = Directory.systemTemp.createTempSync('cc-owned-grafana-import-');
    addTearDown(() {
      final owned = directory.resolveSymbolicLinksSync();
      final root = Directory.systemTemp.resolveSymbolicLinksSync();
      expect(owned.startsWith('$root${Platform.pathSeparator}cc-owned-grafana-import-'), isTrue);
      Directory(owned).deleteSync(recursive: true);
    });
    final file = File('${directory.path}/owned.json');
    final original = ' ${grafanaDashboard()}\n';
    file.writeAsStringSync(original);
    final files = _Files()..path = file.path;
    final service = SavedPanelTestService(), store = SavedPanelTestStore()..panels = [];
    await _open(tester, service, store, files: files);
    await tapKey(tester, 'saved-panel-import-grafana');
    // Start the actual IO callback in the real async zone. No real-IO
    // completion is left waiting on a fake-clock continuation.
    await tester.runAsync(() async {
      await tester.tap(find.byKey(const ValueKey('grafana-import-file')));
      final until = DateTime.now().add(const Duration(seconds: 5));
      while (tester.widget<TextField>(find.byKey(const ValueKey('grafana-import-json'))).controller!.text != original) {
        if (DateTime.now().isAfter(until)) fail('Owned local JSON read did not complete');
        await Future<void>.delayed(const Duration(milliseconds: 10));
      }
    });
    await settle(tester);
    expect(files.reads, 1);
    expect(tester.widget<TextField>(find.byKey(const ValueKey('grafana-import-json'))).controller!.text, original);
    expect(find.byKey(const ValueKey('grafana-import-choose-panels/0')), findsNothing);
    expect(store.saves, isEmpty);
    expect(service.queries, 0);
    expect(tester.takeException(), isNull);
  });
  testWidgets('locking during native file picker denies late file completion and clears import', (tester) async {
    final files = _Files()..pending = Completer<String?>();
    final service = SavedPanelTestService(), store = SavedPanelTestStore()..panels = [];
    final (container, _) = await _open(tester, service, store, files: files);
    await tapKey(tester, 'saved-panel-import-grafana');
    await tapKey(tester, 'grafana-import-file');
    await container.read(vaultServiceProvider).lock();
    await settle(tester);
    files.pending!.complete('must-not-read-after-lock.json');
    await settle(tester);
    expect(find.byType(MonitoringGrafanaImport), findsNothing);
    expect(find.byKey(const ValueKey('saved-panel-query')), findsNothing);
    expect(store.saves, isEmpty);
    expect(service.queries, 0);
    expect(tester.takeException(), isNull);
  });
  testWidgets('profile switch denies old file-picker completion and never prefills the next profile', (tester) async {
    final files = _Files()..pending = Completer<String?>();
    final service = SavedPanelTestService(), store = SavedPanelTestStore()..panels = [];
    final (_, backend) = await _open(tester, service, store, files: files);
    await tapKey(tester, 'saved-panel-import-grafana');
    await tapKey(tester, 'grafana-import-file');
    await backend.debugCreateUnlockedLocalProfile(name: 'Next owned import profile');
    await settle(tester);
    files.pending!.complete('must-not-read-from-previous-profile.json');
    await settle(tester);
    expect(find.byType(MonitoringGrafanaImport), findsNothing);
    expect(find.byKey(const ValueKey('saved-panel-query'), skipOffstage: false), findsNothing);
    expect(store.saves, isEmpty);
    expect(service.queries, 0);
    expect(tester.takeException(), isNull);
  });
  testWidgets('synced admission rejection after manual Save keeps imported draft and shows localized failure', (
    tester,
  ) async {
    final service = SavedPanelTestService(), store = _AdmissionStore()..panels = [];
    await _open(tester, service, store);
    await tapKey(tester, 'saved-panel-import-grafana');
    await inspect(tester);
    await tapKey(tester, 'grafana-import-choose-panels/0');
    await chooseSource(tester);
    await tapKey(tester, 'grafana-import-continue');
    await tapKey(tester, 'saved-panel-save');
    expect(find.byKey(const ValueKey('saved-panel-save')), findsOneWidget);
    expect(tester.widget<TextField>(find.byKey(const ValueKey('saved-panel-query'))).controller!.text, 'up');
    expect(store.saves, isEmpty);
    expect(service.queries, 0);
    expect(find.textContaining('private diagnostics'), findsNothing);
    expect(tester.takeException(), isNull);
  });
  testWidgets('source revision changed during review rejects prefill and preserves the original editor draft', (
    tester,
  ) async {
    final service = SavedPanelTestService(), store = SavedPanelTestStore()..panels = [];
    await _open(tester, service, store);
    await tapKey(tester, 'saved-panel-new');
    await enterKey(tester, 'saved-panel-title', 'Keep old draft');
    await enterKey(tester, 'saved-panel-query', 'owned_original');
    await tapKey(tester, 'saved-panel-import-grafana');
    await tapKey(tester, 'saved-panel-import-replace');
    await inspect(tester);
    await tapKey(tester, 'grafana-import-choose-panels/0');
    await chooseSource(tester);
    store.sources = [monitoringFixture.withRecord(revision: 2)];
    await tapKey(tester, 'grafana-import-continue');
    expect(find.byKey(const ValueKey('monitoring-error')), findsOneWidget);
    expect(
      tester.widget<TextField>(find.byKey(const ValueKey('saved-panel-title'))).controller!.text,
      'Keep old draft',
    );
    expect(
      tester.widget<TextField>(find.byKey(const ValueKey('saved-panel-query'))).controller!.text,
      'owned_original',
    );
    expect(store.saves, isEmpty);
    expect(service.queries, 0);
    expect(tester.takeException(), isNull);
  });
  testWidgets('leaving Monitoring denies a late final catalog response and never fills the hidden editor', (
    tester,
  ) async {
    final service = SavedPanelTestService(), store = _PendingCatalogStore()..panels = [];
    final (container, _) = await _open(tester, service, store);
    await tapKey(tester, 'saved-panel-import-grafana');
    await inspect(tester);
    await tapKey(tester, 'grafana-import-choose-panels/0');
    await chooseSource(tester);
    store.pending = Completer<List<MonitoringSource>>();
    await tapKey(tester, 'grafana-import-continue');
    container.read(routerProvider).go(AppRoutes.terminal);
    await settle(tester);
    store.pending!.complete([monitoringFixture]);
    await settle(tester);
    expect(find.byKey(const ValueKey('saved-panel-query'), skipOffstage: false), findsNothing);
    expect(store.saves, isEmpty);
    expect(service.queries, 0);
    expect(service.reads, isEmpty);
    expect(tester.takeException(), isNull);
  });
  testWidgets('narrow enlarged text keeps offline review and explicit actions accessible', (tester) async {
    final service = SavedPanelTestService(), store = SavedPanelTestStore()..panels = [];
    await _open(tester, service, store, width: 580);
    tester.platformDispatcher.textScaleFactorTestValue = 1.6;
    addTearDown(tester.platformDispatcher.clearTextScaleFactorTestValue);
    await tapKey(tester, 'saved-panel-import-grafana');
    await inspect(tester, unsupported: true);
    await tapKey(tester, 'grafana-import-choose-panels/0');
    await chooseSource(tester);
    expect(tester.widget<GlassButton>(find.byKey(const ValueKey('grafana-import-continue'))).onPressed, isNotNull);
    expect(store.saves, isEmpty);
    expect(service.queries, 0);
    expect(tester.takeException(), isNull);
  });
  for (final width in [580.0, 1500.0]) {
    testWidgets('Russian Grafana import review at width $width', (tester) async {
      await loadAutomationProofFonts(tester);
      final service = SavedPanelTestService(), store = SavedPanelTestStore()..panels = [];
      await _open(tester, service, store, width: width, russian: true);
      await tapKey(tester, 'saved-panel-import-grafana');
      await inspect(tester, unsupported: true);
      await tapKey(tester, 'grafana-import-choose-panels/0');
      await chooseSource(tester);
      await captureAutomationWorkspace(tester, 'grafana-import-${width < 800 ? 'narrow' : 'wide'}-ru');
      expect(store.saves, isEmpty);
      expect(service.queries, 0);
      expect(tester.takeException(), isNull);
    }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  }
}
