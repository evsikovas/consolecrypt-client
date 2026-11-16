import 'package:consolecrypt/core/bridge/rust_agent_metrics_collector.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/monitoring/agent_metrics_collector_dialog.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../support/agent_metrics_fixture.dart' as f;

Widget host(f.Calls calls, ValueNotifier<bool> original, {Locale locale = const Locale('en'), double scale = 1}) =>
    MaterialApp(
      locale: locale,
      supportedLocales: AppLocalizations.supportedLocales,
      localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
      builder: (context, child) => MediaQuery(
        data: MediaQuery.of(context).copyWith(textScaler: TextScaler.linear(scale)),
        child: child!,
      ),
      home: Scaffold(
        body: AgentMetricsCollectorDialog(
          port: RustAgentMetricsCollectorPort(calls: calls),
          scope: f.scope,
          record: f.metricRecord(),
          original: () => original.value,
          originalChanges: original,
          now: () => f.now,
        ),
      ),
    );
Future<void> tap(WidgetTester t, String key) async {
  final finder = find.byKey(ValueKey(key));
  await t.ensureVisible(finder);
  await t.tap(finder);
  await t.pump();
  await t.pump(const Duration(milliseconds: 50));
}

void main() {
  testWidgets('compact RU review keeps duration, scope and receiver readable; expanding identity makes no RPC', (
    t,
  ) async {
    t.view.physicalSize = const Size(500, 1000);
    t.view.devicePixelRatio = 1;
    addTearDown(t.view.reset);
    final calls = f.Calls(f.NativeHandle()), original = ValueNotifier(true);
    await t.pumpWidget(host(calls, original, locale: const Locale('ru'), scale: 1.5));
    await t.pumpAndSettle();
    await tap(t, 'agent-metrics-prepare');
    final l = AppLocalizations.of(t.element(find.byType(AgentMetricsCollectorDialog)));
    expect(find.text(l.agentMetricsDuration), findsOneWidget);
    expect(find.text('metrics.collect'), findsNothing);
    expect(find.text('http://127.0.0.1:9090/api/v1/write'), findsOneWidget);
    expect(find.text('a' * 64), findsNothing);
    await tap(t, 'agent-metrics-review-details');
    expect(find.text('metrics.collect'), findsOneWidget);
    expect(find.text('a' * 64), findsOneWidget);
    expect(calls.current.calls, ['details']);
    expect(calls.requests.length, 1);
    expect(t.takeException(), isNull);
    await t.pumpWidget(const SizedBox());
    await t.pump();
    original.dispose();
  });
  testWidgets('mount is inert; review then explicit approval and stop preserve truthful ACK state', (t) async {
    final h = f.NativeHandle(), calls = f.Calls(f.NativeHandle()), original = ValueNotifier(true);
    calls.current = h;
    await t.pumpWidget(host(calls, original));
    await t.pumpAndSettle();
    expect(calls.requests, isEmpty);
    await tap(t, 'agent-metrics-prepare');
    expect(h.calls, ['details']);
    expect(find.byKey(const ValueKey('agent-metrics-approve')), findsOneWidget);
    await tap(t, 'agent-metrics-approve');
    expect(h.calls.where((x) => x == 'approve').length, 1);
    expect(find.byKey(const ValueKey('agent-metrics-stop')), findsOneWidget);
    await tap(t, 'agent-metrics-stop');
    expect(find.byKey(const ValueKey('agent-metrics-stop-requested')), findsOneWidget);
    expect(find.byKey(const ValueKey('agent-metrics-cleanup')), findsNothing);
    await t.pumpWidget(const SizedBox());
    await t.pump();
    expect(h.calls, contains('cancel'));
    expect(h.calls.last, 'dispose');
    original.dispose();
  });
  testWidgets('original notice retirement hides prior metadata and joins same handle', (t) async {
    final h = f.NativeHandle(), calls = f.Calls(f.NativeHandle()), original = ValueNotifier(true);
    calls.current = h;
    await t.pumpWidget(host(calls, original));
    await t.pumpAndSettle();
    await tap(t, 'agent-metrics-prepare');
    original.value = false;
    await t.pump();
    await t.pump();
    expect(find.byKey(const ValueKey('agent-metrics-approve')), findsNothing);
    expect(h.calls, contains('cancel'));
    expect(h.calls.last, 'dispose');
    expect(calls.requests.length, 1);
    await t.pumpWidget(const SizedBox());
    original.dispose();
  });
  testWidgets('Russian flow shows unknown storage before explicit prepare and original volatile loss afterward', (
    t,
  ) async {
    final calls = f.Calls(f.NativeHandle()), original = ValueNotifier(true);
    await t.pumpWidget(host(calls, original, locale: const Locale('ru')));
    await t.pumpAndSettle();
    final context = t.element(find.byType(AgentMetricsCollectorDialog));
    final l = AppLocalizations.of(context);
    expect(find.text(l.agentMetricsModePending), findsOneWidget);
    expect(find.text(l.agentMetricsDisclosure), findsNothing);
    expect(calls.requests, isEmpty);
    await tap(t, 'agent-metrics-prepare');
    expect(find.text(l.agentMetricsDisclosure), findsOneWidget);
    expect(t.takeException(), isNull);
    await t.pumpWidget(const SizedBox());
    original.dispose();
  });
}
