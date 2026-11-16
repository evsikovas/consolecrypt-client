import 'dart:async';

import 'package:consolecrypt/core/bridge/rust_agent_collector_service.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_dialog.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../support/agent_collector_service_flow_fixture.dart' as f;
import '../support/agent_collector_service_lifetime_fixture.dart' as x;
import 'agent_collector_service_status_test.dart' show host, tap;

Future<void> chooseRefs(WidgetTester t) async {
  await tap(t, 'collector-service-read');
  await tap(t, 'collector-service-credentials');
  for (final key in ['collector-service-scrape-credential', 'collector-service-receiver-credential']) {
    await tap(t, key);
    await t.pumpAndSettle();
    await t.tap(find.text('Owned record').last);
    await t.pumpAndSettle();
  }
}

void main() {
  testWidgets('default SingleStart remains selected; reading choices or reviews never silently opts into restarts', (t) async {
    final calls = x.LifetimeCalls(), original = ValueNotifier(true);
    await t.pumpWidget(host(AgentCollectorServiceDialog(
      port: calls.port, scope: f.scope, record: f.record,
      original: () => original.value, originalChanges: original,
    )));
    expect(calls.singleRequests, isEmpty);
    expect(calls.lifetimeRequests, isEmpty);
    expect(find.byKey(const ValueKey('collector-service-lifetime-selection')), findsNothing);
    await chooseRefs(t);
    expect(t.widget<CheckboxListTile>(find.byKey(const ValueKey('collector-service-lifetime-selection'))).value, isFalse);
    await tap(t, 'collector-service-prepare');
    expect(calls.singleRequests, hasLength(1));
    expect(calls.lifetimeRequests, isEmpty);
    expect(find.byKey(const ValueKey('collector-service-lifetime-review')), findsNothing);
    expect(calls.single.calls, ['details']);
    await t.pumpWidget(const SizedBox());
    await t.pump();
    original.dispose();
  });

  testWidgets('explicit lifetime prepare still requires fresh consent to actual purpose/expiry and one-use confirm', (t) async {
    final calls = x.LifetimeCalls(), original = ValueNotifier(true);
    await t.pumpWidget(host(AgentCollectorServiceDialog(
      port: calls.port, scope: f.scope, record: f.record,
      original: () => original.value, originalChanges: original,
    )));
    await chooseRefs(t);
    await tap(t, 'collector-service-lifetime-selection');
    expect(calls.lifetimeRequests, isEmpty);
    await tap(t, 'collector-service-prepare');
    expect(calls.singleRequests, isEmpty);
    expect(calls.lifetimeRequests, hasLength(1));
    expect(find.text(x.purpose), findsOneWidget);
    expect(find.text('2030-01-02T00:00:00.000Z'), findsNWidgets(2)); // Native fixture policy and retention.
    final l = AppLocalizations.of(t.element(find.byType(AgentCollectorServiceDialog)));
    expect(find.text(l.agentCollectorServiceLifetimeConsent), findsOneWidget);
    expect(t.widget<CheckboxListTile>(find.byKey(const ValueKey('collector-service-consent'))).value, isFalse);
    expect(t.widget<FilledButton>(find.byKey(const ValueKey('collector-service-confirm'))).onPressed, isNull);
    expect(calls.lifetime.calls, ['details']);
    await tap(t, 'collector-service-consent');
    await tap(t, 'collector-service-confirm');
    expect(calls.lifetime.calls, ['details', 'confirmConfiguration', 'joined', 'dispose']);
    expect(find.text(l.agentCollectorServiceRuntimeUnknown), findsOneWidget);
    expect(find.byKey(const ValueKey('collector-service-confirm')), findsNothing);
    await t.pumpWidget(const SizedBox());
    original.dispose();
  });

  testWidgets('retired original frame cancels late lifetime issuance and never offers confirmation', (t) async {
    final calls = x.LifetimeCalls(), original = ValueNotifier(true);
    calls.pendingPrepare = Completer<AgentCollectorServiceNativeHandle>();
    await t.pumpWidget(host(AgentCollectorServiceDialog(
      port: calls.port, scope: f.scope, record: f.record,
      original: () => original.value, originalChanges: original,
    )));
    await chooseRefs(t);
    await tap(t, 'collector-service-lifetime-selection');
    await tap(t, 'collector-service-prepare');
    original.value = false;
    await t.pump();
    calls.pendingPrepare!.complete(calls.lifetime);
    await t.pump();
    await t.pump();
    expect(calls.lifetime.calls, ['cancel', 'joined', 'dispose']);
    expect(find.byKey(const ValueKey('collector-service-confirm')), findsNothing);
    await t.pumpWidget(const SizedBox());
    original.dispose();
  });
}
