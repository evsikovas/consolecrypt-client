import 'dart:convert';

import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_review.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_view.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../support/agent_association_fixture.dart' as a;
import '../support/agent_collector_service_fixture.dart' as f;
import '../support/agent_metrics_fixture.dart' as m;

Widget host(Widget child, {bool ru = false}) => MaterialApp(
  locale: ru ? const Locale('ru') : const Locale('en'),
  localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
  supportedLocales: AppLocalizations.supportedLocales,
  home: Scaffold(body: child),
);
void main() {
  testWidgets('recorded configuration shows unknown runtime and distinct24h permission/2h disk with no fake actions', (
    t,
  ) async {
    final view = parseAgentCollectorServiceView(
      jsonEncode(f.serviceWire(phase: 'configurationRecorded')),
      a.scope,
      m.metricRecord(),
    );
    await t.pumpWidget(host(SingleChildScrollView(child: AgentCollectorServiceReview(view: view))));
    final l = AppLocalizations.of(t.element(find.byType(AgentCollectorServiceReview)));
    expect(find.text(l.agentCollectorServiceRecorded), findsOneWidget);
    expect(find.text(l.agentCollectorServiceRuntimeUnknown), findsOneWidget);
    expect(find.text(l.agentCollectorServiceSummary), findsOneWidget);
    expect(find.text(l.agentCollectorServiceDisk), findsOneWidget);
    expect(find.text(l.agentCollectorServiceReboot), findsOneWidget);
    expect(find.byType(TextButton), findsNothing);
    await t.tap(find.byKey(const ValueKey('collector-service-technical')));
    await t.pump();
    await t.pump(const Duration(milliseconds: 250));
    final bootField = find.byWidgetPredicate(
      (widget) =>
          widget is Column &&
          widget.children.length == 2 &&
          widget.children.first is Text &&
          (widget.children.first as Text).data == l.agentCollectorServiceBootCreated,
    );
    final bootValue = find.descendant(of: bootField, matching: find.text(view.bootStamp!.createdBoottimeNs));
    expect(bootField, findsOneWidget);
    await t.ensureVisible(bootValue);
    expect(bootValue, findsOneWidget);
    await t.pumpWidget(const SizedBox());
  });
  testWidgets(
    'RU500 readonly modal retains all risks without overflow; original replacement hides old service metadata',
    (t) async {
      await t.binding.setSurfaceSize(const Size(500, 720));
      addTearDown(() => t.binding.setSurfaceSize(null));
      final original = ValueNotifier(true),
          view = parseAgentCollectorServiceView(jsonEncode(f.serviceWire()), a.scope, m.metricRecord());
      await t.pumpWidget(
        host(
          MediaQuery(
            data: const MediaQueryData(textScaler: TextScaler.linear(1.5)),
            child: AgentCollectorServiceViewDialog(
              view: view,
              original: () => original.value,
              originalChanges: original,
            ),
          ),
          ru: true,
        ),
      );
      final l = AppLocalizations.of(t.element(find.byType(AgentCollectorServiceViewDialog)));
      await t.ensureVisible(find.byKey(const ValueKey('collector-service-close')));
      await t.pump();
      expect(t.takeException(), isNull);
      expect(find.text(l.agentCollectorServiceRuntimeUnknown), findsOneWidget);
      original.value = false;
      await t.pump();
      expect(find.text(l.agentCollectorServiceOriginalChanged), findsOneWidget);
      expect(find.byType(AgentCollectorServiceReview), findsNothing);
      original.value = true;
      await t.pump();
      expect(find.byType(AgentCollectorServiceReview), findsNothing);
      await t.pumpWidget(const SizedBox());
      original.dispose();
    },
  );
}
