import 'dart:convert';

import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/monitoring/agent_metrics_collector_dialog.dart';
import 'package:consolecrypt/monitoring/agent_metrics_runtime_review.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../support/agent_metrics_fixture.dart' as f;
import '../support/agent_metrics_runtime_v2_fixture.dart' as v2;
import 'agent_metrics_collector_test.dart' show host, tap;

void main() {
  testWidgets(
    'disk-v2 review displays distinct software metadata and disk residence without volatile-only or runtime-success claim',
    (t) async {
      final handle = f.NativeHandle()..detailsReply = jsonEncode(v2.diskCollectorView(arm: true));
      final calls = f.Calls(handle), original = ValueNotifier(true);
      await t.pumpWidget(host(calls, original));
      expect(calls.requests, isEmpty);
      await tap(t, 'agent-metrics-prepare');
      final l = AppLocalizations.of(t.element(find.byType(AgentMetricsCollectorDialog)));
      expect(find.byKey(const ValueKey('agent-metrics-runtime-disk-metadata')), findsOneWidget);
      expect(find.text(l.agentMetricsRuntimeDiskMetadata), findsOneWidget);
      expect(find.text(l.agentMetricsRuntimeVolatileOnly), findsNothing);
      expect(find.text(l.agentMetricsDiskDisclosure), findsOneWidget);
      expect(find.byKey(const ValueKey('agent-metrics-disk-buffer')), findsOneWidget);
      expect(find.text(l.agentMetricsRunning), findsNothing);
      expect(find.text('prometheusHardmode 3.16.0 / linux-aarch64'), findsOneWidget);
      expect(handle.calls, ['details']);
      expect(calls.requests, hasLength(1));
      await t.pumpWidget(const SizedBox());
      await t.pump();
      expect(handle.calls, containsAllInOrder(['cancel', 'joined', 'dispose']));
      original.dispose();
    },
  );

  testWidgets('explicit ARM review shows volatile-only software metadata without starting or claiming running', (
    t,
  ) async {
    final handle = f.NativeHandle()..detailsReply = jsonEncode(v2.collectorView());
    final calls = f.Calls(handle), original = ValueNotifier(true);
    await t.pumpWidget(host(calls, original));
    expect(calls.requests, isEmpty);
    await tap(t, 'agent-metrics-prepare');
    final l = AppLocalizations.of(t.element(find.byType(AgentMetricsCollectorDialog)));
    expect(find.byType(AgentMetricsRuntimeReview), findsOneWidget);
    expect(find.text(l.agentMetricsRuntimeVolatileOnly), findsOneWidget);
    expect(find.text(l.agentMetricsReview), findsOneWidget);
    expect(find.text(l.agentMetricsRunning), findsNothing);
    expect(find.text('vmagent v1.156.0 / linux-aarch64'), findsOneWidget);
    expect(find.byKey(const ValueKey('agent-metrics-disk-buffer')), findsNothing);
    expect(find.text(v2.runtime()['binarySha256'] as String), findsNothing);
    await tap(t, 'agent-metrics-runtime-details');
    await t.pump(const Duration(milliseconds: 250));
    expect(find.text(v2.runtime()['binarySha256'] as String), findsOneWidget);
    expect(find.text(l.agentMetricsRuntimeMetadataOnly), findsOneWidget);
    expect(handle.calls, ['details']);
    expect(calls.requests, hasLength(1));
    await t.pumpWidget(const SizedBox());
    await t.pump();
    original.dispose();
  });

  testWidgets('RU500 enlarged review keeps original consent controls and persistent-unsupported message reachable', (
    t,
  ) async {
    t.view.physicalSize = const Size(500, 1000);
    t.view.devicePixelRatio = 1;
    addTearDown(t.view.reset);
    final handle = f.NativeHandle()..detailsReply = jsonEncode(v2.collectorView());
    final calls = f.Calls(handle), original = ValueNotifier(true);
    await t.pumpWidget(host(calls, original, locale: const Locale('ru'), scale: 1.5));
    await tap(t, 'agent-metrics-prepare');
    final l = AppLocalizations.of(t.element(find.byType(AgentMetricsCollectorDialog)));
    await t.ensureVisible(find.byKey(const ValueKey('agent-metrics-runtime-volatile-only')));
    expect(find.text(l.agentMetricsRuntimeVolatileOnly), findsOneWidget);
    await tap(t, 'agent-metrics-runtime-details');
    await t.pump(const Duration(milliseconds: 250));
    await t.ensureVisible(find.byKey(const ValueKey('agent-metrics-approve')));
    expect(handle.calls, ['details']);
    expect(t.takeException(), isNull);
    await t.pumpWidget(const SizedBox());
    await t.pump();
    original.dispose();
  });

  testWidgets('retiring original ARM review removes its metadata and joins same original handle without approval', (
    t,
  ) async {
    final handle = f.NativeHandle()..detailsReply = jsonEncode(v2.collectorView());
    final calls = f.Calls(handle), original = ValueNotifier(true);
    await t.pumpWidget(host(calls, original));
    await tap(t, 'agent-metrics-prepare');
    original.value = false;
    await t.pump();
    await t.pump();
    expect(find.byType(AgentMetricsRuntimeReview), findsNothing);
    expect(find.byKey(const ValueKey('agent-metrics-approve')), findsNothing);
    expect(handle.calls, ['details', 'cancel', 'joined', 'dispose']);
    await t.pumpWidget(const SizedBox());
    original.dispose();
  });
}
