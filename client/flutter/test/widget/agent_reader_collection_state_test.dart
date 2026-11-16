import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/l10n/app_localizations_en.dart';
import 'package:consolecrypt/l10n/app_localizations_ru.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../support/agent_reader_fixture.dart' as f;
import '../support/agent_reader_history_fixture.dart';
import 'agent_reader_history_test.dart' as history;
import 'agent_reader_test.dart' as old;

void main() {
  for (final locale in ['en', 'ru']) {
    testWidgets('recorded collector states in $locale remain distinct with unknown values and no automatic read', (
      tester,
    ) async {
      tester.view.physicalSize = const Size(500, 900);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      final body = f.history(count: 8);
      final states = <String, int?>{
        'running': 1,
        'disabled': 0,
        'stopped': 0,
        'not_running': 0,
        'starting': null,
        'degraded': null,
        'blocked': null,
        'unknown': null,
      };
      final oldPoints = body['points'] as List;
      final points = <Map<String, dynamic>>[];
      var index = 0;
      for (final entry in states.entries) {
        points.add(<String, dynamic>{
          ...oldPoints[index++] as Map<String, dynamic>,
          'collection_state_version': 1,
          'collection_state': entry.key,
          'agent_collector_running': entry.value,
          'agent_uptime_seconds': null,
        });
      }
      body['points'] = points;
      final port = ReadPort()..body = f.dataset(body);
      final AppLocalizations l = locale == 'en' ? AppLocalizationsEn() : AppLocalizationsRu();
      await tester.pumpWidget(old.host(port, f.Files(), locale: Locale(locale), scale: 1.5));
      await tester.pumpAndSettle();
      expect(port.requests, isEmpty);
      await history.ready(tester);
      await old.tap(tester, 'agent-consume');
      for (final label in [
        l.agentReaderCollectionRunning,
        l.agentReaderCollectionDisabled,
        l.agentReaderCollectionStopped,
        l.agentReaderCollectionNotRunning,
        l.agentReaderCollectionStarting,
        l.agentReaderCollectionDegraded,
        l.agentReaderCollectionBlocked,
        l.commonUnknown,
      ]) {
        expect(find.text(label), findsOneWidget);
      }
      expect(find.text(l.agentReaderCollectionRecorded), findsOneWidget);
      expect(find.text('agent_uptime_seconds: ${l.commonUnknown}'), findsNWidgets(8));
      expect(find.text('agent_collector_running: ${l.commonUnknown}'), findsNWidgets(4));
      await tester.pump(const Duration(seconds: 2));
      await tester.pump();
      expect(port.handles.single.reads, 1);
      expect(port.requests, hasLength(1));
      expect(port.statuses, hasLength(1));
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox.shrink());
      await tester.pump();
    });
  }
  testWidgets('long recorded-state label keeps its full tooltip at text scale two without overflow or another read', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(500, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    final body = f.history(count: 1);
    body['points'] = [
      <String, dynamic>{
        ...(body['points'] as List).single as Map<String, dynamic>,
        'agent_collector_running': null,
        'collection_state_version': 1,
        'collection_state': 'degraded',
      },
    ];
    final port = ReadPort()..body = f.dataset(body);
    final original = old.host(port, f.Files(), locale: const Locale('ru')) as MaterialApp;
    final scale = ValueNotifier<double>(1.5);
    addTearDown(scale.dispose);
    await tester.pumpWidget(
      MaterialApp(
        locale: original.locale,
        supportedLocales: original.supportedLocales,
        localizationsDelegates: original.localizationsDelegates,
        home: original.home,
        builder: (context, child) => ValueListenableBuilder<double>(
          valueListenable: scale,
          builder: (context, value, _) => MediaQuery(
            data: MediaQuery.of(context).copyWith(textScaler: TextScaler.linear(value)),
            child: child!,
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    await history.ready(tester);
    await old.tap(tester, 'agent-consume');
    scale.value = 2;
    await tester.pumpAndSettle();
    final full = AppLocalizationsRu().agentReaderCollectionDegraded;
    expect(find.byTooltip(full), findsOneWidget);
    expect(tester.widget<Text>(find.text(full)).overflow, TextOverflow.ellipsis);
    expect(port.handles.single.reads, 1);
    expect(port.requests, hasLength(1));
    expect(port.statuses, hasLength(1));
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox.shrink());
    await tester.pump();
  });
  testWidgets('legacy not-running cause is not inferred and state paging remains local', (tester) async {
    final body = f.history(count: 70);
    final last = (body['points'] as List).last as Map<String, dynamic>;
    last['agent_collector_running'] = 0;
    final port = ReadPort()..body = f.dataset(body);
    final l = AppLocalizationsEn();
    await tester.pumpWidget(old.host(port, f.Files()));
    await tester.pumpAndSettle();
    await history.ready(tester);
    await old.tap(tester, 'agent-consume');
    expect(find.text(l.agentReaderCollectionNotRunning), findsOneWidget);
    expect(find.text(l.agentReaderCollectionDisabled), findsNothing);
    expect(find.text(l.agentReaderCollectionStopped), findsNothing);
    await old.tap(tester, 'agent-history-earlier');
    expect(find.text(l.agentReaderCollectionNotRunning), findsNothing);
    await tester.pump(const Duration(seconds: 2));
    await tester.pump();
    await old.tap(tester, 'agent-history-later');
    expect(find.text(l.agentReaderCollectionNotRunning), findsOneWidget);
    expect(port.handles.single.reads, 1);
    expect(port.requests, hasLength(1));
    expect(port.statuses, hasLength(1));
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox.shrink());
    await tester.pump();
  });
}
