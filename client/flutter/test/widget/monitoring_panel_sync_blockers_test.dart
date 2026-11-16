import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/monitoring/monitoring_saved_panel_models.dart';
import 'package:consolecrypt/monitoring/monitoring_saved_panels.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../unit/monitoring_panel_sync_blockers_test.dart' show blockers;

Widget host(
  MonitoringPanelSyncSnapshot value, {
  Locale locale = const Locale('en'),
  double scale = 1,
  VoidCallback? check,
}) => MaterialApp(
  locale: locale,
  supportedLocales: AppLocalizations.supportedLocales,
  localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
  builder: (context, child) => MediaQuery(
    data: MediaQuery.of(context).copyWith(textScaler: TextScaler.linear(scale)),
    child: child!,
  ),
  home: Scaffold(
    body: SingleChildScrollView(
      child: MonitoringPanelSyncStatus(snapshot: value, onCheck: check),
    ),
  ),
);
void main() {
  testWidgets('explicit reason disclosure shows old versus unknown client without Ready or hidden check', (
    tester,
  ) async {
    var checks = 0;
    final value = MonitoringPanelSyncSnapshot.fromJson(blockers());
    await tester.pumpWidget(host(value, check: () => checks++));
    await tester.pumpAndSettle();
    expect(checks, 0);
    await tester.tap(find.byKey(const ValueKey('panel-sync-device-blockers')));
    await tester.pumpAndSettle();
    expect(find.text('The client version is unknown.'), findsOneWidget);
    expect(find.text('The reported client version cannot be validated.'), findsOneWidget);
    expect(find.textContaining('Ready'), findsNothing);
    expect(checks, 0);
    expect(tester.takeException(), isNull);
  });
  testWidgets('RU bounded disclosure remains readable and changed Native snapshot replaces old reasons', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(420, 760);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    final value = MonitoringPanelSyncSnapshot.fromJson(blockers());
    await tester.pumpWidget(host(value, locale: const Locale('ru'), scale: 1.5));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('panel-sync-device-blockers')));
    await tester.pumpAndSettle();
    expect(tester.takeException(), isNull);
    final viewport = find.descendant(
      of: find.byKey(const ValueKey('panel-sync-device-blockers')),
      matching: find.byType(ListView),
    );
    expect(tester.getSize(viewport).height, lessThanOrEqualTo(264));
    await tester.pumpWidget(
      host(
        MonitoringPanelSyncSnapshot.fromJson(
          blockers()
            ..['readiness'] = {
              'state': 'serverAdmissionUnverified',
              'checkedActiveDevices': 0,
              'blockedDevices': <Object>[],
            },
        ),
        locale: const Locale('ru'),
        scale: 1.5,
      ),
    );
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('panel-sync-device-blockers')), findsNothing);
    expect(tester.takeException(), isNull);
  });
}
