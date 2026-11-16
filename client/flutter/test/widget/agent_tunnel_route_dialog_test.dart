import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/bridge/rust_agent_tunnel_route.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/monitoring/agent_association_dialog.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_association_port.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_route_dialog.dart';
import 'package:consolecrypt/src/rust/api/agent_tunnel_route.dart' as native;
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../support/agent_association_fixture.dart' as a;
import '../unit/agent_tunnel_route_test.dart' as f;

// Schema-shaped replies exercise the actual generated callback/opaque types.
// They prove UI sequencing, not a real SSH/TLS/Agent admission or delivery.
class Calls implements AgentTunnelRouteNativeCalls {
  final handle = f.NativeHandle();
  final requests = <Map<String, dynamic>>[];
  int audits = 0;
  Completer<native.AgentTunnelRouteHandle>? pending;
  Map<String, dynamic> auditReply = f.auditWire();
  Calls() {
    handle.detailsReply = reply();
    handle.approvalReply = reply(phase: 'listening');
    handle.stopReply = reply(phase: 'stopRequested');
  }
  String reply({String phase = 'routeReview', int remaining = 60000}) => jsonEncode(
    f.view(phase: phase, remaining: remaining)..['destination'] = {'protocol': 'tcp', 'ip': '127.0.0.1', 'port': 9090},
  );
  @override
  Future<native.AgentTunnelRouteHandle> prepare(String raw) {
    requests.add(jsonDecode(raw) as Map<String, dynamic>);
    return pending?.future ?? Future.value(handle);
  }

  @override
  Future<String> audit(String raw) async {
    audits++;
    return jsonEncode(auditReply);
  }
}

Widget host(
  Calls calls,
  ValueNotifier<bool> original, {
  Locale locale = const Locale('en'),
  double scale = 1,
  Duration Function()? elapsed,
}) => MaterialApp(
  locale: locale,
  supportedLocales: AppLocalizations.supportedLocales,
  localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
  builder: (context, child) => MediaQuery(
    data: MediaQuery.of(context).copyWith(textScaler: TextScaler.linear(scale)),
    child: child!,
  ),
  home: Scaffold(
    body: AgentTunnelRouteDialog(
      key: const ValueKey('original-dialog'),
      port: RustAgentTunnelRoutePort(calls: calls),
      scope: f.scope,
      record: f.intent().record,
      original: () => original.value,
      originalChanges: original,
      elapsedForReview: elapsed,
    ),
  ),
);
Future<void> tap(WidgetTester t, String key) async {
  final finder = find.byKey(ValueKey(key));
  await t.ensureVisible(finder);
  await t.tap(finder);
  await t.pump();
  await t.pump(const Duration(milliseconds: 100));
}

Future<void> finish(WidgetTester t) async {
  await t.pumpWidget(const SizedBox());
  await t.pump();
}

class AssociationPort implements AgentAssociationPort {
  final delegate = a.Port();
  final List<AgentAssociationRecord> records;
  final List<AgentAssociationHost> hosts;
  AssociationPort(this.records, {this.hosts = const []});
  @override
  Future<AgentAssociationStatus> status(RuntimeNoticeScope requested) async => AgentAssociationStatus(
    scope: f.scope,
    clientIdentityState: AgentClientIdentityState.storedMetadata,
    hosts: hosts,
    records: records,
  );
  @override
  Future<AgentAssociationHandle> registerProbe(RuntimeNoticeScope scope, AgentAssociationHost host) =>
      delegate.registerProbe(scope, host);
  @override
  Future<AgentClientIdentityHandle> prepareClientIdentity(RuntimeNoticeScope scope) =>
      delegate.prepareClientIdentity(scope);
}

void main() {
  testWidgets('opening route performs no RPC; explicit prepare freezes exact loopback choices then one approval', (
    t,
  ) async {
    final calls = Calls(), original = ValueNotifier(true);
    await t.pumpWidget(host(calls, original));
    expect(calls.requests, isEmpty);
    expect(calls.audits, 0);
    expect(calls.handle.calls, isEmpty);
    await tap(t, 'agent-tunnel-prepare');
    expect(calls.requests.single, {
      'version': 1,
      'profileId': f.profile,
      'notificationScopeId': f.scope.nonce,
      'associationId': f.association,
      'listenIp': '127.0.0.1',
      'listenPort': 0,
      'destinationIp': '127.0.0.1',
      'destinationPort': 9090,
      'ttlSeconds': 60,
      'idleSeconds': 30,
      'resumeOriginal': false,
    });
    expect(calls.handle.calls, ['details']);
    expect(find.byKey(const ValueKey('agent-tunnel-listen-port')), findsNothing);
    await tap(t, 'agent-tunnel-review-details');
    expect(find.text('b' * 64), findsOneWidget);
    expect(find.text('9007199254740993'), findsNothing); // host revision is paired with its ID.
    await tap(t, 'agent-tunnel-approve');
    expect(calls.handle.calls.where((s) => s == 'approve').length, 1);
    expect(find.byKey(const ValueKey('agent-tunnel-approve')), findsNothing);
    expect(find.text('Waiting for a local connection'), findsOneWidget);
    expect(find.byKey(const ValueKey('agent-tunnel-actor-admission')), findsNothing);
    await t.pump(const Duration(seconds: 2));
    expect(calls.handle.calls, ['details', 'approve']);
    await finish(t);
    expect(calls.handle.calls.where((s) => s == 'stop'), isEmpty);
  });
  testWidgets('invalid port or lifetime sends zero preparations', (t) async {
    final c = Calls(), o = ValueNotifier(true);
    await t.pumpWidget(host(c, o));
    await t.enterText(find.byKey(const ValueKey('agent-tunnel-idle')), '61');
    await tap(t, 'agent-tunnel-prepare');
    expect(c.requests, isEmpty);
    expect(find.textContaining('Choose valid loopback'), findsOneWidget);
    await finish(t);
  });
  testWidgets('explicit Stop is one request; local closure stays unknown until original joined response', (t) async {
    final c = Calls(), o = ValueNotifier(true);
    c.handle.pendingJoin = Completer();
    await t.pumpWidget(host(c, o));
    await tap(t, 'agent-tunnel-prepare');
    await tap(t, 'agent-tunnel-approve');
    await tap(t, 'agent-tunnel-stop');
    expect(c.handle.calls.where((s) => s == 'stop').length, 1);
    expect(find.text('Stop requested; completion is unknown'), findsOneWidget);
    expect(find.byKey(const ValueKey('agent-tunnel-stop')), findsNothing);
    expect(find.byKey(const ValueKey('agent-tunnel-closed')), findsNothing);
    await tap(t, 'agent-tunnel-observe-close');
    expect(c.handle.calls.where((s) => s == 'joined').length, 1);
    expect(find.byKey(const ValueKey('agent-tunnel-closed')), findsNothing);
    expect(c.handle.calls.contains('dispose'), false);
    c.handle.pendingJoin!.complete(f.cleanup(state: 'unknown'));
    await t.pump();
    await t.pump();
    expect(find.byKey(const ValueKey('agent-tunnel-unknown')), findsOneWidget);
    expect(find.byKey(const ValueKey('agent-tunnel-closed')), findsNothing);
    expect(c.handle.calls.where((s) => s == 'dispose').length, 1);
    await finish(t);
  });
  testWidgets('scope retirement during pending prepare cancels and joins exact late handle without publishing review', (
    t,
  ) async {
    final c = Calls()..pending = Completer();
    final o = ValueNotifier(true);
    await t.pumpWidget(host(c, o));
    await tap(t, 'agent-tunnel-prepare');
    o.value = false;
    await t.pump();
    c.pending!.complete(c.handle);
    await t.pump();
    await t.pump();
    expect(find.byKey(const ValueKey('agent-tunnel-approve')), findsNothing);
    expect(c.handle.calls, ['cancel', 'joined', 'dispose']);
    expect(c.requests.length, 1);
    await finish(t);
  });
  testWidgets('expiry uses local countdown and never polls, approves or sends authenticated Stop', (t) async {
    var elapsed = Duration.zero;
    final c = Calls(), o = ValueNotifier(true);
    c.handle.detailsReply = c.reply(remaining: 1000);
    await t.pumpWidget(host(c, o, elapsed: () => elapsed));
    await tap(t, 'agent-tunnel-prepare');
    elapsed = const Duration(seconds: 1);
    await t.pump(const Duration(seconds: 1));
    await t.pump();
    expect(find.byKey(const ValueKey('agent-tunnel-approve')), findsNothing);
    expect(c.handle.calls, ['details', 'cancel', 'joined', 'dispose']);
    expect(c.requests.length, 1);
    await finish(t);
  });
  testWidgets('local audit is explicit, filtered to selected association and paged without extra RPC', (t) async {
    final c = Calls(), o = ValueNotifier(true);
    final row = (f.auditWire()['rows'] as List).single as Map<String, dynamic>;
    c.auditReply['rows'] = [
      for (var i = 0; i < 10; i++)
        {
          ...row,
          'sequence': i + 1,
          'record': {
            ...(row['record'] as Map<String, dynamic>),
            'operationId': '60000000-0000-4000-8000-${(i + 1).toString().padLeft(12, '0')}',
            'associationId': i == 9 ? '70000000-0000-4000-8000-000000000007' : f.association,
          },
        },
    ];
    await t.pumpWidget(host(c, o));
    expect(c.audits, 0);
    await tap(t, 'agent-tunnel-audit-read');
    expect(c.audits, 1);
    expect(c.requests, isEmpty);
    expect(find.text('1 / 2'), findsOneWidget);
    await tap(t, 'agent-tunnel-audit-next');
    expect(find.text('2 / 2'), findsOneWidget);
    expect(c.audits, 1);
    expect(find.text('60000000-0000-4000-8000-000000000010'), findsNothing);
    await finish(t);
  });
  testWidgets(
    'original association row exposes tunnel only for eligible saved tunnel record, with no auto preparation',
    (t) async {
      final c = Calls(),
          row = f.intent().record,
          metrics = AgentAssociationRecord(
            associationId: '70000000-0000-4000-8000-000000000007',
            hostId: f.host,
            agentId: f.agent,
            phase: AgentAssociationPhase.associated,
            hostRevision: '1',
            certificateDigest: 'b' * 64,
            purpose: AgentAssociationPurpose.metrics,
          );
      await t.pumpWidget(
        MaterialApp(
          localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
          supportedLocales: AppLocalizations.supportedLocales,
          home: Scaffold(
            body: AgentAssociationDialog(
              port: AssociationPort(
                [row, metrics],
                hosts: [AgentAssociationHost(id: row.hostId, revision: row.hostRevision!, name: 'Owned fixture Host')],
              ),
              scope: f.scope,
              original: () => true,
              tunnelPort: RustAgentTunnelRoutePort(calls: c),
            ),
          ),
        ),
      );
      await tap(t, 'agent-association-status');
      expect(find.byKey(ValueKey('agent-tunnel-row-${row.id}')), findsOneWidget);
      expect(find.byKey(ValueKey('agent-tunnel-row-${metrics.id}')), findsNothing);
      await tap(t, 'agent-tunnel-row-${row.id}');
      expect(find.byKey(const ValueKey('agent-tunnel-dialog')), findsOneWidget);
      expect(c.requests, isEmpty);
      await tap(t, 'agent-tunnel-close');
      await t.pumpAndSettle();
      expect(find.byKey(const ValueKey('agent-tunnel-dialog')), findsNothing);
      await finish(t);
    },
  );
  testWidgets('missing or changed original Host disables tunnel entry and performs no route RPC', (t) async {
    final row = f.intent().record;
    for (final hosts in <List<AgentAssociationHost>>[
      [],
      [AgentAssociationHost(id: row.hostId, revision: '2', name: 'Owned fixture Host')],
    ]) {
      final c = Calls();
      await t.pumpWidget(
        MaterialApp(
          localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
          supportedLocales: AppLocalizations.supportedLocales,
          home: Scaffold(
            body: AgentAssociationDialog(
              port: AssociationPort([row], hosts: hosts),
              scope: f.scope,
              original: () => true,
              tunnelPort: RustAgentTunnelRoutePort(calls: c),
            ),
          ),
        ),
      );
      await tap(t, 'agent-association-status');
      final button = find.byKey(ValueKey('agent-tunnel-row-${row.id}'));
      expect(t.widget<OutlinedButton>(button).onPressed, isNull);
      expect(find.byKey(ValueKey('agent-tunnel-host-stale-${row.id}')), findsOneWidget);
      await t.ensureVisible(button);
      await t.tapAt(t.getCenter(button));
      await t.pump();
      expect(find.byKey(const ValueKey('agent-tunnel-dialog')), findsNothing);
      expect(c.requests, isEmpty);
      expect(c.audits, 0);
      expect(c.handle.calls, isEmpty);
      await finish(t);
    }
  });
  testWidgets('Russian compact review and live controls fit 500px at large text scale', (t) async {
    t.view.physicalSize = const Size(500, 1000);
    t.view.devicePixelRatio = 1;
    addTearDown(t.view.resetPhysicalSize);
    addTearDown(t.view.resetDevicePixelRatio);
    final c = Calls(), o = ValueNotifier(true);
    await t.pumpWidget(host(c, o, locale: const Locale('ru'), scale: 1.5));
    expect(t.takeException(), isNull);
    await tap(t, 'agent-tunnel-prepare');
    expect(t.takeException(), isNull);
    await tap(t, 'agent-tunnel-approve');
    expect(t.takeException(), isNull);
    await finish(t);
    expect(t.takeException(), isNull);
  });
}
