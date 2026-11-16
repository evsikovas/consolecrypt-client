import 'dart:convert';

import 'package:consolecrypt/core/bridge/rust_agent_collector_service_revocation.dart';
import 'package:consolecrypt/core/bridge/rust_agent_collector_service_status.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_dialog.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_revocation_dialog.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../support/agent_collector_service_flow_fixture.dart' as f;
import '../support/agent_collector_service_revocation_fixture.dart' as r;
import '../unit/agent_collector_service_revocation_test.dart' as u;
import '../unit/agent_collector_service_status_test.dart' show statusWire;
import 'agent_collector_service_status_test.dart' show host, tap;

void main() {
  testWidgets(
    'targeted revoke needs explicit unchecked consent; lost remote reply shows localblock without remote success or retry',
    (t) async {
      final target = u.target(),
          h = r.RevokeNative(target)..confirmError = StateError('owned fixture lost reply'),
          original = ValueNotifier(true);
      await t.pumpWidget(
        host(
          AgentCollectorServiceRevocationDialog(
            port: u.port(h),
            target: target,
            original: () => original.value,
            originalChanges: original,
          ),
        ),
      );
      expect(h.calls, isEmpty);
      await tap(t, 'collector-service-revoke-prepare');
      final l = AppLocalizations.of(t.element(find.byType(AgentCollectorServiceRevocationDialog)));
      expect(t.widget<FilledButton>(find.byKey(const ValueKey('collector-service-revoke-confirm'))).onPressed, isNull);
      await tap(t, 'collector-service-revoke-consent');
      await tap(t, 'collector-service-revoke-confirm');
      expect(find.text(l.agentCollectorServiceRevokeLocalBlocked), findsOneWidget);
      expect(find.text(l.agentCollectorServiceRevokeUnknown), findsOneWidget);
      expect(find.text(l.agentCollectorServiceRemoteRevoked), findsNothing);
      expect(find.byKey(const ValueKey('collector-service-revoke-confirm')), findsNothing);
      expect(h.calls.where((v) => v == 'confirmRevocation').length, 1);
      await t.pumpWidget(const SizedBox());
      original.dispose();
    },
  );
  testWidgets(
    'saved-service revoke entry holds exact target; closing makes status stale with explicit reread only and no newpolicy',
    (t) async {
      final target = u.target(),
          h = r.RevokeNative(target),
          base = f.Calls(f.NativeHandle()),
          original = ValueNotifier(true);
      var reads = 0, prepares = 0;
      final status = RustAgentCollectorServiceWithStatus(
        original: base.port,
        readJson: (_) async {
          reads++;
          return jsonEncode(statusWire());
        },
      );
      final port = RustAgentCollectorServiceWithRevocation(
        original: status,
        status: status,
        prepareOriginal: (_) async {
          prepares++;
          return h;
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
      await tap(t, 'collector-service-status');
      expect(reads, 1);
      await tap(t, 'collector-service-revoke');
      await t.pumpAndSettle();
      expect(prepares, 0);
      await tap(t, 'collector-service-revoke-prepare');
      expect(prepares, 1);
      await tap(t, 'collector-service-revoke-cancel');
      await tap(t, 'collector-service-revoke-close');
      await t.pumpAndSettle();
      final l = AppLocalizations.of(t.element(find.byType(AgentCollectorServiceDialog)));
      expect(find.text(l.agentCollectorServiceSavedStale), findsOneWidget);
      expect(reads, 1);
      expect(find.byKey(const ValueKey('collector-service-prepare')), findsNothing);
      expect(find.byKey(const ValueKey('collector-service-revoke')), findsNothing);
      await tap(t, 'collector-service-status');
      expect(reads, 2);
      expect(base.prepareRequests, isEmpty);
      expect(h.calls, ['details', 'cancel', 'joined', 'dispose']);
      await t.pumpWidget(const SizedBox());
      original.dispose();
    },
  );
}
