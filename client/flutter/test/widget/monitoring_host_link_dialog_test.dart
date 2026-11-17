import 'dart:async';
import 'dart:io';
import 'dart:ui' as ui;

import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/monitoring/monitoring_host_link_dialog.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_ui_proof.dart';
import '../helpers/monitoring_binding_fakes.dart';
import '../helpers/monitoring_fakes.dart';
import '../helpers/test_app.dart' show settle, tapKey;

const selectedLabels = {'cluster': 'selected-site', 'instance': 'selected:9100', 'job': 'node'};
const targets = QueryOutcome(
  status: 'success',
  httpStatus: 200,
  data: QueryData(
    'vector',
    series: [
      MonitoringSeries(
        {'__name__': 'up', 'job': 'node', 'instance': 'other:9100', 'cluster': 'other-site'},
        [MonitoringSample(1000, SampleKind.finite, 1)],
      ),
      MonitoringSeries(
        {'__name__': 'up', 'job': 'node', 'instance': 'selected:9100', 'cluster': 'selected-site'},
        [MonitoringSample(1000, SampleKind.finite, 1)],
      ),
    ],
  ),
);

Future<void> open(
  WidgetTester tester,
  MonitoringBindingTestStore store,
  MonitoringTestService service,
  List<MonitoringBinding> linked, {
  bool Function()? isCurrent,
  Size size = const Size(1000, 800),
  double textScale = 1,
  Locale locale = const Locale('en'),
}) async {
  await tester.binding.setSurfaceSize(size);
  addTearDown(() => tester.binding.setSurfaceSize(null));
  await tester.pumpWidget(
    MaterialApp(
      theme: AppTheme.dark(platform: TargetPlatform.windows),
      locale: locale,
      supportedLocales: AppLocalizations.supportedLocales,
      localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
      builder: (context, child) => MediaQuery(
        data: MediaQuery.of(context).copyWith(textScaler: TextScaler.linear(textScale)),
        child: child!,
      ),
      home: RepaintBoundary(
        key: const ValueKey('host-link-proof'),
        child: Scaffold(
          body: MonitoringHostLinkDialog(
            host: bindingHost,
            service: service,
            store: store,
            isCurrent: isCurrent ?? () => true,
            onLinked: linked.add,
            onConfigureSource: () {},
          ),
        ),
      ),
    ),
  );
  await settle(tester);
}

Future<void> chooseTarget(WidgetTester tester) async {
  await tapKey(tester, 'monitoring-host-link-target');
  await tester.tap(find.textContaining('instance: selected:9100').last);
  await settle(tester);
}

FilledButton saveButton(WidgetTester tester) =>
    tester.widget<FilledButton>(find.byKey(const ValueKey('monitoring-host-link-save')));

void main() {
  for (final size in [const Size(900, 700), const Size(640, 480)]) {
    testWidgets('existing metrics target selection fits $size with enlarged text', (tester) async {
      await loadAutomationProofFonts(tester);
      final service = MonitoringTestService()..result = targets;
      await open(
        tester,
        MonitoringBindingTestStore(),
        service,
        [],
        size: size,
        textScale: 1.3,
        locale: const Locale('ru'),
      );
      await tapKey(tester, 'monitoring-host-link-find');
      await chooseTarget(tester);
      expect(saveButton(tester).onPressed, isNotNull);
      expect(tester.takeException(), isNull);
      final directory = Platform.environment['CONSOLECRYPT_HOST_METRICS_PROOF_DIR'];
      if (directory != null) {
        final boundary = tester.renderObject<RenderRepaintBoundary>(find.byKey(const ValueKey('host-link-proof')));
        await tester.runAsync(() async {
          final frame = await boundary.toImage();
          try {
            final bytes = await frame.toByteData(format: ui.ImageByteFormat.png);
            final file = File('$directory/link-${size.width.toInt()}.png');
            await file.parent.create(recursive: true);
            await file.writeAsBytes(bytes!.buffer.asUint8List());
          } finally {
            frame.dispose();
          }
        });
      }
    });
  }

  testWidgets('opening host link reads local catalog without contacting metrics source', (tester) async {
    final store = MonitoringBindingTestStore();
    final service = MonitoringTestService()..result = targets;
    final linked = <MonitoringBinding>[];
    await open(tester, store, service, linked);
    expect(store.loads, 1);
    expect(service.queries, 0);
    expect(store.saves, isEmpty);
    expect(linked, isEmpty);
    expect(saveButton(tester).onPressed, isNull);
    await tapKey(tester, 'monitoring-host-link-find');
    expect(service.queries, 1);
    expect(service.last!.expression, 'up');
    expect(store.saves, isEmpty);
    expect(tester.takeException(), isNull);
  });

  testWidgets('found targets require explicit selection before saving', (tester) async {
    final store = MonitoringBindingTestStore();
    final service = MonitoringTestService()..result = targets;
    final linked = <MonitoringBinding>[];
    await open(tester, store, service, linked);
    await tapKey(tester, 'monitoring-host-link-find');
    expect(find.byKey(const ValueKey('monitoring-host-link-target')), findsOneWidget);
    expect(saveButton(tester).onPressed, isNull);
    expect(store.saves, isEmpty);
    await chooseTarget(tester);
    expect(saveButton(tester).onPressed, isNotNull);
    expect(store.saves, isEmpty);
    expect(linked, isEmpty);
  });

  testWidgets('save binds exact saved host source revision and selected complete labels', (tester) async {
    final store = MonitoringBindingTestStore();
    final service = MonitoringTestService()..result = targets;
    final linked = <MonitoringBinding>[];
    await open(tester, store, service, linked);
    await tapKey(tester, 'monitoring-host-link-find');
    await chooseTarget(tester);
    await tapKey(tester, 'monitoring-host-link-save');
    expect(store.saves, hasLength(1));
    final (binding, create) = store.saves.single;
    expect(create, isTrue);
    expect(binding.hostId, bindingHost.id);
    expect(binding.sourceId, monitoringFixture.id);
    expect(binding.sourceRevision, monitoringFixture.revision);
    expect(binding.selector, selectedLabels);
    expect(binding.selector.containsKey('__name__'), isFalse);
    expect(binding.state, MonitoringBindingState.current);
    expect(linked.single.id, store.bindings.single.id);
    expect(service.queries, 1);
    expect(tester.takeException(), isNull);
  });

  testWidgets('failed target read cannot create binding or report linked', (tester) async {
    final store = MonitoringBindingTestStore();
    final service = MonitoringTestService()
      ..result = const QueryOutcome(
        status: 'error',
        httpStatus: 500,
        error: MonitoringFailure('invalid_response', 'Fixture refused query'),
      );
    final linked = <MonitoringBinding>[];
    await open(tester, store, service, linked);
    await tapKey(tester, 'monitoring-host-link-find');
    expect(find.byKey(const ValueKey('monitoring-host-link-error')), findsOneWidget);
    expect(find.byKey(const ValueKey('monitoring-host-link-target')), findsNothing);
    expect(saveButton(tester).onPressed, isNull);
    expect(store.saves, isEmpty);
    expect(linked, isEmpty);
    expect(tester.takeException(), isNull);
  });

  testWidgets('late query after original context retires cannot publish targets or write', (tester) async {
    var current = true;
    final store = MonitoringBindingTestStore();
    final pending = Completer<QueryOutcome>();
    final service = MonitoringTestService()..pending = pending;
    final linked = <MonitoringBinding>[];
    await open(tester, store, service, linked, isCurrent: () => current);
    await tapKey(tester, 'monitoring-host-link-find');
    expect(service.queries, 1);
    current = false;
    pending.complete(targets);
    await settle(tester);
    expect(find.byKey(const ValueKey('monitoring-host-link-target')), findsNothing);
    expect(store.saves, isEmpty);
    expect(linked, isEmpty);
    await tester.pumpWidget(const SizedBox());
    await settle(tester);
    expect(service.cancellations, 1);
    expect(tester.takeException(), isNull);
  });

  testWidgets('source revision change before save rejects stale target selection', (tester) async {
    final store = MonitoringBindingTestStore();
    final service = MonitoringTestService()..result = targets;
    final linked = <MonitoringBinding>[];
    await open(tester, store, service, linked);
    await tapKey(tester, 'monitoring-host-link-find');
    await chooseTarget(tester);
    store.sources = [monitoringFixture.withRecord(revision: monitoringFixture.revision + 1)];
    await tapKey(tester, 'monitoring-host-link-save');
    expect(find.byKey(const ValueKey('monitoring-host-link-error')), findsOneWidget);
    expect(store.saves, isEmpty);
    expect(linked, isEmpty);
    expect(store.loads, 2);
    expect(tester.takeException(), isNull);
  });

  testWidgets('same current binding is reopened without duplicate catalog write', (tester) async {
    final existing = bindingFixture(selector: {'job': 'node', 'instance': 'selected:9100', 'cluster': 'selected-site'});
    final store = MonitoringBindingTestStore(rows: [existing]);
    final service = MonitoringTestService()..result = targets;
    final linked = <MonitoringBinding>[];
    await open(tester, store, service, linked);
    await tapKey(tester, 'monitoring-host-link-find');
    await chooseTarget(tester);
    await tapKey(tester, 'monitoring-host-link-save');
    expect(store.saves, isEmpty);
    expect(store.bindings, hasLength(1));
    expect(linked.single, same(existing));
    expect(tester.takeException(), isNull);
  });

  testWidgets('read-only catalog cannot save a discovered target', (tester) async {
    final store = MonitoringBindingTestStore()..readOnly = true;
    final service = MonitoringTestService()..result = targets;
    final linked = <MonitoringBinding>[];
    await open(tester, store, service, linked);
    await tapKey(tester, 'monitoring-host-link-find');
    await chooseTarget(tester);
    expect(saveButton(tester).onPressed, isNull);
    expect(store.saves, isEmpty);
    expect(linked, isEmpty);
    expect(tester.takeException(), isNull);
  });
}
