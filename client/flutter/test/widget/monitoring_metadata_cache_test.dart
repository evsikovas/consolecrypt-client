import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';
import 'package:consolecrypt/monitoring/monitoring_workspace.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/monitoring_fakes.dart';

final class _NativeCache {
  final requests = <Json>[], cancelled = <String>[];
  Completer<void>? heldClear;
  Json acknowledgement = {'cleared': true};
  late final service = AppCoreMonitoringService(
    read: (request) async {
      requests.add(request);
      final kind = (request['action'] as Json)['kind'];
      Json result;
      if (kind == 'clearCache') {
        await heldClear?.future;
        result = acknowledgement;
      } else if (kind == 'labelValues') {
        result = {
          'values': ['label-${request['sourceId']}'],
          'warnings': <String>[],
          'infos': <String>[],
          'partial': false,
          'cached': true,
        };
      } else {
        throw StateError('Unexpected query/probe: $kind');
      }
      return {'sourceId': request['sourceId'], 'sourceRevision': request['sourceRevision'], 'result': result};
    },
    cancelOperation: (id) async {
      cancelled.add(id);
    },
  );
}

final _original = RuntimeNoticeScope.fromProfile({
  'id': '10000000-0000-4000-8000-000000000001',
  'active': true,
  'vault_state': 'unlocked',
  'notification_scope_id': 'original cache scope 🔐',
})!;

Widget _host(
  MonitoringService service,
  MonitoringTestStore store, {
  Locale locale = const Locale('en'),
  bool active = true,
  RuntimeNoticeScope? scope,
}) => MaterialApp(
  locale: locale,
  supportedLocales: AppLocalizations.supportedLocales,
  localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
  home: Scaffold(
    body: MonitoringWorkspace(
      key: const ValueKey('same-cache-workspace'),
      service: service,
      store: store,
      active: active,
      panelSyncScope: scope ?? _original,
      panelSyncCurrent: () => active,
    ),
  ),
);
Future<void> _tap(WidgetTester tester, String key) async {
  final target = find.byKey(ValueKey(key));
  await tester.ensureVisible(target);
  await tester.tap(target);
  await tester.pump();
  await tester.pump(const Duration(milliseconds: 300));
}

void main() {
  for (final language in ['en', 'ru']) {
    testWidgets('explicit label cache clear $language acknowledges once without a replacement query', (tester) async {
      tester.view.physicalSize = const Size(1100, 1000);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      final cache = _NativeCache(), store = MonitoringTestStore();
      await tester.pumpWidget(_host(cache.service, store, locale: Locale(language)));
      await tester.pumpAndSettle();
      expect(cache.requests, isEmpty);
      expect(find.textContaining('60'), findsWidgets);
      await _tap(tester, 'monitoring-label-values');
      expect(find.text('label-${monitoringFixture.id}'), findsOneWidget);
      await _tap(tester, 'monitoring-clear-metadata-cache');
      expect(find.byKey(const ValueKey('monitoring-metadata-cache-cleared')), findsOneWidget);
      expect(find.text('label-${monitoringFixture.id}'), findsNothing);
      expect(cache.requests.map((r) => (r['action'] as Json)['kind']), ['labelValues', 'clearCache']);
      expect(cache.requests.last['sourceId'], monitoringFixture.id);
      expect(cache.requests.last['sourceRevision'], monitoringFixture.revision);
      await tester.pump(const Duration(seconds: 5));
      expect(cache.requests, hasLength(2));
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox());
    });
  }
  testWidgets('late clear acknowledgment cannot erase a replacement source metadata snapshot', (tester) async {
    final cache = _NativeCache()..heldClear = Completer<void>();
    final second = monitoringFixture.withRecord(id: 'second-source', revision: 3);
    final store = MonitoringTestStore(initial: [monitoringFixture, second]);
    await tester.pumpWidget(_host(cache.service, store));
    await tester.pumpAndSettle();
    await _tap(tester, 'monitoring-clear-metadata-cache');
    tester.widget<GlassSelect<String?>>(find.byKey(const ValueKey('monitoring-source-select'))).onChanged!(second.id);
    await tester.pumpAndSettle();
    await _tap(tester, 'monitoring-label-values');
    expect(find.text('label-${second.id}'), findsOneWidget);
    cache.heldClear!.complete();
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('monitoring-metadata-cache-cleared')), findsNothing);
    expect(find.text('label-${second.id}'), findsOneWidget);
    expect(cache.cancelled, hasLength(1));
    expect(cache.requests.map((r) => (r['action'] as Json)['kind']), ['clearCache', 'labelValues']);
    await tester.pumpWidget(const SizedBox());
  });
  testWidgets('changed original nonce retires a late clear and malformed acknowledgment never claims success', (
    tester,
  ) async {
    final cache = _NativeCache()..heldClear = Completer<void>();
    final store = MonitoringTestStore();
    await tester.pumpWidget(_host(cache.service, store));
    await tester.pumpAndSettle();
    await _tap(tester, 'monitoring-clear-metadata-cache');
    final nextScope = RuntimeNoticeScope.fromProfile({
      'id': _original.profileId,
      'active': true,
      'vault_state': 'unlocked',
      'notification_scope_id': 'new cache scope',
    })!;
    await tester.pumpWidget(_host(cache.service, store, scope: nextScope));
    cache.heldClear!.complete();
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('monitoring-metadata-cache-cleared')), findsNothing);
    cache.heldClear = null;
    cache.acknowledgement = {'cleared': true, 'ready': true};
    await _tap(tester, 'monitoring-clear-metadata-cache');
    expect(find.byKey(const ValueKey('monitoring-metadata-cache-cleared')), findsNothing);
    await tester.pump(const Duration(seconds: 5));
    expect(cache.requests, hasLength(2));
    await tester.pumpWidget(const SizedBox());
  });
  testWidgets('old service exposes no unsupported cache action or cache lifetime claim', (tester) async {
    final service = MonitoringTestService();
    await tester.pumpWidget(_host(service, MonitoringTestStore()));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('monitoring-clear-metadata-cache')), findsNothing);
    expect(find.byKey(const ValueKey('monitoring-metadata-cache-ttl')), findsNothing);
    expect(service.queries, 0);
    await tester.pumpWidget(const SizedBox());
  });
}
