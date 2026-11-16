import 'dart:async';

import 'package:consolecrypt/core/bridge/rust_agent_metrics_collector.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/monitoring/agent_metrics_collector_dialog.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../support/agent_metrics_fixture.dart' as f;

Future<void> tap(WidgetTester t, String key) async {
  final finder = find.byKey(ValueKey(key));
  await t.ensureVisible(finder);
  await t.tap(finder);
  await t.pump();
  await t.pump(const Duration(milliseconds: 80));
}

Future<ValueNotifier<bool>> open(WidgetTester t, f.NativeHandle handle) async {
  final original = ValueNotifier(true), calls = f.Calls(handle);
  await t.pumpWidget(
    MaterialApp(
      localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
      supportedLocales: AppLocalizations.supportedLocales,
      home: Scaffold(
        body: Builder(
          builder: (context) => TextButton(
            key: const ValueKey('open-metrics-clarity'),
            onPressed: () => showDialog<void>(
              context: context,
              builder: (_) => AgentMetricsCollectorDialog(
                port: RustAgentMetricsCollectorPort(calls: calls),
                scope: f.scope,
                record: f.metricRecord(),
                original: () => original.value,
                originalChanges: original,
                now: () => f.now,
              ),
            ),
            child: const Text('Open'),
          ),
        ),
      ),
    ),
  );
  await tap(t, 'open-metrics-clarity');
  return original;
}

void main() {
  testWidgets('review keeps human delivery and one cancel action, advanced reveals rawscope without anotherRPC', (
    t,
  ) async {
    final h = f.NativeHandle()..pendingJoin = Completer<String>();
    final original = await open(t, h);
    expect(h.calls, isEmpty);
    expect(find.byKey(const ValueKey('agent-metrics-close')), findsOneWidget);
    expect(find.byKey(const ValueKey('agent-metrics-cancel')), findsNothing);
    await tap(t, 'agent-metrics-prepare');
    final l = AppLocalizations.of(t.element(find.byType(AgentMetricsCollectorDialog)));
    expect(find.text(l.agentMetricsConfiguredReceiver), findsOneWidget);
    expect(find.text(l.agentMetricsReceiver), findsOneWidget);
    expect(l.agentMetricsReceiver.toLowerCase(), isNot(contains('read-only')));
    expect(find.text('metrics.collect'), findsNothing);
    expect(find.text('remote_write'), findsNothing);
    expect(find.byKey(const ValueKey('agent-metrics-close')), findsNothing);
    expect(find.text(l.agentMetricsCancelLocalRequest), findsOneWidget);
    await tap(t, 'agent-metrics-review-details');
    await t.pump(const Duration(milliseconds: 250));
    expect(find.text('metrics.collect'), findsOneWidget);
    expect(find.text('remote_write'), findsOneWidget);
    expect(h.calls, ['details']);
    await tap(t, 'agent-metrics-cancel');
    expect(h.calls, ['details', 'cancel', 'joined']);
    expect(find.byKey(const ValueKey('agent-metrics-close')), findsOneWidget);
    expect(find.byKey(const ValueKey('agent-metrics-cancel')), findsNothing);
    expect(find.byKey(const ValueKey('agent-metrics-approve')), findsNothing);
    await tap(t, 'agent-metrics-close');
    await t.pump(const Duration(milliseconds: 400));
    expect(find.byType(AgentMetricsCollectorDialog), findsNothing);
    expect(h.calls, isNot(contains('dispose')), reason: 'Closing window does not fabricate actual join');
    h.pendingJoin!.complete(f.cleanup(state: 'unknown'));
    await t.pump();
    await t.pump();
    expect(h.calls, ['details', 'cancel', 'joined', 'dispose']);
    expect(t.takeException(), isNull);
    await t.pumpWidget(const SizedBox());
    original.dispose();
  });
  testWidgets('authenticated stop remains separate from local cancel and never claimsStopped from request', (t) async {
    final h = f.NativeHandle();
    final original = await open(t, h);
    await tap(t, 'agent-metrics-prepare');
    await tap(t, 'agent-metrics-approve');
    final l = AppLocalizations.of(t.element(find.byType(AgentMetricsCollectorDialog)));
    expect(find.byKey(const ValueKey('agent-metrics-close')), findsNothing);
    expect(find.byKey(const ValueKey('agent-metrics-stop')), findsOneWidget);
    expect(find.text(l.agentMetricsCancelLocalRequest), findsOneWidget);
    await tap(t, 'agent-metrics-stop');
    expect(find.text(l.agentMetricsStopRequested), findsOneWidget);
    expect(find.text(l.agentMetricsStopped), findsNothing);
    expect(h.calls, ['details', 'approve', 'stop']);
    await tap(t, 'agent-metrics-cancel');
    await t.pump();
    await t.pump();
    expect(h.calls, ['details', 'approve', 'stop', 'cancel', 'joined', 'dispose']);
    expect(find.byKey(const ValueKey('agent-metrics-stop')), findsNothing);
    expect(find.byKey(const ValueKey('agent-metrics-cancel')), findsNothing);
    expect(find.byKey(const ValueKey('agent-metrics-close')), findsOneWidget);
    await tap(t, 'agent-metrics-close');
    await t.pump(const Duration(milliseconds: 400));
    expect(t.takeException(), isNull);
    await t.pumpWidget(const SizedBox());
    original.dispose();
  });
}
