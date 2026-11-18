import 'dart:async';

import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/monitoring_host_live.dart';
import 'package:consolecrypt/monitoring/monitoring_metric_browser.dart';
import 'package:consolecrypt/monitoring/monitoring_result_view.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/monitoring_fakes.dart';
import '../helpers/test_app.dart' show settle;
import '../support/agent_association_fixture.dart' as fixture;

class _Reader implements MonitoringHostLiveReader {
  final samples = StreamController<HostMetricsSample>();
  final closed = <String>[];
  String? operation;
  Host? selected;

  @override
  Stream<HostMetricsSample> watchHost(Host host, RuntimeNoticeScope scope, String operationId) {
    selected = host;
    operation = operationId;
    return samples.stream;
  }

  @override
  Future<void> cancelHostWatch(String operationId) async => closed.add(operationId);
}

Widget _app(Widget widget) => MaterialApp(
  theme: AppTheme.dark(platform: TargetPlatform.windows),
  locale: const Locale('en'),
  supportedLocales: AppLocalizations.supportedLocales,
  localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
  home: Scaffold(body: widget),
);

void main() {
  testWidgets('live host view shows real values and cancels its own watch when closed', (tester) async {
    final host = Host.create(name: 'Test Linux', address: '192.0.2.30'), reader = _Reader();
    await tester.pumpWidget(
      _app(MonitoringHostLive(host: host, reader: reader, scope: fixture.scope, active: true, isCurrent: () => true)),
    );
    expect(reader.selected, same(host));
    expect(find.textContaining('first sample'), findsOneWidget);
    final t = DateTime.utc(2026, 11, 18).millisecondsSinceEpoch;
    for (final (delta, cpu) in [(0, null), (2000, 21.5)]) {
      reader.samples.add(
        HostMetricsSample.fromJson({
          'timestampMs': t + delta,
          'cpuPercent': cpu,
          'uptimeSeconds': 551,
          'memoryUsed': 2048,
          'memoryTotal': 4096,
          'diskUsed': 1024,
          'diskTotal': 4096,
          'receivedPerSecond': 55,
          'sentPerSecond': 12,
        }),
      );
    }
    await settle(tester);
    expect(find.text('21.5 %'), findsOneWidget);
    await tester.tap(find.text('Values'));
    await settle(tester);
    expect(find.textContaining('21.5 %', findRichText: true), findsOneWidget);
    expect(reader.closed, isEmpty);
    await tester.pumpWidget(const SizedBox());
    expect(reader.closed, [reader.operation]);
    unawaited(reader.samples.close());
    await tester.pump();
  });

  testWidgets('existing source opens a range chart by metric name and can show its samples', (tester) async {
    final source = MonitoringTestService();
    await tester.binding.setSurfaceSize(const Size(1100, 900));
    addTearDown(() => tester.binding.setSurfaceSize(null));
    await tester.pumpWidget(
      _app(MonitoringMetricBrowser(source: monitoringFixture, service: source, isCurrent: () => true)),
    );
    expect(source.queries, 0);
    await tester.tap(find.text('All metrics'));
    await settle(tester);
    await tester.enterText(find.byKey(const ValueKey('monitoring-metric-search')), 'load1');
    await settle(tester);
    expect(find.text('up'), findsNothing);
    await tester.tap(find.text('node_load1'));
    await settle(tester);
    expect(source.last?.expression, 'node_load1');
    expect(source.last?.range, isTrue);
    expect(source.last!.end! - source.last!.start!, 3600);
    final result = find.byType(MonitoringResultView);
    expect(tester.widget<MonitoringResultView>(result).showChart, isTrue);
    expect(find.byKey(const ValueKey('monitoring-samples')), findsNothing);
    await tester.tap(find.text('Values'));
    await settle(tester);
    expect(tester.widget<MonitoringResultView>(result).showChart, isFalse);
    expect(find.byKey(const ValueKey('monitoring-samples')), findsOneWidget);
    expect(source.queries, 1);
    expect(tester.takeException(), isNull);
  });
}
