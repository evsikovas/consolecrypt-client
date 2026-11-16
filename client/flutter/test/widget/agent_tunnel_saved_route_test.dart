import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_route_controller.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_route_dialog.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_route_models.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../unit/agent_tunnel_route_test.dart' as f;
import '../unit/agent_tunnel_saved_route_test.dart' as s;

Future<void> tap(WidgetTester t, String key) async {
  final found = find.byKey(ValueKey(key));
  await t.ensureVisible(found); await t.tap(found); await t.pump();
  await t.pump(const Duration(milliseconds: 100));
}
Future<void> open(WidgetTester t, s.SavedCalls calls, ValueNotifier<bool> original) async {
  final record = s.savedIntent().record;
  await t.pumpWidget(MaterialApp(
    localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
    supportedLocales: AppLocalizations.supportedLocales,
    home: Scaffold(body: Builder(builder: (context) => TextButton(
      key: const ValueKey('show-tunnel'),
      onPressed: () => unawaited(showDialog<void>(context: context, barrierDismissible: false,
        builder: (_) => AgentTunnelRouteDialog(port: calls.port, scope: f.scope, record: record,
          original: () => original.value, originalChanges: original))),
      child: const Text('Open fixture'),
    ))),
  ));
  await tap(t, 'show-tunnel');
}
Future<void> end(WidgetTester t, ValueNotifier<bool> original) async {
  await t.pumpWidget(const SizedBox()); await t.pump(); original.dispose();
}
void main() {
  testWidgets('explicit opted lookup reuses original and only Leave action releases without cancelling', (t) async {
    final calls = s.SavedCalls(), original = ValueNotifier(true);
    await open(t, calls, original);
    expect(calls.requests, isEmpty); expect(calls.raw.calls, isEmpty);
    await tap(t, 'agent-tunnel-resume-original');
    expect(t.widget<OutlinedButton>(find.byKey(const ValueKey('agent-tunnel-prepare'))).onPressed, isNull);
    await tap(t, 'agent-tunnel-attach-saved');
    expect(calls.requests.keys, ['attach']); expect(calls.raw.calls, ['details']);
    expect(find.byKey(const ValueKey('agent-tunnel-approve')), findsNothing);
    await tap(t, 'agent-tunnel-leave-original'); await t.pumpAndSettle();
    expect(find.byKey(const ValueKey('agent-tunnel-dialog')), findsNothing);
    expect(calls.raw.calls, ['details', 'detach', 'dispose']);
    expect(calls.requests.keys, ['attach']);
    await end(t, original);
  });
  testWidgets('ordinary Close of an opted reused tunnel still cancels and observes original join', (t) async {
    final calls = s.SavedCalls(), original = ValueNotifier(true);
    await open(t, calls, original); await tap(t, 'agent-tunnel-resume-original');
    await tap(t, 'agent-tunnel-attach-saved'); await tap(t, 'agent-tunnel-close'); await t.pumpAndSettle();
    expect(calls.raw.calls, ['details', 'cancel', 'joined', 'dispose']);
    expect(find.byKey(const ValueKey('agent-tunnel-dialog')), findsNothing);
    await end(t, original);
  });
  testWidgets('lookup None visibly offers a separate explicit fresh review without automatic prepare', (t) async {
    final calls = s.SavedCalls(found: false), original = ValueNotifier(true);
    await open(t, calls, original); await tap(t, 'agent-tunnel-resume-original');
    await tap(t, 'agent-tunnel-attach-saved');
    expect(calls.requests.keys, ['attach']); expect(calls.raw.calls, isEmpty);
    expect(find.byKey(const ValueKey('agent-tunnel-original-missing')), findsOneWidget);
    expect(t.widget<OutlinedButton>(find.byKey(const ValueKey('agent-tunnel-prepare'))).onPressed, isNotNull);
    await tap(t, 'agent-tunnel-prepare');
    expect(calls.requests.keys, ['attach', 'prepare']); expect(calls.raw.calls, ['details']);
    expect(find.byKey(const ValueKey('agent-tunnel-approve')), findsOneWidget);
    expect(calls.raw.calls, isNot(contains('approve')));
    await tap(t, 'agent-tunnel-close'); await t.pumpAndSettle();
    await end(t, original);
  });
  testWidgets('validated detach restores full nondefault intent in same service and reopens only original without approval', (t) async {
    final calls = s.SavedCalls(), original = ValueNotifier(true), previous = calls.raw;
    final intent = AgentTunnelRouteIntent(record: s.savedIntent().record,
      listenIp: '::1', listenPort: 32123, destinationIp: '::1', destinationPort: 9191,
      ttlSeconds: 45, idleSeconds: 12, resumeOriginal: true);
    final view = s.savedView();
    view['clientBind'] = {'ip': '::1', 'port': 32123};
    view['destination'] = {'protocol': 'tcp', 'ip': '::1', 'port': 9191};
    view['boundAddress'] = '[::1]:32123'; view['grantExpiresUtc'] = '2030-01-01T00:00:45Z';
    view['remainingMs'] = 45000;
    (view['limits'] as Map<String, dynamic>)['maxTtlSeconds'] = 45;
    (view['limits'] as Map<String, dynamic>)['idleTimeoutSeconds'] = 12;
    previous.detailsReply = jsonEncode(view);
    calls.receiptOverride = {...s.detachWire(), 'destination': view['destination'],
      'boundAddress': view['boundAddress'], 'grantExpiresUtc': view['grantExpiresUtc'], 'remainingMs': 45000};
    final owner = AgentTunnelRouteController(port: calls.port, scope: f.scope, intent: intent,
      original: () => original.value, now: () => DateTime.utc(2030));
    await owner.attachSaved(); await owner.detachAutoOpen(); owner.dispose();
    expect(previous.calls, ['details', 'detach', 'dispose']);
    calls.requests.clear();
    calls.raw = f.NativeHandle()..detailsReply = jsonEncode(view); // A NEW issued UI handle, SAME synthetic original.
    await open(t, calls, original); await t.pumpAndSettle();
    expect(calls.requests.keys, ['attach']);
    expect(calls.requests['attach']!.single, intent.request(f.scope));
    expect(calls.raw.calls, ['details']);
    expect(find.byKey(const ValueKey('agent-tunnel-approve')), findsNothing);
    await tap(t, 'agent-tunnel-close'); await t.pumpAndSettle();
    expect(calls.raw.calls, ['details', 'cancel', 'joined', 'dispose']);
    expect(previous.calls, ['details', 'detach', 'dispose']);
    expect(calls.port.savedIntents.restore(f.scope, intent.record), isNull);
    await end(t, original);
  });
}
