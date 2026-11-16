import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/monitoring/grafana_import.dart';
import 'package:consolecrypt/monitoring/monitoring_grafana_buffer.dart';
import 'package:consolecrypt/monitoring/monitoring_grafana_import.dart';
import 'package:consolecrypt/monitoring/monitoring_grafana_worker.dart';
import 'package:consolecrypt/monitoring/monitoring_providers.dart';
import 'package:consolecrypt/monitoring/monitoring_screen.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_ui_proof.dart';
import '../helpers/monitoring_fakes.dart';
import '../helpers/monitoring_saved_panel_fakes.dart';
import '../helpers/test_app.dart';

String largeJson({int size = 310000, bool pretty = false}) {
  final data = {
    'schemaVersion': 42,
    'title': 'Owned synthetic large JSON',
    'panels': [
      {
        'title': 'Owned supported panel',
        'type': 'timeseries',
        'datasource': {'type': 'prometheus', 'uid': 'not-a-source-map'},
        'targets': [
          {'expr': 'up'},
        ],
      },
    ],
    'metadata': {'owned': 'x' * size},
  };
  return (pretty ? const JsonEncoder.withIndent('  ') : const JsonEncoder()).convert(data);
}

class _Inspector implements GrafanaDashboardInspector {
  final inputs = <String>[];
  final pending = <Completer<GrafanaInspection>>[];
  int cancellations = 0, disposals = 0;
  @override
  Future<GrafanaInspection> inspect(String text) {
    inputs.add(text);
    final c = Completer<GrafanaInspection>();
    pending.add(c);
    return c.future;
  }

  void complete(int i) => pending[i].complete(GrafanaInspection(inspectGrafanaDashboard(inputs[i])));
  @override
  void cancel() {
    cancellations++;
  }

  @override
  void dispose() {
    disposals++;
  }
}

class _ObservedRealInspector implements GrafanaDashboardInspector {
  final worker = IsolatedGrafanaDashboardInspector();
  Future<GrafanaInspection>? result;
  @override
  Future<GrafanaInspection> inspect(String text) => result = worker.inspect(text);
  @override
  void cancel() => worker.cancel();
  @override
  void dispose() => worker.dispose();
}

Future<void> _open(
  WidgetTester tester,
  GrafanaDashboardInspector worker, {
  Future<String?> Function()? clipboard,
  Future<String?> Function()? file,
  VoidCallback? cancel,
  double width = 1000,
  bool russian = false,
}) async {
  tester.view.physicalSize = Size(width, 1100);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await loadAutomationProofFonts(tester);
  await tester.pumpWidget(
    RepaintBoundary(
      key: const ValueKey('automation-proof-root'),
      child: MaterialApp(
        theme: AppTheme.dark(platform: TargetPlatform.windows),
        locale: Locale(russian ? 'ru' : 'en'),
        supportedLocales: AppLocalizations.supportedLocales,
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        home: Scaffold(
          body: MonitoringGrafanaImport(
            sources: const [monitoringFixture],
            isCurrent: () => true,
            loadJson: file ?? () async => null,
            readClipboard: clipboard,
            inspectorFactory: () => worker,
            onContinue: (_) {},
            onCancel: cancel ?? () {},
          ),
        ),
      ),
    ),
  );
  await settle(tester);
}

void main() {
  for (final pretty in [false, true]) {
    for (final size in [310000, 1950000]) {
      testWidgets('platform editing projects large JSON before EditableText pretty=$pretty size=$size', (tester) async {
        final worker = _Inspector(), raw = largeJson(size: size, pretty: pretty);
        await _open(tester, worker);
        final watch = Stopwatch()..start();
        await tester.enterText(find.byKey(const ValueKey('grafana-import-json')), raw);
        await tester.pump();
        final field = tester.widget<TextField>(find.byKey(const ValueKey('grafana-import-json')));
        final editAndLayoutMs = watch.elapsedMilliseconds;
        expect(field.maxLength, isNull);
        expect(field.readOnly, isTrue);
        expect(field.controller!.text.length, lessThanOrEqualTo(GrafanaInputBuffer.previewLimit));
        expect(worker.inputs, isEmpty);
        await tapKey(tester, 'grafana-import-inspect');
        expect(worker.inputs.single == raw, isTrue);
        expect(find.byKey(const ValueKey('grafana-import-progress')), findsOneWidget);
        for (var i = 0; i < 8; i++) {
          await tester.pump(const Duration(milliseconds: 16));
        }
        expect(tester.widget<GlassButton>(find.byKey(const ValueKey('grafana-import-cancel'))).onPressed, isNotNull);
        debugPrint(
          'SYNTHETIC_FIXED_INPUT ${jsonEncode({'bytes': utf8.encode(raw).length, 'pretty': pretty, 'editAndLayoutMs': editAndLayoutMs, 'displayUnits': field.controller!.text.length})}',
        );
        worker.complete(0);
        await settle(tester);
        expect(tester.takeException(), isNull);
      });
    }
  }
  testWidgets('real isolated worker completion reaches the bounded widget without a UI-thread parser', (tester) async {
    final worker = _ObservedRealInspector(), raw = largeJson(size: 1950000);
    await _open(tester, worker);
    await tester.enterText(find.byKey(const ValueKey('grafana-import-json')), raw);
    await settle(tester);
    final result = await tester.runAsync(() async {
      await tester.tap(find.byKey(const ValueKey('grafana-import-inspect')));
      return worker.result!.timeout(const Duration(seconds: 5));
    });
    expect(result!.report.originalJson == raw, isTrue);
    await settle(tester);
    expect(find.text('Owned supported panel'), findsOneWidget);
    expect(find.byKey(const ValueKey('grafana-import-progress')), findsNothing);
  });
  testWidgets('explicit clipboard read is bounded before creating an editable value', (tester) async {
    final worker = _Inspector(), raw = largeJson();
    var reads = 0;
    await _open(
      tester,
      worker,
      clipboard: () async {
        reads++;
        return raw;
      },
    );
    await tapKey(tester, 'grafana-import-paste');
    final field = tester.widget<TextField>(find.byKey(const ValueKey('grafana-import-json')));
    expect(reads, 1);
    expect(field.controller!.text.length, lessThanOrEqualTo(8192));
    expect(field.readOnly, isTrue);
    await tapKey(tester, 'grafana-import-inspect');
    expect(worker.inputs.single == raw, isTrue);
    worker.complete(0);
    await settle(tester);
  });
  testWidgets('keyboard Paste follows the bounded clipboard path and new input cancels pending inspection', (
    tester,
  ) async {
    final worker = _Inspector(), raw = largeJson();
    await _open(tester, worker, clipboard: () async => raw);
    await tester.tap(find.byKey(const ValueKey('grafana-import-json')));
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyV);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    await settle(tester);
    expect(tester.widget<TextField>(find.byKey(const ValueKey('grafana-import-json'))).controller!.text.length, 8192);
    await tapKey(tester, 'grafana-import-inspect');
    final before = worker.cancellations;
    await tapKey(tester, 'grafana-import-clear');
    expect(worker.cancellations, greaterThan(before));
    worker.complete(0);
    await settle(tester);
    expect(find.text('Owned supported panel'), findsNothing);
    expect(find.byKey(const ValueKey('grafana-import-progress')), findsNothing);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  testWidgets('large file becomes a preview and explicit Cancel remains responsive while worker is pending', (
    tester,
  ) async {
    final worker = _Inspector(), raw = largeJson(pretty: true);
    var cancelled = 0;
    await _open(
      tester,
      worker,
      file: () async => raw,
      cancel: () {
        cancelled++;
      },
    );
    await tapKey(tester, 'grafana-import-file');
    expect(tester.widget<TextField>(find.byKey(const ValueKey('grafana-import-json'))).readOnly, isTrue);
    await tapKey(tester, 'grafana-import-inspect');
    await tapKey(tester, 'grafana-import-cancel');
    expect(cancelled, 1);
    worker.complete(0);
    await settle(tester);
    expect(find.text('Owned supported panel'), findsNothing);
    expect(find.byKey(const ValueKey('grafana-import-progress')), findsNothing);
  });
  testWidgets('late clipboard response cannot replace input edited while clipboard was pending', (tester) async {
    final worker = _Inspector(), pending = Completer<String?>();
    await _open(tester, worker, clipboard: () => pending.future);
    await tapKey(tester, 'grafana-import-paste');
    await tester.enterText(find.byKey(const ValueKey('grafana-import-json')), '{"new":"typed"}');
    pending.complete(largeJson());
    await settle(tester);
    expect(
      tester.widget<TextField>(find.byKey(const ValueKey('grafana-import-json'))).controller!.text,
      '{"new":"typed"}',
    );
  });
  testWidgets('unsupported whole format shows actual identifiers and panels-not-inspected instead of zero counts', (
    tester,
  ) async {
    final worker = _Inspector();
    await _open(tester, worker);
    final raw = jsonEncode({
      'apiVersion': 'dashboard.grafana.app/v999',
      'kind': 'Dashboard',
      'spec': {
        'elements': {'owned': <String, Object?>{}},
      },
    });
    await tester.enterText(find.byKey(const ValueKey('grafana-import-json')), raw);
    await tapKey(tester, 'grafana-import-inspect');
    worker.pending.single.complete(
      GrafanaInspection(inspectGrafanaDashboard(raw), apiVersion: 'dashboard.grafana.app/v999', kind: 'Dashboard'),
    );
    await settle(tester);
    final l = tester.element(find.byKey(const ValueKey('grafana-import-not-inspected'))).l10n;
    expect(find.text(l.grafanaImportCounts(0, 0)), findsNothing);
    // Whole-format metadata remains available behind the explicit notes control.
    final details = find.byKey(const ValueKey('grafana-import-notes-toggle'));
    await tester.ensureVisible(details);
    await tester.tap(details);
    await settle(tester);
    expect(
      find.byWidgetPredicate((w) => w is Text && (w.data?.contains('dashboard.grafana.app/v999') ?? false)),
      findsOneWidget,
    );
    expect(tester.widget<GlassButton>(find.byKey(const ValueKey('grafana-import-continue'))).onPressed, isNull);
  });
  testWidgets('profile lock disposes inspector and denies late large-input completion without saving or querying', (
    tester,
  ) async {
    final backend = testBackend(),
        worker = _Inspector(),
        service = SavedPanelTestService(),
        store = SavedPanelTestStore();
    addTearDown(backend.dispose);
    await backend.debugCreateUnlockedLocalProfile(name: 'Owned responsive import profile');
    await pumpApp(
      tester,
      backend,
      overrides: [
        monitoringBackendProvider.overrideWithValue(MonitoringTestBackend(service: service, store: store)),
        grafanaDashboardInspectorFactoryProvider.overrideWithValue(() => worker),
      ],
    );
    final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    container.read(routerProvider).go(AppRoutes.monitoring);
    await settle(tester);
    await tapKey(tester, 'saved-panel-import-grafana');
    await tester.enterText(find.byKey(const ValueKey('grafana-import-json')), largeJson());
    await tapKey(tester, 'grafana-import-inspect');
    await container.read(vaultServiceProvider).lock();
    await settle(tester);
    expect(worker.disposals, 1);
    worker.complete(0);
    await settle(tester);
    expect(find.byType(MonitoringGrafanaImport), findsNothing);
    expect(service.queries, 0);
    expect(store.saves, isEmpty);
  });
  for (final width in [580.0, 1500.0]) {
    testWidgets('Russian large JSON progress remains readable at width $width', (tester) async {
      final worker = _Inspector();
      await _open(tester, worker, width: width, russian: true);
      await tester.enterText(find.byKey(const ValueKey('grafana-import-json')), largeJson(pretty: true));
      await tapKey(tester, 'grafana-import-inspect');
      await captureAutomationWorkspace(tester, 'grafana-responsive-${width < 800 ? 'narrow' : 'wide'}-ru');
      worker.complete(0);
      await settle(tester);
      expect(tester.takeException(), isNull);
    }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  }
}
