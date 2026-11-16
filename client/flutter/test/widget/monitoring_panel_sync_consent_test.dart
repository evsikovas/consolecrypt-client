import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/monitoring/monitoring_panel_sync_consent.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';
import 'package:consolecrypt/monitoring/monitoring_workspace.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/monitoring_saved_panel_fakes.dart';
import '../support/panel_policy_fixture.dart' as f;

Widget host(AppCoreMonitoringSourceStore store, SavedPanelTestService service, {bool active = true}) => MaterialApp(
  locale: const Locale('ru'),
  supportedLocales: AppLocalizations.supportedLocales,
  localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
  builder: (context, child) => MediaQuery(
    data: MediaQuery.of(context).copyWith(textScaler: const TextScaler.linear(1.5)),
    child: child!,
  ),
  home: Scaffold(
    body: MonitoringWorkspace(
      key: const ValueKey('original-workspace'),
      service: service,
      store: store,
      active: active,
      panelSyncScope: f.syncScope(),
      panelSyncCurrent: () => active,
    ),
  ),
);
Future<void> tap(WidgetTester tester, String key) async {
  final finder = find.byKey(ValueKey(key));
  await tester.ensureVisible(finder);
  await tester.pumpAndSettle();
  await tester.tap(finder.hitTestable());
  await tester.pumpAndSettle();
}

void main() {
  testWidgets(
    'real workspace opens metadata-only policy review; blocked floor cannot confirm and local fields remain intact',
    (tester) async {
      tester.view.physicalSize = const Size(500, 900);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);
      final port = f.Port(), service = SavedPanelTestService();
      final store = AppCoreMonitoringSourceStore(
        (_, _) async => savedPanelCatalog()..['panelSync'] = f.syncSnapshot(),
        panelSyncConsentPort: port,
      );
      await tester.pumpWidget(host(store, service));
      await tester.pumpAndSettle();
      expect(port.requests, isEmpty);
      await tap(tester, 'panel-policy-open');
      expect(port.requests, isEmpty);
      expect(find.byKey(const ValueKey('panel-policy-dialog')), findsOneWidget);
      await tap(tester, 'panel-policy-prepare');
      expect(port.requests, hasLength(1));
      expect(port.handle.detailsCalls, 1);
      expect(find.textContaining('всем вашим личным сейфам'), findsOneWidget);
      expect(find.textContaining('18446744073709551615'), findsWidgets);
      expect(tester.widget<GlassButton>(find.byKey(const ValueKey('panel-policy-confirm'))).onPressed, isNull);
      expect(port.handle.confirms, 0);
      expect(port.handle.rechecks, 0);
      expect(service.queries, 0);
      expect(store.readOnly, false);
      expect(tester.takeException(), isNull);
      await tap(tester, 'panel-policy-close');
      expect(port.handle.cancels, 1);
    },
  );
  testWidgets('hidden or replaced workspace retires only original policy dialog under unrelated modal', (tester) async {
    final port = f.Port()..pending = Completer<MonitoringPanelSyncConsentHandle>();
    final service = SavedPanelTestService();
    final store = AppCoreMonitoringSourceStore(
      (_, _) async => savedPanelCatalog()..['panelSync'] = f.syncSnapshot(),
      panelSyncConsentPort: port,
    );
    await tester.pumpWidget(host(store, service));
    await tester.pumpAndSettle();
    await tap(tester, 'panel-policy-open');
    await tester.ensureVisible(find.byKey(const ValueKey('panel-policy-prepare')));
    await tester.tap(find.byKey(const ValueKey('panel-policy-prepare')));
    await tester.pump(const Duration(milliseconds: 250));
    final context = tester.element(find.byKey(const ValueKey('panel-policy-dialog')));
    unawaited(
      showDialog<void>(
        context: context,
        builder: (_) => const AlertDialog(key: ValueKey('unrelated-modal'), content: Text('UNRELATED')),
      ),
    );
    await tester.pump(const Duration(milliseconds: 300));
    await tester.pumpWidget(host(store, service, active: false));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('panel-policy-dialog')), findsNothing);
    expect(find.byKey(const ValueKey('unrelated-modal')), findsOneWidget);
    port.pending!.complete(port.handle);
    await tester.pumpAndSettle();
    expect(port.handle.cancels, 1);
    expect(port.handle.detailsCalls, 0);
    expect(port.handle.confirms, 0);
    expect(tester.takeException(), isNull);
  });
  testWidgets('local profile and missing real capability expose no policy action or hidden requests', (tester) async {
    final service = SavedPanelTestService(), port = f.Port();
    for (final local in [true, false]) {
      final store = AppCoreMonitoringSourceStore(
        (_, _) async => savedPanelCatalog()..['panelSync'] = f.syncSnapshot(storage: local ? 'local' : 'synced'),
        panelSyncConsentPort: local ? port : null,
      );
      await tester.pumpWidget(host(store, service));
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('panel-policy-open')), findsNothing);
      expect(store.readOnly, false);
    }
    expect(port.requests, isEmpty);
    expect(service.queries, 0);
    expect(tester.takeException(), isNull);
  });
}
