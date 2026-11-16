import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/bridge/rust_agent_collector_service.dart';
import 'package:consolecrypt/core/bridge/rust_agent_collector_service_credentials.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_dialog.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_review.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../support/agent_collector_service_credentials_fixture.dart' as c;
import '../support/agent_collector_service_flow_fixture.dart' as f;
import '../support/agent_collector_service_revocation_fixture.dart' as r;
import '../unit/agent_collector_service_revocation_test.dart' as u;
import 'agent_collector_service_status_test.dart' show host, tap;

void main() {
  testWidgets(
    'saved credentials load only explicitly, start unselected, and actual refs are visible before one-use approval',
    (t) async {
      final h = c.CredentialNative(), requests = <Map<String, dynamic>>[], original = ValueNotifier(true);
      var reads = 0;
      final base = RustAgentCollectorServicePort(
            prepareOriginal: (raw) async {
              requests.add(jsonDecode(raw) as Map<String, dynamic>);
              return h;
            },
            readCatalogJson: (_) async => f.catalogReply(),
          ),
          previous = u.port(r.RevokeNative(u.target()));
      final port = RustAgentCollectorServiceWithCredentials(
        original: base,
        status: previous,
        revocation: previous,
        readJson: (_) async {
          reads++;
          return jsonEncode(c.choicesWire());
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
      expect(reads, 0);
      await tap(t, 'collector-service-credentials');
      expect(reads, 1);
      expect(requests, isEmpty);
      for (final key in ['collector-service-scrape-credential', 'collector-service-receiver-credential']) {
        expect(t.widget<DropdownButtonFormField<String>>(find.byKey(ValueKey(key))).initialValue, '');
        await tap(t, key);
        await t.pumpAndSettle();
        await t.tap(find.text('Owned record').last);
        await t.pumpAndSettle();
      }
      await tap(t, 'collector-service-prepare');
      final l = AppLocalizations.of(t.element(find.byType(AgentCollectorServiceDialog)));
      expect(find.byType(AgentCollectorServiceReview), findsOneWidget);
      expect(find.text(l.agentCollectorServiceCredentialsReview), findsOneWidget);
      expect(find.text('${c.credentialId} / 7'), findsNWidgets(2));
      expect(requests.single['credentials'], hasLength(2));
      expect(h.calls, ['details']);
      expect(t.widget<FilledButton>(find.byKey(const ValueKey('collector-service-confirm'))).onPressed, isNull);
      await tap(t, 'collector-service-consent');
      await tap(t, 'collector-service-confirm');
      expect(h.calls, ['details', 'confirmConfiguration', 'joined', 'dispose']);
      await t.pumpWidget(const SizedBox());
      original.dispose();
    },
  );
  testWidgets('late credential choices after original retirement cannot select a ref or prepare a service', (t) async {
    final base = f.Calls(f.NativeHandle()),
        previous = u.port(r.RevokeNative(u.target())),
        original = ValueNotifier(true),
        pending = Completer<String>();
    var reads = 0;
    final port = RustAgentCollectorServiceWithCredentials(
      original: base.port,
      status: previous,
      revocation: previous,
      readJson: (_) {
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
    await tap(t, 'collector-service-read');
    await tap(t, 'collector-service-credentials');
    original.value = false;
    await t.pump();
    pending.complete(jsonEncode(c.choicesWire()));
    await t.pump();
    expect(find.byKey(const ValueKey('collector-service-scrape-credential')), findsNothing);
    expect(find.byKey(const ValueKey('collector-service-prepare')), findsNothing);
    expect(reads, 1);
    expect(base.prepareRequests, isEmpty);
    await t.pumpWidget(const SizedBox());
    original.dispose();
  });
}
