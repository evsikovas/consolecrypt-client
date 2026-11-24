import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/settings.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/services/host_metrics_service.dart';
import 'package:consolecrypt/terminal/host_metrics_panel.dart';
import 'package:consolecrypt/terminal/terminal_status_bar.dart';
import 'package:flutter/gestures.dart' show PointerDeviceKind;
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
  testWidgets('narrow status bar keeps SSH latency visible and hides stale samples', (tester) async {
    final state = ValueNotifier(
      HostMetricsHistory(
        samples: [
          _sample(0, {'cpuPercent': 25.0, 'channelLatencyMs': 12, 'swapTotal': 0}),
        ],
      ),
    );
    addTearDown(state.dispose);
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(320, 210);
    addTearDown(tester.view.reset);
    await tester.pumpWidget(
      MaterialApp(
        supportedLocales: AppLocalizations.supportedLocales,
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        home: Scaffold(
          body: Align(
            alignment: Alignment.bottomCenter,
            child: TerminalStatusBar(history: state),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text('25%'), findsOneWidget);
    expect(tester.getRect(find.text('12 ms')).right, lessThanOrEqualTo(320));
    expect(find.byTooltip('Swap'), findsNothing);
    state.value = HostMetricsHistory(
      samples: state.value.samples,
      error: const AppException(AppErrorCode.serverUnreachable, 'test'),
    );
    await tester.pump();
    expect(find.text('12 ms'), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('only overflowed metrics open above the arrow on hover or tap', (tester) async {
    final state = ValueNotifier(
      HostMetricsHistory(
        samples: [
          _sample(0, {
            'cpuPercent': 25.0,
            'channelLatencyMs': 12,
            'swapTotal': 4096,
            'swapUsed': 1024,
            'memoryUsed': 1024,
            'memoryTotal': 4096,
            'receivedPerSecond': 100.0,
            'sentPerSecond': 70.0,
          }),
        ],
      ),
    );
    addTearDown(state.dispose);
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(210, 300);
    addTearDown(tester.view.reset);
    await tester.pumpWidget(
      MaterialApp(
        supportedLocales: AppLocalizations.supportedLocales,
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        home: Scaffold(
          body: Align(
            alignment: Alignment.bottomCenter,
            child: TerminalStatusBar(
              history: state,
              hidden: const {TerminalMetric.received},
              colors: const {TerminalMetric.cpu: 0xffcc00, TerminalMetric.swap: 0x88eeaa},
            ),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    final more = find.byKey(const ValueKey('terminal-metrics-more'));
    final popup = find.byKey(const ValueKey('terminal-hidden-metrics'));
    expect(more, findsOneWidget);
    expect(popup, findsNothing);
    expect(tester.widget<Text>(find.text('25%')).style?.color, const Color(0xffffcc00));
    final mouse = await tester.createGesture(kind: PointerDeviceKind.mouse);
    await mouse.addPointer();
    await mouse.moveTo(tester.getCenter(more));
    await tester.pump(const Duration(milliseconds: 160));
    await tester.pumpAndSettle();
    expect(tester.getRect(popup).bottom, lessThanOrEqualTo(tester.getRect(more).top + 1));
    expect(find.descendant(of: popup, matching: find.text('Swap')), findsOneWidget);
    expect(find.descendant(of: popup, matching: find.byIcon(Icons.arrow_downward)), findsNothing);
    expect(
      tester.widget<Icon>(find.descendant(of: popup, matching: find.byIcon(Icons.swap_horiz))).color,
      const Color(0xff88eeaa),
    );
    expect(find.descendant(of: popup, matching: find.text('CPU usage')), findsNothing);
    await mouse.moveTo(tester.getCenter(popup));
    await tester.pump(const Duration(milliseconds: 350));
    expect(popup, findsOneWidget);
    await mouse.moveTo(const Offset(2, 2));
    await tester.pump(const Duration(milliseconds: 350));
    await tester.pumpAndSettle();
    expect(popup, findsNothing);
    await mouse.removePointer();
    await tester.tap(more);
    await tester.pumpAndSettle();
    expect(popup, findsOneWidget);
    tester.view.physicalSize = const Size(1150, 300);
    await tester.pumpAndSettle();
    expect(more, findsNothing);
    expect(popup, findsNothing);
    expect(find.byTooltip('Swap'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

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
