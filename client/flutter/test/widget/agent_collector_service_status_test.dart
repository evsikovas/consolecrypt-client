import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/bridge/rust_agent_collector_service_status.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_dialog.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_status_review.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../support/agent_collector_service_flow_fixture.dart' as f;
import '../unit/agent_collector_service_status_test.dart' show statusWire;

Widget host(Widget child) => MaterialApp(
  localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
  supportedLocales: AppLocalizations.supportedLocales,
  home: Scaffold(body: child),
);
Future<void> tap(WidgetTester t, String key) async {
  final finder = find.byKey(ValueKey(key));
  await t.ensureVisible(finder);
  await t.tap(finder);
  await t.pump();
  await t.pump(const Duration(milliseconds: 80));
}

void main() {
  testWidgets(
    'saved signed observation preserves local Unknown, disables freshconfiguration, and never claims running or stopping',
    (t) async {
      final calls = f.Calls(f.NativeHandle()), original = ValueNotifier(true);
      var reads = 0;
      final port = RustAgentCollectorServiceWithStatus(
        original: calls.port,
        readJson: (raw) async {
          reads++;
          return jsonEncode(statusWire(revoked: true));
        },
      );
      await t.pumpWidget(
        host(
          AgentCollectorServiceDialog(
            port: port,
            scope: f.scope,
            record: f.record,
            original: () => original.value,
            originalChanges: original,
          ),
        ),
      );
      expect(reads, 0);
      await tap(t, 'collector-service-read');
      expect(find.byKey(const ValueKey('collector-service-prepare')), findsOneWidget);
      await tap(t, 'collector-service-status');
      final l = AppLocalizations.of(t.element(find.byType(AgentCollectorServiceDialog)));
      expect(find.byType(AgentCollectorServiceStatusReview), findsOneWidget);
      expect(find.text(l.agentCollectorServiceLocalUnknown), findsOneWidget);
      expect(find.text(l.agentCollectorServiceRemoteRevoked), findsOneWidget);
      expect(find.text(l.agentCollectorServiceRuntimeUnknown), findsOneWidget);
      expect(find.byKey(const ValueKey('collector-service-prepare')), findsNothing);
      expect(calls.prepareRequests, isEmpty);
      await t.pump(const Duration(minutes: 2));
      expect(reads, 1);
      expect(calls.prepareRequests, isEmpty);
      await t.pumpWidget(const SizedBox());
      original.dispose();
    },
  );
  testWidgets(
    'pending status reply from retired original scope is hidden without adopting metadata or starting a review',
    (t) async {
      final calls = f.Calls(f.NativeHandle()), original = ValueNotifier(true), pending = Completer<String>();
      var reads = 0;
      final port = RustAgentCollectorServiceWithStatus(
        original: calls.port,
        readJson: (raw) {
          reads++;
          return pending.future;
        },
      );
      await t.pumpWidget(
        host(
          AgentCollectorServiceDialog(
            port: port,
            scope: f.scope,
            record: f.record,
            original: () => original.value,
            originalChanges: original,
          ),
        ),
      );
      await tap(t, 'collector-service-status');
      original.value = false;
      await t.pump();
      pending.complete(jsonEncode(statusWire()));
      await t.pump();
      expect(find.byType(AgentCollectorServiceStatusReview), findsNothing);
      expect(calls.prepareRequests, isEmpty);
      expect(reads, 1);
      original.value = true;
      await t.pump();
      expect(find.byType(AgentCollectorServiceStatusReview), findsNothing);
      await t.pumpWidget(const SizedBox());
      original.dispose();
    },
  );
}
