import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/bridge/rust_agent_tunnel_resume_preference.dart';
import 'package:consolecrypt/core/bridge/rust_agent_tunnel_route.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_route_dialog.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../unit/agent_tunnel_route_test.dart' as f;
import 'agent_tunnel_route_dialog_test.dart' as route;

class Harness {
  final calls = route.Calls(), original = ValueNotifier(true), reads = <Map<String, dynamic>>[];
  Completer<String>? pending;
  late final originalPort = RustAgentTunnelRoutePort(calls: calls);
  late final port = RustAgentTunnelRouteWithResumePreference(
    original: originalPort,
    readJson: (raw) async {
      final request = jsonDecode(raw) as Map<String, dynamic>;
      reads.add(request);
      return pending?.future ?? reply(request);
    },
  );
  String reply(Map<String, dynamic> request) => jsonEncode({
    'version': 1,
    'profileId': f.scope.profileId,
    'notificationScopeId': f.scope.nonce,
    'associationId': f.association,
    'destination': {'protocol': 'tcp', 'ip': request['destinationIp'], 'port': request['destinationPort']},
    'resumeOriginal': true,
  });
  Future<void> pump(WidgetTester t, {bool supported = true}) async {
    await t.pumpWidget(
      MaterialApp(
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        supportedLocales: AppLocalizations.supportedLocales,
        home: Scaffold(
          body: AgentTunnelRouteDialog(
            port: supported ? port : originalPort,
            scope: f.scope,
            record: f.intent().record,
            original: () => original.value,
            originalChanges: original,
          ),
        ),
      ),
    );
  }

  Future<void> finish(WidgetTester t) async {
    await t.pumpWidget(const SizedBox());
    await t.pump();
    original.dispose();
  }
}

Future<void> tap(WidgetTester t, String key) async {
  final finder = find.byKey(ValueKey(key));
  await t.ensureVisible(finder);
  await t.tap(finder);
  await t.pump();
  await t.pump(const Duration(milliseconds: 100));
}

void main() {
  for (final selected in [false, true]) {
    testWidgets('Native35 capability true keeps selected $selected separate and never claims recovery success', (
      t,
    ) async {
      final h = Harness();
      await h.pump(t);
      if (selected) await tap(t, 'agent-tunnel-resume-original');
      h.calls.handle.detailsReply = jsonEncode(
        jsonDecode(h.calls.reply()) as Map<String, dynamic>
          ..['resumeOriginalSignedOptIn'] = selected
          ..['resumeTransportImplemented'] = true,
      );
      await tap(t, 'agent-tunnel-prepare');
      final l = AppLocalizations.of(t.element(find.byType(AgentTunnelRouteDialog)));
      expect(h.calls.requests.single['resumeOriginal'], selected);
      expect(
        find.text(selected ? l.agentTunnelResumeTransportSelected : l.agentTunnelResumeTransportOff),
        findsOneWidget,
      );
      expect(find.text(l.agentTunnelResumeUnavailable), findsNothing);
      expect(h.calls.handle.calls, ['details']);
      expect(h.reads, isEmpty);
      await h.finish(t);
    });
  }

  testWidgets('saved read is explicit and never enables checkbox; manual choice waits for route review approval', (
    t,
  ) async {
    final h = Harness();
    await h.pump(t);
    expect(h.reads, isEmpty);
    expect(h.calls.requests, isEmpty);
    expect(t.widget<CheckboxListTile>(find.byKey(const ValueKey('agent-tunnel-resume-original'))).value, false);
    await tap(t, 'agent-tunnel-resume-read');
    expect(h.reads.length, 1);
    expect(h.calls.requests, isEmpty);
    expect(t.widget<CheckboxListTile>(find.byKey(const ValueKey('agent-tunnel-resume-original'))).value, false);
    await tap(t, 'agent-tunnel-resume-original');
    expect(h.reads.length, 1);
    expect(h.calls.requests, isEmpty);
    h.calls.handle.detailsReply = jsonEncode(
      jsonDecode(h.calls.reply()) as Map<String, dynamic>..['resumeOriginalSignedOptIn'] = true,
    );
    h.calls.handle.approvalReply = jsonEncode(
      jsonDecode(h.calls.reply(phase: 'listening')) as Map<String, dynamic>..['resumeOriginalSignedOptIn'] = true,
    );
    await tap(t, 'agent-tunnel-prepare');
    expect(h.calls.requests.single['resumeOriginal'], true);
    expect(h.calls.handle.calls, ['details']);
    final l = AppLocalizations.of(t.element(find.byType(AgentTunnelRouteDialog)));
    expect(find.text(l.agentTunnelResumeUnavailable), findsOneWidget);
    expect(find.text(l.agentTunnelResumeSelectedOn), findsOneWidget);
    await tap(t, 'agent-tunnel-approve');
    expect(h.calls.handle.calls.where((s) => s == 'approve').length, 1);
    expect(h.reads.length, 1);
    await h.finish(t);
  });
  testWidgets('destination change resets manual opt-in and discards late saved preference without another read', (
    t,
  ) async {
    final h = Harness();
    h.pending = Completer();
    await h.pump(t);
    await tap(t, 'agent-tunnel-resume-original');
    await tap(t, 'agent-tunnel-resume-read');
    await t.enterText(find.byKey(const ValueKey('agent-tunnel-destination-port')), '9091');
    await t.pump();
    expect(t.widget<CheckboxListTile>(find.byKey(const ValueKey('agent-tunnel-resume-original'))).value, false);
    expect(t.widget<OutlinedButton>(find.byKey(const ValueKey('agent-tunnel-resume-read'))).onPressed, isNull);
    h.pending!.complete(h.reply(h.reads.single));
    await t.pump();
    await t.pump();
    expect(find.byKey(const ValueKey('agent-tunnel-resume-saved')), findsNothing);
    expect(h.reads.length, 1);
    expect(h.calls.requests, isEmpty);
    await h.finish(t);
  });
  testWidgets('old backend keeps preference unsupported and off with no fake success', (t) async {
    final h = Harness();
    await h.pump(t, supported: false);
    expect(t.widget<CheckboxListTile>(find.byKey(const ValueKey('agent-tunnel-resume-original'))).onChanged, isNull);
    expect(t.widget<OutlinedButton>(find.byKey(const ValueKey('agent-tunnel-resume-read'))).onPressed, isNull);
    await tap(t, 'agent-tunnel-prepare');
    expect(h.calls.requests.single['resumeOriginal'], false);
    expect(h.reads, isEmpty);
    await h.finish(t);
  });
  testWidgets('original profile retirement suppresses a late read and never prepares a replacement route', (t) async {
    final h = Harness();
    h.pending = Completer();
    await h.pump(t);
    await tap(t, 'agent-tunnel-resume-read');
    h.original.value = false;
    await t.pump();
    h.pending!.complete(h.reply(h.reads.single));
    await t.pump();
    await t.pump();
    expect(find.byKey(const ValueKey('agent-tunnel-resume-saved')), findsNothing);
    expect(h.calls.requests, isEmpty);
    expect(h.reads.length, 1);
    await h.finish(t);
  });
}
