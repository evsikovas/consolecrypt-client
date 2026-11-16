import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/monitoring/agent_association_dialog.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_association_port.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_dialog.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../support/agent_association_fixture.dart' as a;
import '../support/agent_collector_service_fixture.dart' as v;
import '../support/agent_collector_service_flow_fixture.dart' as f;

/// Callback/Widget contract only; no actual Native/Node signature or FFI claim.
class AssociationPort extends Fake implements AgentAssociationPort {
  @override
  Future<AgentAssociationStatus> status(RuntimeNoticeScope scope) async => AgentAssociationStatus(
    scope: scope,
    clientIdentityState: AgentClientIdentityState.storedMetadata,
    hosts: [a.selectedHost],
    records: [f.record],
  );
}

Widget host(Widget child, {bool ru = false}) => MaterialApp(
  locale: ru ? const Locale('ru') : const Locale('en'),
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
    'actual existing Metrics row opens inert service flow; explicit catalog and consent precede configuration, not runtime',
    (t) async {
      final h = f.NativeHandle(), calls = f.Calls(h), association = AssociationPort();
      await t.pumpWidget(
        host(
          AgentAssociationDialog(
            port: association,
            scope: f.scope,
            original: () => true,
            collectorServicePort: calls.port,
          ),
        ),
      );
      await tap(t, 'agent-association-status');
      await tap(t, 'agent-collector-service-row-${f.record.id}');
      expect(calls.catalogRequests, isEmpty);
      expect(calls.prepareRequests, isEmpty);
      await tap(t, 'collector-service-read');
      expect(calls.catalogRequests.length, 1);
      expect(calls.prepareRequests, isEmpty);
      await tap(t, 'collector-service-prepare');
      expect(calls.prepareRequests.length, 1);
      expect(h.calls, ['details']);
      expect(t.widget<FilledButton>(find.byKey(const ValueKey('collector-service-confirm'))).onPressed, isNull);
      await tap(t, 'collector-service-consent');
      expect(h.calls, ['details']);
      await tap(t, 'collector-service-confirm');
      final l = AppLocalizations.of(t.element(find.byType(AgentCollectorServiceDialog)));
      expect(find.text(l.agentCollectorServiceRecorded), findsOneWidget);
      expect(find.text(l.agentCollectorServiceRuntimeUnknown), findsOneWidget);
      expect(find.text(l.agentCollectorServiceMayRemainActive), findsOneWidget);
      expect(h.calls, ['details', 'confirmConfiguration', 'joined', 'dispose']);
      expect(find.byKey(const ValueKey('collector-service-confirm')), findsNothing);
      await tap(t, 'collector-service-close');
      await t.pumpWidget(const SizedBox());
    },
  );
  testWidgets(
    'older unsupported service has no action; unavailable saved-service query never enables new configuration or autoretries',
    (t) async {
      final association = AssociationPort();
      await t.pumpWidget(host(AgentAssociationDialog(port: association, scope: f.scope, original: () => true)));
      await tap(t, 'agent-association-status');
      expect(find.byKey(ValueKey('agent-collector-service-row-${f.record.id}')), findsNothing);
      await t.pumpWidget(const SizedBox());
      final calls = f.Calls(f.NativeHandle())..catalogError = StateError('owned fixture unavailable saved state'),
          original = ValueNotifier(true);
      await t.pumpWidget(
        host(
          AgentCollectorServiceDialog(
            port: calls.port,
            scope: f.scope,
            record: f.record,
            original: () => original.value,
            originalChanges: original,
          ),
        ),
      );
      await tap(t, 'collector-service-read');
      final l = AppLocalizations.of(t.element(find.byType(AgentCollectorServiceDialog)));
      expect(find.text(l.agentCollectorServiceUnavailable), findsOneWidget);
      expect(find.byKey(const ValueKey('collector-service-prepare')), findsNothing);
      expect(calls.prepareRequests, isEmpty);
      await t.pump(const Duration(minutes: 2));
      expect(calls.catalogRequests.length, 1);
      expect(calls.prepareRequests, isEmpty);
      await t.pumpWidget(const SizedBox());
      original.dispose();
      final unknown = f.Calls(f.NativeHandle())..catalogOverride = f.catalogReply(cleanup: 'unknown'),
          second = ValueNotifier(true);
      await t.pumpWidget(
        host(
          AgentCollectorServiceDialog(
            port: unknown.port,
            scope: f.scope,
            record: f.record,
            original: () => second.value,
            originalChanges: second,
          ),
        ),
      );
      await tap(t, 'collector-service-read');
      final again = AppLocalizations.of(t.element(find.byType(AgentCollectorServiceDialog)));
      expect(find.text(again.agentCollectorServiceCatalogObserved), findsOneWidget);
      expect(find.text(again.agentCollectorServiceCleanupUnknown), findsOneWidget);
      expect(find.byKey(const ValueKey('collector-service-prepare')), findsNothing);
      expect(unknown.prepareRequests, isEmpty);
      await t.pumpWidget(const SizedBox());
      second.dispose();
    },
  );
  testWidgets(
    'RU500 service review keeps controls reachable and original retirement cancels pending details without late metadata',
    (t) async {
      await t.binding.setSurfaceSize(const Size(500, 720));
      addTearDown(() => t.binding.setSurfaceSize(null));
      final h = f.NativeHandle(), calls = f.Calls(h), original = ValueNotifier(true);
      await t.pumpWidget(
        host(
          MediaQuery(
            data: const MediaQueryData(textScaler: TextScaler.linear(1.5)),
            child: AgentCollectorServiceDialog(
              port: calls.port,
              scope: f.scope,
              record: f.record,
              original: () => original.value,
              originalChanges: original,
            ),
          ),
          ru: true,
        ),
      );
      await tap(t, 'collector-service-read');
      await tap(t, 'collector-service-prepare');
      await t.ensureVisible(find.byKey(const ValueKey('collector-service-confirm')));
      await t.pump();
      expect(t.takeException(), isNull);
      h.pendingDetails = Completer<String>();
      await tap(t, 'collector-service-refresh');
      original.value = false;
      await t.pump();
      expect(h.calls, contains('cancel'));
      expect(h.calls, contains('joined'));
      h.pendingDetails!.complete(jsonEncode(v.serviceWire()));
      await t.pump();
      final l = AppLocalizations.of(t.element(find.byType(AgentCollectorServiceDialog)));
      expect(find.text(l.agentCollectorServiceOriginalChanged), findsOneWidget);
      expect(find.byKey(const ValueKey('collector-service-confirm')), findsNothing);
      expect(h.calls, isNot(contains('confirmConfiguration')));
      expect(calls.prepareRequests.length, 1);
      original.value = true;
      await t.pump();
      expect(find.byKey(const ValueKey('collector-service-confirm')), findsNothing);
      await t.pumpWidget(const SizedBox());
      original.dispose();
    },
  );
}
