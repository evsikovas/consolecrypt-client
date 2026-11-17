import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/monitoring/monitoring_agent_overview.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

Future<void> pumpOverview(WidgetTester tester, Widget child, {double scale = 1}) async {
  await tester.pumpWidget(
    MaterialApp(
      locale: const Locale('ru'),
      supportedLocales: AppLocalizations.supportedLocales,
      localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
      builder: (context, child) => MediaQuery(
        data: MediaQuery.of(context).copyWith(textScaler: TextScaler.linear(scale)),
        child: child!,
      ),
      home: Scaffold(body: child),
    ),
  );
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('explains optional Agent and keeps three states unknown until checked', (tester) async {
    var reads = 0, associations = 0;
    await pumpOverview(
      tester,
      MonitoringAgentOverview(
        isCurrent: () => true,
        hostName: 'Saved host',
        onOpenAgentReader: () => reads++,
        onOpenAgentAssociation: () => associations++,
      ),
    );
    expect(find.text('Статус не проверен'), findsNWidgets(3));
    expect(find.text('Программа на сервере'), findsOneWidget);
    expect(find.text('Подключение устройства'), findsOneWidget);
    expect(find.text('Сбор метрик'), findsOneWidget);
    expect(find.textContaining('работают без этого агента'), findsOneWidget);
    expect(find.byKey(const ValueKey('monitoring-agent-observed-version')), findsNothing);
    expect(reads, 0);
    expect(associations, 0);
    await tester.ensureVisible(find.byKey(const ValueKey('monitoring-agent-read')));
    await tester.tap(find.byKey(const ValueKey('monitoring-agent-read')));
    await tester.pumpAndSettle();
    expect(reads, 1);
    expect(associations, 0);
    await tester.ensureVisible(find.byKey(const ValueKey('monitoring-agent-association')));
    await tester.tap(find.byKey(const ValueKey('monitoring-agent-association')));
    expect(associations, 1);
    expect(find.text('Статус не проверен'), findsNWidgets(3));
    expect(tester.takeException(), isNull);
  });

  testWidgets('missing capabilities never show fake management or an installer project', (tester) async {
    tester.view.physicalSize = const Size(640, 480);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await pumpOverview(tester, MonitoringAgentOverview(isCurrent: () => true), scale: 1.4);
    expect(find.byKey(const ValueKey('monitoring-agent-read')), findsNothing);
    expect(find.byKey(const ValueKey('monitoring-agent-association')), findsNothing);
    expect(find.text('Статус не проверен'), findsNWidgets(3));
    expect(find.textContaining('playbook'), findsNothing);
    expect(find.textContaining('manifest'), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('an action prepared before the original scope retired cannot dispatch', (tester) async {
    var live = true, reads = 0;
    await pumpOverview(tester, MonitoringAgentOverview(isCurrent: () => live, onOpenAgentReader: () => reads++));
    await tester.ensureVisible(find.byKey(const ValueKey('monitoring-agent-read')));
    live = false;
    await tester.tap(find.byKey(const ValueKey('monitoring-agent-read')));
    expect(reads, 0);
    await pumpOverview(tester, MonitoringAgentOverview(isCurrent: () => live));
    expect(find.byKey(const ValueKey('monitoring-agent-overview')), findsNothing);
  });
}
