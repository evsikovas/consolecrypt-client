import 'dart:async';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_providers.dart';
import 'package:consolecrypt/monitoring/monitoring_saved_panel_models.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';
import 'package:consolecrypt/monitoring/monitoring_workspace.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_ui_proof.dart';
import '../helpers/monitoring_binding_fakes.dart';
import '../helpers/monitoring_fakes.dart';
import '../helpers/monitoring_saved_panel_fakes.dart';
import '../helpers/test_app.dart';

const otherSource = MonitoringSource(
  id: 'c0a26470-e5af-4dcc-b37f-d71a65fd7891',
  revision: 4,
  name: 'Other saved connection',
  endpoint: 'https://other.invalid',
);

class _Store extends MonitoringTestStore {
  _Store({super.initial});
  final deleted = <MonitoringSource>[];
  Future<void> Function()? switchProfile;
  Completer<void>? pendingDelete;
  MonitoringFailure? failure;
  bool failRefreshAfterDelete = false;
  @override
  Future<List<MonitoringSource>> load() {
    if (failRefreshAfterDelete && deleted.isNotEmpty) {
      throw const MonitoringFailure.local(MonitoringErrorCode.transport);
    }
    return super.load();
  }

  @override
  Future<void> delete(MonitoringSource source) async {
    deleted.add(source);
    if (failure case final value?) throw value;
    if (pendingDelete case final value?) await value.future;
    await super.delete(source);
  }
}

class _Panels extends _Store implements MonitoringSavedPanelStore {
  @override
  List<SavedMonitoringPanel> panels = [savedPanel];
  @override
  Future<SavedMonitoringPanel> savePanel(
    SavedPanelDefinition definition, {
    SavedMonitoringPanel? previous,
  }) => throw StateError('This test never edits saved panels');
  @override
  Future<void> deletePanel(SavedMonitoringPanel panel) =>
      throw StateError('This test never deletes saved panels');
}

Future<ProviderContainer> _open(
  WidgetTester tester,
  MonitoringTestStore store, {
  MonitoringTestService? service,
  double width = 1500,
  bool russian = false,
}) async {
  await loadAutomationProofFonts(tester);
  final backend = testBackend();
  addTearDown(backend.dispose);
  await backend.debugCreateUnlockedLocalProfile(
    name: 'Owned source deletion profile',
  );
  if (store is _Store) {
    store.switchProfile = () async {
      await backend.debugCreateUnlockedLocalProfile(
        name: 'Other source deletion profile',
      );
    };
  }
  if (russian) {
    final settings = backend.services.settings;
    await settings.updateLocal(
      settings.currentLocal.copyWith(themeMode: AppThemeMode.dark),
    );
  }
  await pumpApp(
    tester,
    backend,
    size: Size(width, 1100),
    locale: russian ? AppLocale.ru : AppLocale.en,
    captureBoundary: true,
    overrides: [
      monitoringBackendProvider.overrideWithValue(
        MonitoringTestBackend(store: store, service: service),
      ),
    ],
  );
  final container = ProviderScope.containerOf(
    tester.element(find.byType(Navigator).first),
  );
  container.read(routerProvider).go(AppRoutes.monitoring);
  await settle(tester);
  // Source editing and custom PromQL are now explicitly in Advanced.
  await tapKey(tester, 'monitoring-view-toggle');
  return container;
}

void chooseSource(WidgetTester tester, String id) => tester
    .widget<GlassSelect<String?>>(
      find.byKey(const ValueKey('monitoring-source-select')),
    )
    .onChanged!(id);

void main() {
  testWidgets(
    'trash opens a named modal; Keep preserves connection and performs no native mutation',
    (tester) async {
      final store = _Store();
      await _open(tester, store);
      await tapKey(tester, 'monitoring-delete-source');
      final dialog = find.byKey(const ValueKey('monitoring-delete-dialog'));
      final l = tester.element(dialog).l10n;
      expect(
        find.text(l.monitoringDeleteNamedSource(monitoringFixture.name)),
        findsOneWidget,
      );
      expect(find.text(l.monitoringDeleteCredentialsKept), findsOneWidget);
      expect(store.deleted, isEmpty);
      await tapKey(tester, 'monitoring-delete-cancel');
      expect(dialog, findsNothing);
      expect(store.sources.single.fingerprint, monitoringFixture.fingerprint);
      expect(store.deleted, isEmpty);
    },
  );

  testWidgets(
    'confirm deletes only the reviewed source record and refreshes the authoritative list',
    (tester) async {
      final store = _Store(initial: [monitoringFixture, otherSource]);
      final service = MonitoringTestService();
      await _open(tester, store, service: service);
      await tapKey(tester, 'monitoring-delete-source');
      await tapKey(tester, 'monitoring-delete-confirm');
      expect(store.deleted.single.fingerprint, monitoringFixture.fingerprint);
      expect(store.sources.single.id, otherSource.id);
      expect(
        tester
            .widget<GlassSelect<String?>>(
              find.byKey(const ValueKey('monitoring-source-select')),
            )
            .value,
        otherSource.id,
      );
      expect(service.queries, 0);
      expect(
        find.byKey(const ValueKey('monitoring-delete-dialog')),
        findsNothing,
      );
    },
  );

  testWidgets(
    'existing connection can be deleted from its editor without saving the draft',
    (tester) async {
      final store = _Store();
      await _open(tester, store);
      await tester.tap(find.text('Edit source'));
      await settle(tester);
      await enterKey(
        tester,
        'monitoring-source-name',
        'Unsaved name stays uncommitted',
      );
      await tapKey(tester, 'monitoring-delete-source');
      final l = tester
          .element(find.byKey(const ValueKey('monitoring-delete-dialog')))
          .l10n;
      expect(
        find.text(l.monitoringDeleteNamedSource(monitoringFixture.name)),
        findsOneWidget,
      );
      await tapKey(tester, 'monitoring-delete-confirm');
      expect(store.deleted.single.name, monitoringFixture.name);
      expect(store.sources, isEmpty);
    },
  );

  testWidgets(
    'source selection changes invalidate the open review and cannot delete the latest selection',
    (tester) async {
      final store = _Store(initial: [monitoringFixture, otherSource]);
      await _open(tester, store);
      await tapKey(tester, 'monitoring-delete-source');
      final staleConfirm = tester
          .widget<GlassButton>(
            find.byKey(const ValueKey('monitoring-delete-confirm')),
          )
          .onPressed!;
      chooseSource(tester, otherSource.id);
      await settle(tester);
      expect(
        find.byKey(const ValueKey('monitoring-delete-dialog')),
        findsNothing,
      );
      staleConfirm();
      await settle(tester);
      expect(store.deleted, isEmpty);
      expect(store.sources.length, 2);
    },
  );

  testWidgets(
    'fresh catalog revision dismisses confirmation before its obsolete record can be used',
    (tester) async {
      final store = _Store();
      await _open(tester, store);
      await tapKey(tester, 'monitoring-delete-source');
      store.sources = [monitoringFixture.withRecord(revision: 2)];
      // This is the same production reconciliation entry point used by refresh,
      // binding/panel mutation and route activation, not an alternate delete path.
      final dynamic state = tester.state(find.byType(MonitoringWorkspace));
      // The production State type is private; exercise its real refresh fence
      // without adding a test-only controller or replacing the source store.
      // ignore: avoid_dynamic_calls
      await state.load(preserveSelection: true);
      await settle(tester);
      expect(
        find.byKey(const ValueKey('monitoring-delete-dialog')),
        findsNothing,
      );
      expect(store.deleted, isEmpty);
      expect(store.sources.single.revision, 2);
    },
  );

  testWidgets(
    'lock and hidden route dismiss review; neither old confirmation nor route reentry starts deletion',
    (tester) async {
      final store = _Store();
      final container = await _open(tester, store);
      await tapKey(tester, 'monitoring-delete-source');
      container.read(routerProvider).go(AppRoutes.terminal);
      await settle(tester);
      expect(
        find.byKey(const ValueKey('monitoring-delete-dialog')),
        findsNothing,
      );
      container.read(routerProvider).go(AppRoutes.monitoring);
      await settle(tester);
      await tapKey(tester, 'monitoring-delete-source');
      await container.read(vaultServiceProvider).lock();
      await settle(tester);
      expect(
        find.byKey(const ValueKey('monitoring-delete-dialog')),
        findsNothing,
      );
      expect(store.deleted, isEmpty);
    },
  );

  testWidgets(
    'profile replacement rejects retained confirmation and cannot delete in the new scope',
    (tester) async {
      final store = _Store();
      await _open(tester, store);
      await tapKey(tester, 'monitoring-delete-source');
      final staleConfirm = tester
          .widget<GlassButton>(
            find.byKey(const ValueKey('monitoring-delete-confirm')),
          )
          .onPressed!;
      await store.switchProfile!();
      await settle(tester);
      staleConfirm();
      await settle(tester);
      expect(
        find.byKey(const ValueKey('monitoring-delete-dialog')),
        findsNothing,
      );
      expect(store.deleted, isEmpty);
      expect(store.sources.single.id, monitoringFixture.id);
    },
  );

  testWidgets(
    'accepted delete reconciles after selection changes without restoring the removed source',
    (tester) async {
      final store = _Store(initial: [monitoringFixture, otherSource])
        ..pendingDelete = Completer<void>();
      await _open(tester, store);
      await tapKey(tester, 'monitoring-delete-source');
      await tapKey(tester, 'monitoring-delete-confirm');
      chooseSource(tester, otherSource.id);
      await settle(tester);
      store.pendingDelete!.complete();
      await settle(tester);
      expect(store.deleted.single.id, monitoringFixture.id);
      expect(store.sources.single.id, otherSource.id);
      final select = tester.widget<GlassSelect<String?>>(
        find.byKey(const ValueKey('monitoring-source-select')),
      );
      expect(select.value, otherSource.id);
      expect(
        select.items.map((item) => item.value),
        isNot(contains(monitoringFixture.id)),
      );
    },
  );

  testWidgets('late HTTP completion cannot revive deleted-source data', (
    tester,
  ) async {
    final store = _Store()..pendingDelete = Completer<void>();
    final service = MonitoringTestService()
      ..pending = Completer<QueryOutcome>();
    await _open(tester, store, service: service);
    await tapKey(tester, 'monitoring-auto-enabled');
    await tapKey(tester, 'monitoring-query');
    await tapKey(tester, 'monitoring-cancel');
    await tapKey(tester, 'monitoring-delete-source');
    await tapKey(tester, 'monitoring-delete-confirm');
    service.pending!.complete(service.result);
    store.pendingDelete!.complete();
    await settle(tester);
    expect(store.sources, isEmpty);
    expect(
      find.byKey(const ValueKey('monitoring-result-status')),
      findsNothing,
    );
    expect(find.text('fixture partial warning'), findsNothing);
    expect(service.queries, 1);
  });

  testWidgets(
    'successful delete remains removed if the follow-up catalog refresh fails',
    (tester) async {
      final store = _Store()..failRefreshAfterDelete = true;
      await _open(tester, store);
      await tapKey(tester, 'monitoring-delete-source');
      await tapKey(tester, 'monitoring-delete-confirm');
      expect(store.sources, isEmpty);
      expect(store.deleted.single.id, monitoringFixture.id);
      expect(
        find.byKey(const ValueKey('monitoring-source-select')),
        findsNothing,
      );
      expect(
        find.byKey(const ValueKey('monitoring-delete-source')),
        findsNothing,
      );
      expect(find.byKey(const ValueKey('monitoring-error')), findsOneWidget);
    },
  );

  testWidgets(
    'saved-panel references disable source trash and explain the required explicit order',
    (tester) async {
      final store = _Panels();
      await _open(tester, store);
      final button = tester.widget<GlassButton>(
        find.byKey(const ValueKey('monitoring-delete-source')),
      );
      expect(button.onPressed, isNull);
      final l = tester
          .element(find.byKey(const ValueKey('monitoring-delete-source')))
          .l10n;
      expect(find.text(l.monitoringSavedPanelSourceInUse), findsOneWidget);
      expect(store.panels.single.id, savedPanelId);
      expect(store.deleted, isEmpty);
    },
  );

  testWidgets(
    'host bindings continue blocking deletion without cascading to dependent records',
    (tester) async {
      final store = MonitoringBindingTestStore(rows: [bindingFixture()]);
      await _open(tester, store);
      expect(
        tester
            .widget<GlassButton>(
              find.byKey(const ValueKey('monitoring-delete-source')),
            )
            .onPressed,
        isNull,
      );
      final l = tester
          .element(find.byKey(const ValueKey('monitoring-delete-source')))
          .l10n;
      expect(find.text(l.monitoringBindingSourceInUse), findsOneWidget);
      expect(store.bindings.length, 1);
      expect(store.deleted, isEmpty);
    },
  );

  testWidgets(
    'native reference rejection keeps connection and presents localized error without claiming success',
    (tester) async {
      final store = _Store()
        ..failure = const MonitoringFailure.local(
          MonitoringErrorCode.sourceInUse,
        );
      await _open(tester, store);
      await tapKey(tester, 'monitoring-delete-source');
      await tapKey(tester, 'monitoring-delete-confirm');
      expect(store.sources.single.id, monitoringFixture.id);
      expect(find.byKey(const ValueKey('monitoring-error')), findsOneWidget);
      expect(
        find.byKey(const ValueKey('monitoring-delete-dialog')),
        findsNothing,
      );
    },
  );

  for (final width in [580.0, 1500.0]) {
    testWidgets('Russian named source deletion at width $width', (
      tester,
    ) async {
      await _open(tester, _Store(), width: width, russian: true);
      await tapKey(tester, 'monitoring-delete-source');
      await captureAutomationWorkspace(
        tester,
        'monitoring-source-delete-${width < 800 ? 'narrow' : 'wide'}-ru',
      );
      expect(tester.takeException(), isNull);
      await tapKey(tester, 'monitoring-delete-cancel');
    }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  }
}
