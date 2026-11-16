import 'dart:async';

import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/l10n/app_localizations_en.dart';
import 'package:consolecrypt/l10n/app_localizations_ru.dart';
import 'package:consolecrypt/monitoring/monitoring_binding_models.dart';
import 'package:consolecrypt/monitoring/monitoring_binding_reader.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_ui_proof.dart';
import '../helpers/monitoring_binding_fakes.dart';
import '../helpers/monitoring_binding_reads_fakes.dart';
import '../helpers/monitoring_fakes.dart';
import '../helpers/test_app.dart' show enterKey, tapKey, settle;

Future<void> open(
  WidgetTester tester,
  MonitoringBindingReadTestService reader, {
  bool active = true,
  Size size = const Size(1400, 1100),
  Locale locale = const Locale('en'),
  MonitoringSource? source,
}) async {
  await tester.binding.setSurfaceSize(size);
  addTearDown(() => tester.binding.setSurfaceSize(null));
  await loadAutomationProofFonts(tester);
  await tester.pumpWidget(
    MaterialApp(
      locale: locale,
      supportedLocales: AppLocalizations.supportedLocales,
      theme: AppTheme.dark(platform: TargetPlatform.windows),
      localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
      home: Scaffold(
        body: RepaintBoundary(
          key: const ValueKey('automation-proof-root'),
          child: SingleChildScrollView(
            child: MonitoringBindingReaderView(
              key: ValueKey(source?.revision ?? 1),
              reader: reader,
              source: source ?? monitoringFixture,
              binding: bindingFixture(),
              active: active,
            ),
          ),
        ),
      ),
    ),
  );
  await settle(tester);
}

Future<void> selectMemory(WidgetTester tester) async {
  tester.widget<GlassSelect<MonitoringPanel>>(find.byKey(const ValueKey('monitoring-binding-panel'))).onChanged!(
    MonitoringPanel.memory,
  );
  await settle(tester);
}

void main() {
  test('all ten panels, eight units and seventeen metric semantics have explicit RU/EN labels', () {
    final en = AppLocalizationsEn(), ru = AppLocalizationsRu();
    expect(MonitoringPanel.values.length, 10);
    for (final panel in MonitoringPanel.values) {
      expect(monitoringPanelLabel(en, panel), isNotEmpty);
      expect(monitoringPanelLabel(ru, panel), isNotEmpty);
    }
    for (final unit in MonitoringUnit.values) {
      expect(monitoringUnitLabel(en, unit), isNotEmpty);
      expect(monitoringUnitLabel(ru, unit), isNotEmpty);
    }
    for (final metric in MonitoringMetric.values) {
      expect(monitoringMetricLabel(en, metric), isNotEmpty);
      expect(monitoringMetricLabel(ru, metric), isNot(monitoringMetricLabel(en, metric)));
    }
    expect(monitoringMetricLabel(en, MonitoringMetric.containerCpu), contains('not host percentage'));
    expect(monitoringMetricLabel(en, MonitoringMetric.filesystemUnavailablePercent), contains('reserved blocks'));
  });

  testWidgets('identity check uses selected end, reports down matching candidate, and never auto-loads a panel', (
    tester,
  ) async {
    final reader = MonitoringBindingReadTestService();
    await open(tester, reader);
    expect(reader.discoveries, isEmpty);
    expect(reader.panels, isEmpty);
    expect(
      tester.widget<GlassSelect<MonitoringPanel>>(find.byKey(const ValueKey('monitoring-binding-panel'))).items.length,
      10,
    );
    await enterKey(tester, 'monitoring-binding-start', '1970-01-01T00:00:00Z');
    await enterKey(tester, 'monitoring-binding-end', '1970-01-01T00:16:40.000Z');
    await tapKey(tester, 'monitoring-binding-apply-window');
    await tapKey(tester, 'monitoring-binding-discover');
    expect(reader.discoveries.single, (bindingFixture().id, 7, 1000.0));
    expect(find.text('One candidate matched'), findsOneWidget);
    expect(reader.panels, isEmpty);
    await tester.ensureVisible(find.text('Identity observations'));
    await tester.tap(find.text('Identity observations'));
    await settle(tester);
    expect(find.text('1000.0: 0.0'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  for (final state in ['missing', 'ambiguous', 'unavailable']) {
    testWidgets('$state identity remains distinct without automatic fallback queries', (tester) async {
      final reader = MonitoringBindingReadTestService()
        ..discoveryResult = BindingReadResult.fromJson(
          bindingDiscoveryJson(state: state, candidates: state == 'ambiguous' ? 2 : 0),
        );
      await open(tester, reader);
      await tapKey(tester, 'monitoring-binding-discover');
      expect(find.byKey(const ValueKey('monitoring-binding-read-state')), findsOneWidget);
      expect(reader.panels, isEmpty);
      expect(reader.queries, 0);
      if (state == 'ambiguous') expect(find.text('Original candidate indices: 0, 1'), findsOneWidget);
      expect(tester.takeException(), isNull);
    });
  }

  testWidgets('panel partial warnings units semantics and native nonfinite samples remain visible', (tester) async {
    final reader = MonitoringBindingReadTestService()
      ..panelResult = (p) => bindingPanelJson(panel: p, state: 'partial', partial: true, limited: true);
    await open(tester, reader);
    await selectMemory(tester);
    await tapKey(tester, 'monitoring-binding-load-panel');
    expect(reader.discoveries, isEmpty); // Native panel operation independently resolves identity.
    expect(reader.panels.single.$3, MonitoringPanel.memory);
    expect(find.text('Partial panel result'), findsOneWidget);
    expect(find.text('Warning: Synthetic metric warning'), findsNWidgets(2));
    expect(find.text('Percent'), findsOneWidget);
    expect(find.text('Bytes'), findsOneWidget);
    expect(find.text('NaN'), findsNWidgets(2));
    expect(find.text('+Inf'), findsNWidgets(2));
    expect(find.text('Total memory minus MemAvailable, as a percentage of total memory.'), findsOneWidget);
    expect(find.textContaining('Native-owned-expression'), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('no data panel shows empty interval and never invents zero measurements', (tester) async {
    final reader = MonitoringBindingReadTestService()..panelResult = (p) => bindingPanelJson(panel: p, state: 'noData');
    await open(tester, reader);
    await selectMemory(tester);
    await tapKey(tester, 'monitoring-binding-load-panel');
    expect(find.text('No samples for the selected interval'), findsOneWidget);
    expect(find.byKey(const ValueKey('monitoring-no-data')), findsNWidgets(2));
    expect(find.text('0.0'), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('missing required metrics and probe warnings stay explicit without showing a fabricated panel', (
    tester,
  ) async {
    final reader = MonitoringBindingReadTestService()
      ..panelResult = (panel) => {
        ...bindingDiscoveryJson(),
        'kind': 'bindingPanel',
        'panel': panel.name,
        'state': 'unavailable',
        'preparation': {
          'state': 'unavailable',
          'panel': panel.name,
          'reason': 'missingMetrics',
          'missingMetrics': ['node_memory_MemTotal_bytes'],
          'probe': identityOutcome(partial: true, warnings: ['Synthetic probe warning']),
          'error': null,
        },
      };
    await open(tester, reader);
    await selectMemory(tester);
    await tapKey(tester, 'monitoring-binding-load-panel');
    expect(find.text('Required metrics are absent'), findsOneWidget);
    expect(find.text('node_memory_MemTotal_bytes'), findsOneWidget);
    expect(find.text('Warning: Synthetic probe warning'), findsOneWidget);
    final l = tester.element(find.byKey(const ValueKey('monitoring-binding-read-state'))).l10n;
    expect(find.text(l.monitoringPartialResult), findsOneWidget);
    expect(find.byKey(const ValueKey('monitoring-panel-query-memoryAvailable')), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('changing panel time cancels its pending operation and rejects the late response', (tester) async {
    final pending = Completer<BindingReadResult>();
    final reader = MonitoringBindingReadTestService()..pendingBinding = pending;
    await open(tester, reader);
    await tapKey(tester, 'monitoring-binding-load-panel');
    expect(find.byKey(const ValueKey('monitoring-binding-cancel')), findsOneWidget);
    await enterKey(tester, 'monitoring-binding-step', '30');
    expect(reader.bindingCancellations, greaterThan(0));
    pending.complete(BindingReadResult.fromJson(bindingPanelJson(panel: MonitoringPanel.hostOverview)));
    await settle(tester);
    expect(find.byKey(const ValueKey('monitoring-binding-read-state')), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('hidden panel rejects late results and reactivation does not auto-query', (tester) async {
    final pending = Completer<BindingReadResult>();
    final reader = MonitoringBindingReadTestService()..pendingBinding = pending;
    await open(tester, reader);
    await tapKey(tester, 'monitoring-binding-discover');
    await open(tester, reader, active: false);
    pending.complete(BindingReadResult.fromJson(bindingDiscoveryJson()));
    await settle(tester);
    expect(find.byKey(const ValueKey('monitoring-binding-read-state')), findsNothing);
    await open(tester, reader);
    expect(reader.discoveries.length, 1);
    expect(tester.takeException(), isNull);
  });

  for (final width in [540.0, 1400.0]) {
    testWidgets('Russian panel controls and unavailable reasons stay readable at width $width', (tester) async {
      final reader = MonitoringBindingReadTestService()
        ..discoveryResult = BindingReadResult.fromJson(bindingDiscoveryJson(state: 'ambiguous', candidates: 2));
      await open(tester, reader, size: Size(width, 1100), locale: const Locale('ru'));
      await tapKey(tester, 'monitoring-binding-discover');
      await captureAutomationWorkspace(
        tester,
        width > 1000 ? 'monitoring-panel-wide-ru' : 'monitoring-panel-narrow-ru',
      );
      expect(tester.takeException(), isNull);
    });
  }
}
