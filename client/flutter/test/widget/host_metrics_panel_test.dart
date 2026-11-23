import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/services/host_metrics_service.dart';
import 'package:consolecrypt/terminal/host_metrics_panel.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

HostMetricsSample _sample(int seconds, Map<String, Object?> values) => HostMetricsSample.fromJson({
  'timestampMs': DateTime(2026, 11, 21, 12, 34, seconds).millisecondsSinceEpoch,
  'uptimeSeconds': 90062,
  ...values,
});

Widget _panel(ValueNotifier<HostMetricsHistory> history) => MaterialApp(
  locale: const Locale('en'),
  supportedLocales: AppLocalizations.supportedLocales,
  localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
  home: Scaffold(body: HostMetricsPanel(history: history)),
);

void main() {
  final samples = [
    _sample(10, {'memoryTotal': 4096, 'memoryUsed': 1024, 'diskTotal': 8192, 'diskUsed': 2048}),
    _sample(15, {
      'cpuPercent': 21.5,
      'cores': {for (var i = 0; i < 12; i++) 'cpu$i': 50.0},
      'interfaces': [
        {'name': 'eth0', 'rx': 1024.0, 'tx': 2048.0},
      ],
      'memoryTotal': 4096,
      'memoryUsed': 2048,
      'diskTotal': 8192,
      'diskUsed': 2048,
      'receivedPerSecond': 512.0,
      'sentPerSecond': 128.0,
      'os': 'Test Linux',
      'cpuModel': 'A64',
      'architecture': 'arm64',
      'logicalCpus': 8,
      'loadAverage': '0.15 / 0.12 / 0.10',
    }),
  ];
  testWidgets('updates charts, rates and host summary from the tab; narrow and wide layouts fit', (tester) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(1000, 650);
    addTearDown(tester.view.reset);
    final state = ValueNotifier(const HostMetricsHistory());
    addTearDown(state.dispose);
    await tester.pumpWidget(_panel(state));
    await tester.pump();
    expect(find.text('Reading the first sample over SSH…'), findsOneWidget);
    state.value = HostMetricsHistory(samples: samples);
    await tester.pumpAndSettle();
    expect(find.text('21.5%'), findsOneWidget);
    expect(find.text('50.0%'), findsOneWidget);
    expect(find.text('25.0%'), findsOneWidget);
    expect(find.textContaining('512 B/s'), findsOneWidget);
    expect(find.textContaining('128 B/s'), findsOneWidget);
    expect(find.text('A64 · arm64 · 8 logical CPUs'), findsOneWidget);
    expect(find.text('1 d 1 h 1 min'), findsOneWidget);
    expect(find.text('Updated at 12:34:15'), findsOneWidget);
    final core = find.byTooltip('cpu0 · 50.0%');
    final fill = find.descendant(of: core, matching: find.byType(FractionallySizedBox));
    expect(tester.widget<FractionallySizedBox>(fill).heightFactor, .5);
    final wideWidth = tester.getSize(core).width;
    expect(tester.getTopLeft(find.byTooltip('cpu11 · 50.0%')).dy, tester.getTopLeft(core).dy);
    expect(find.text('eth0'), findsOneWidget);
    final cpu = find.text('CPU usage'), ram = find.text('Memory used');
    expect(tester.getTopLeft(ram).dy, greaterThan(tester.getBottomLeft(cpu).dy));
    tester.view.physicalSize = const Size(265, 390);
    await tester.pumpAndSettle();
    expect(tester.getTopLeft(ram).dy, greaterThan(tester.getBottomLeft(cpu).dy));
    expect(tester.getTopLeft(cpu).dx, tester.getTopLeft(ram).dx);
    expect(tester.getSize(core).width, lessThan(wideWidth));
    expect(tester.getTopLeft(find.byTooltip('cpu11 · 50.0%')).dy, greaterThan(tester.getTopLeft(core).dy));
    await tester.ensureVisible(find.text('Test Linux'));
    expect(tester.takeException(), isNull);
  });

  testWidgets('shows a localized failure with the last good sample and switches cleanly between tabs', (tester) async {
    const error = AppException(
      AppErrorCode.unsupported,
      'private-native-detail',
      args: {'field': 'host_metrics', 'rule': 'host_metrics_linux_only'},
    );
    final first = ValueNotifier(HostMetricsHistory(samples: samples, error: error));
    final second = ValueNotifier(const HostMetricsHistory(error: error));
    addTearDown(first.dispose);
    addTearDown(second.dispose);
    await tester.pumpWidget(_panel(first));
    await tester.pumpAndSettle();
    expect(find.text('This view reads live metrics from Linux hosts.'), findsOneWidget);
    expect(find.text('private-native-detail'), findsNothing);
    expect(find.text('21.5%'), findsOneWidget);
    await tester.pumpWidget(_panel(second));
    await tester.pumpAndSettle();
    expect(find.text('21.5%'), findsNothing);
    expect(find.text('Test Linux'), findsNothing);
    expect(find.byType(LinearProgressIndicator), findsNothing);
    second.value = HostMetricsHistory(
      samples: [
        _sample(22, {'os': 'Other Linux'}),
      ],
    );
    await tester.pumpAndSettle();
    expect(find.text('This view reads live metrics from Linux hosts.'), findsNothing);
    expect(find.text('Other Linux'), findsOneWidget);
    expect(find.text('—'), findsWidgets);
    expect(tester.takeException(), isNull);
  });
}
