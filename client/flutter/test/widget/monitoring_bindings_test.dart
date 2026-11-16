import 'dart:async';

import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_workspace.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_ui_proof.dart';
import '../helpers/monitoring_binding_fakes.dart';
import '../helpers/monitoring_fakes.dart';
import '../helpers/test_app.dart' show enterKey, tapKey, settle;

Future<void> open(
  WidgetTester tester,
  MonitoringBindingTestStore store,
  MonitoringTestService service, {
  bool active = true,
  Size size = const Size(1400, 1100),
  Locale locale = const Locale('en'),
}) async {
  await tester.binding.setSurfaceSize(size);
  addTearDown(() => tester.binding.setSurfaceSize(null));
  await loadAutomationProofFonts(tester);
  await tester.pumpWidget(
    MaterialApp(
      theme: AppTheme.dark(platform: TargetPlatform.windows),
      locale: locale,
      supportedLocales: AppLocalizations.supportedLocales,
      localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
      home: Scaffold(
        body: RepaintBoundary(
          key: const ValueKey('automation-proof-root'),
          child: MonitoringWorkspace(
            store: store,
            service: service,
            active: active,
            hosts: const [bindingHost, otherBindingHost],
            chooseHost: () async => bindingHost,
          ),
        ),
      ),
    ),
  );
  await settle(tester);
}

Future<void> enterDraft(WidgetTester tester, {String name = 'Node exporter'}) async {
  await tapKey(tester, 'monitoring-binding-add');
  await enterKey(tester, 'monitoring-binding-name', name);
  await enterKey(tester, 'monitoring-binding-label-0', 'instance');
  await enterKey(tester, 'monitoring-binding-value-0', 'literal-host:9100');
}

void main() {
  testWidgets('binding create requires explicit saved host and exact labels; no probe/query or inferred assignment', (
    tester,
  ) async {
    final store = MonitoringBindingTestStore(), service = MonitoringTestService();
    await open(tester, store, service);
    expect(store.saves, isEmpty);
    await enterDraft(tester);
    await tapKey(tester, 'monitoring-binding-save');
    expect(find.byKey(const ValueKey('monitoring-binding-error')), findsOneWidget);
    expect(store.saves, isEmpty);
    await tapKey(tester, 'monitoring-binding-host');
    await tapKey(tester, 'monitoring-binding-save');
    expect(store.saves.length, 1);
    final draft = store.saves.single.$1;
    expect(store.saves.single.$2, true);
    expect(draft.id, isEmpty);
    expect(draft.hostId, bindingHost.id);
    expect(draft.sourceId, monitoringFixture.id);
    expect(draft.sourceRevision, 1);
    expect(draft.selector, {'instance': 'literal-host:9100'});
    expect(service.queries, 0);
    expect(find.text('References current'), findsOneWidget);
    expect(
      find.text('Current references describe saved metadata only. Metric availability has not been checked.'),
      findsOneWidget,
    );
    expect(tester.takeException(), isNull);
  });

  testWidgets('same host/source supports multiple named bindings and stale revision requires explicit rereview save', (
    tester,
  ) async {
    final old = bindingFixture(state: MonitoringBindingState.staleSource);
    final store = MonitoringBindingTestStore(
      initial: [monitoringFixture.withRecord(revision: 2)],
      rows: [
        old,
        bindingFixture(id: '66666666-6666-4666-8666-666666666666', name: 'Containers', selector: {'job': 'cadvisor'}),
      ],
    );
    final service = MonitoringTestService();
    await open(tester, store, service);
    expect(find.text('Node exporter'), findsOneWidget);
    expect(find.text('Containers'), findsOneWidget);
    expect(find.text('Source changed — review binding'), findsOneWidget);
    expect(store.saves, isEmpty);
    await tapKey(tester, 'monitoring-binding-edit-${old.id}');
    expect(tester.widget<TextField>(find.byKey(const ValueKey('monitoring-binding-name'))).controller!.text, old.name);
    expect(find.text('Reviewed source revision: 1'), findsOneWidget);
    expect(find.text('Fixture metrics · Source revision: 2'), findsOneWidget);
    expect(store.saves, isEmpty);
    await tapKey(tester, 'monitoring-binding-save');
    final rereview = store.saves.single;
    expect(rereview.$2, false);
    expect(rereview.$1.id, old.id);
    expect(rereview.$1.revision, 7);
    expect(rereview.$1.sourceRevision, 2);
    expect(rereview.$1.selector, old.selector);
    expect(service.queries, 0);
    expect(tester.takeException(), isNull);
  });

  testWidgets('duplicate manual label rows fail without overwriting one value or calling Native', (tester) async {
    final store = MonitoringBindingTestStore();
    await open(tester, store, MonitoringTestService());
    await enterDraft(tester);
    await tapKey(tester, 'monitoring-binding-host');
    await tapKey(tester, 'monitoring-binding-add-label');
    await enterKey(tester, 'monitoring-binding-label-1', 'instance');
    await enterKey(tester, 'monitoring-binding-value-1', 'other-host:9100');
    await tapKey(tester, 'monitoring-binding-save');
    expect(store.saves, isEmpty);
    expect(find.byKey(const ValueKey('monitoring-binding-error')), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('source deletion blocked by references and missing-source orphan remains explicitly deletable', (
    tester,
  ) async {
    final orphan = bindingFixture(
      id: '77777777-7777-4777-8777-777777777777',
      name: 'Orphan',
      sourceId: '88888888-8888-4888-8888-888888888888',
      state: MonitoringBindingState.missingSource,
    );
    final dualOrphan = bindingFixture(
      id: 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa',
      name: 'Missing host and source',
      sourceId: 'bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb',
      state: MonitoringBindingState.missingHost,
    );
    final store = MonitoringBindingTestStore(
      rows: [
        bindingFixture(state: MonitoringBindingState.missingHost),
        orphan,
        dualOrphan,
      ],
    );
    await open(tester, store, MonitoringTestService());
    expect(tester.widget<GlassButton>(find.byKey(const ValueKey('monitoring-delete-source'))).onPressed, isNull);
    expect(find.text('Host unavailable'), findsNWidgets(2));
    expect(find.text('Source unavailable'), findsNWidgets(2));
    expect(find.byKey(ValueKey('monitoring-binding-edit-${orphan.id}')), findsNothing);
    await tapKey(tester, 'monitoring-binding-delete-${orphan.id}');
    expect(store.deleted, isEmpty);
    await tapKey(tester, 'monitoring-binding-delete-confirm-${orphan.id}');
    expect(store.deleted, [orphan.id]);
    expect(find.byKey(ValueKey('monitoring-binding-edit-${dualOrphan.id}')), findsNothing);
    await tapKey(tester, 'monitoring-binding-delete-${dualOrphan.id}');
    expect(store.deleted, [orphan.id]);
    await tapKey(tester, 'monitoring-binding-delete-confirm-${dualOrphan.id}');
    expect(store.deleted, [orphan.id, dualOrphan.id]);
    expect(store.sources.single.id, monitoringFixture.id);
    expect(tester.takeException(), isNull);
  });

  testWidgets('leaving Monitoring discards pending binding editor completion and preserves accepted metadata only', (
    tester,
  ) async {
    final pending = Completer<MonitoringBinding>();
    final store = MonitoringBindingTestStore()..pendingSave = pending;
    final service = MonitoringTestService();
    await open(tester, store, service);
    await enterDraft(tester);
    await tapKey(tester, 'monitoring-binding-host');
    await tapKey(tester, 'monitoring-binding-save');
    await open(tester, store, service, active: false);
    pending.complete(bindingFixture());
    await settle(tester);
    expect(find.byKey(const ValueKey('monitoring-binding-name')), findsNothing);
    expect(service.queries, 0);
    expect(tester.takeException(), isNull);
  });

  testWidgets('source switch discards the previous binding form and late completion without changing the new source', (
    tester,
  ) async {
    final next = monitoringFixture.withRecord(id: '99999999-9999-4999-8999-999999999999', revision: 3);
    final pending = Completer<MonitoringBinding>();
    final store = MonitoringBindingTestStore(initial: [monitoringFixture, next])..pendingSave = pending;
    await open(tester, store, MonitoringTestService());
    await enterDraft(tester);
    await tapKey(tester, 'monitoring-binding-host');
    await tapKey(tester, 'monitoring-binding-save');
    tester.widget<GlassSelect<String?>>(find.byType(GlassSelect<String?>).first).onChanged!(next.id);
    await settle(tester);
    pending.complete(bindingFixture());
    await settle(tester);
    expect(find.byKey(const ValueKey('monitoring-binding-name')), findsNothing);
    expect(find.byKey(const ValueKey('monitoring-binding-33333333-3333-4333-8333-333333333333')), findsNothing);
    expect(tester.widget<GlassSelect<String?>>(find.byType(GlassSelect<String?>).first).value, next.id);
    expect(tester.takeException(), isNull);
  });

  testWidgets('unsupported read-only catalog exposes no writable binding or source action', (tester) async {
    final store = MonitoringBindingTestStore(rows: [bindingFixture()])..readOnly = true;
    await open(tester, store, MonitoringTestService());
    expect(tester.widget<TextButton>(find.byKey(const ValueKey('monitoring-binding-add'))).onPressed, isNull);
    expect(
      tester
          .widget<TextButton>(find.byKey(ValueKey('monitoring-binding-delete-${store.bindings.single.id}')))
          .onPressed,
      isNull,
    );
    expect(store.saves, isEmpty);
    expect(store.deleted, isEmpty);
    expect(tester.takeException(), isNull);
  });

  testWidgets('CAS conflict offers explicit catalog reload and discards the stale editor without retrying a write', (
    tester,
  ) async {
    final store = MonitoringBindingTestStore()
      ..saveError = const MonitoringFailure.local(MonitoringErrorCode.bindingChanged);
    await open(tester, store, MonitoringTestService());
    await enterDraft(tester);
    await tapKey(tester, 'monitoring-binding-host');
    await tapKey(tester, 'monitoring-binding-save');
    expect(find.byKey(const ValueKey('monitoring-binding-error')), findsOneWidget);
    final loads = store.loads;
    await tapKey(tester, 'monitoring-bindings-refresh');
    expect(store.loads, loads + 1);
    expect(store.saves.length, 1);
    expect(find.byKey(const ValueKey('monitoring-binding-name')), findsNothing);
    expect(find.byKey(const ValueKey('monitoring-binding-error')), findsNothing);
    expect(tester.takeException(), isNull);
  });

  for (final width in [540.0, 1400.0]) {
    testWidgets('Russian binding states and manual editor remain readable at width $width', (tester) async {
      final store = MonitoringBindingTestStore(
        initial: [monitoringFixture.withRecord(revision: 2)],
        rows: [bindingFixture(state: MonitoringBindingState.staleSource)],
      );
      await open(tester, store, MonitoringTestService(), size: Size(width, 1100), locale: const Locale('ru'));
      await tapKey(tester, 'monitoring-binding-edit-${store.bindings.single.id}');
      await tester.ensureVisible(find.byKey(const ValueKey('monitoring-bindings')));
      await settle(tester);
      await captureAutomationWorkspace(
        tester,
        width > 1000 ? 'monitoring-bindings-wide-ru' : 'monitoring-bindings-narrow-ru',
      );
      expect(tester.takeException(), isNull);
    });
  }
}
