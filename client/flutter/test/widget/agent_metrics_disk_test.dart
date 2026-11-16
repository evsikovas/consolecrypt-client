import 'dart:convert';

import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/monitoring/agent_metrics_collector_dialog.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../support/agent_metrics_disk_fixture.dart' as disk;
import '../support/agent_metrics_fixture.dart' as f;
import 'agent_metrics_collector_test.dart' as ui;

void main() {
  for (final locale in [const Locale('en'), const Locale('ru')]) {
    testWidgets(
      'disk review $locale shows permission and residence separately without RAM loss or automatic recovery claim',
      (t) async {
        t.view.physicalSize = const Size(500, 1000);
        t.view.devicePixelRatio = 1;
        addTearDown(t.view.reset);
        final h = f.NativeHandle()..detailsReply = jsonEncode(disk.diskView()),
            calls = f.Calls(f.NativeHandle()),
            original = ValueNotifier(true);
        calls.current = h;
        await t.pumpWidget(ui.host(calls, original, locale: locale, scale: 1.5));
        await t.pumpAndSettle();
        expect(calls.requests, isEmpty);
        await ui.tap(t, 'agent-metrics-prepare');
        final l = AppLocalizations.of(t.element(find.byType(AgentMetricsCollectorDialog)));
        expect(find.text(l.agentMetricsDiskDisclosure), findsOneWidget);
        expect(find.text(l.agentMetricsDisclosure), findsNothing);
        expect(find.text(l.agentMetricsDuration), findsOneWidget);
        expect(find.byKey(const ValueKey('agent-metrics-disk-buffer')), findsOneWidget);
        expect(find.text('metrics.collect'), findsNothing);
        await ui.tap(t, 'agent-metrics-review-details');
        await t.pump(const Duration(milliseconds: 250));
        expect(find.text('metrics.collect'), findsOneWidget);
        expect(h.calls, ['details']);
        expect(find.text('http://127.0.0.1:9090/api/v1/write'), findsOneWidget);
        await ui.tap(t, 'agent-metrics-disclosure-details');
        expect(find.text(l.agentMetricsDiskDetails), findsOneWidget);
        expect(h.calls, ['details']);
        expect(calls.requests.length, 1);
        expect(t.takeException(), isNull);
        await t.pumpWidget(const SizedBox());
        await t.pump();
        expect(h.calls, contains('cancel'));
        original.dispose();
      },
    );
  }
}
