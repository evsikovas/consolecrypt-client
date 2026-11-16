import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/monitoring/agent_association_dialog.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_association_port.dart';
import 'package:consolecrypt/monitoring/agent_outbound_bootstrap_dialog.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../support/agent_association_fixture.dart' as association;
import '../support/agent_outbound_bootstrap_fixture.dart' as f;

Future<void> tap(WidgetTester t, String key) async {
  final finder = find.byKey(ValueKey(key));
  await t.ensureVisible(finder);
  await t.tap(finder);
  await t.pump();
  await t.pump(const Duration(milliseconds: 80));
}

Widget app(Widget child, {bool ru = false}) => MaterialApp(
  locale: ru ? const Locale('ru') : const Locale('en'),
  localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
  supportedLocales: AppLocalizations.supportedLocales,
  home: Scaffold(body: child),
);
Future<ValueNotifier<bool>> open(WidgetTester t, f.Calls calls, {bool ru = false}) async {
  final live = ValueNotifier(true);
  await t.pumpWidget(
    app(
      Builder(
        builder: (context) => TextButton(
          key: const ValueKey('open'),
          onPressed: () => showDialog<void>(
            context: context,
            builder: (_) => AgentOutboundBootstrapDialog(
              port: calls.port,
              scope: f.scope,
              record: f.record,
              original: () => live.value,
              originalChanges: live,
            ),
          ),
          child: const Text('Open'),
        ),
      ),
      ru: ru,
    ),
  );
  await tap(t, 'open');
  return live;
}

final class AssociationPort implements AgentAssociationPort {
  bool matchingHost = true;
  @override
  Future<AgentAssociationStatus> status(_) async => AgentAssociationStatus(
    scope: f.scope,
    clientIdentityState: AgentClientIdentityState.storedMetadata,
    hosts: matchingHost ? [association.selectedHost] : [],
    records: [f.record],
  );
  @override
  Future<AgentAssociationHandle> registerProbe(_, _) => throw StateError('Unexpected registration');
  @override
  Future<AgentClientIdentityHandle> prepareClientIdentity(_) => throw StateError('Unexpected identity generation');
}

void main() {
  testWidgets(
    'default off and review are inert until explicit consent; configured expiry is not future connection success',
    (t) async {
      final h = f.NativeHandle(), calls = f.Calls(h);
      final live = await open(t, calls);
      expect(calls.requests, isEmpty);
      expect(h.calls, isEmpty);
      final button = t.widget<TextButton>(find.byKey(const ValueKey('agent-outbound-prepare')));
      expect(button.onPressed, isNull);
      await tap(t, 'agent-outbound-opt-in');
      expect(calls.requests, isEmpty);
      await tap(t, 'agent-outbound-prepare');
      expect(calls.requests.single['ttlSeconds'], 3600);
      expect(find.byKey(const ValueKey('agent-outbound-close')), findsNothing);
      expect(h.calls, ['details']);
      final l = AppLocalizations.of(t.element(find.byType(AgentOutboundBootstrapDialog)));
      expect(find.text(l.agentOutboundCaps), findsOneWidget);
      expect(find.text(l.agentOutboundSeconds(3600)), findsOneWidget);
      await tap(t, 'agent-outbound-approve');
      expect(h.calls, ['details', 'approveOnce', 'cancel', 'joined', 'dispose']);
      expect(find.text(l.agentOutboundConfigured), findsOneWidget);
      expect(find.text(l.agentOutboundExpiry), findsOneWidget);
      expect(find.byKey(const ValueKey('agent-outbound-approve')), findsNothing);
      await tap(t, 'agent-outbound-close');
      await t.pumpWidget(const SizedBox());
      live.dispose();
    },
  );
  testWidgets('cancel during details reaches exact issued handle; stale scope and late data cannot publish review', (
    t,
  ) async {
    final h = f.NativeHandle()
      ..pendingDetails = Completer<String>()
      ..pendingJoin = Completer<String>();
    final calls = f.Calls(h);
    final live = await open(t, calls);
    await tap(t, 'agent-outbound-opt-in');
    await tap(t, 'agent-outbound-prepare');
    await tap(t, 'agent-outbound-cancel');
    expect(h.calls, ['details', 'cancel', 'joined']);
    live.value = false;
    await t.pump();
    h.pendingDetails!.complete(jsonEncode(f.wire()));
    await t.pump();
    expect(find.byKey(const ValueKey('agent-outbound-approve')), findsNothing);
    h.pendingJoin!.complete(f.cleanup('unknown'));
    await t.pump();
    final l = AppLocalizations.of(t.element(find.byType(AgentOutboundBootstrapDialog)));
    expect(find.text(l.agentOutboundUnknown), findsOneWidget);
    expect(find.text(l.agentOutboundClosed), findsNothing);
    expect(calls.requests.length, 1);
    await tap(t, 'agent-outbound-close');
    await t.pumpWidget(const SizedBox());
    live.dispose();
  });
  testWidgets('RU500 review keeps finite registration and actions reachable at text scale1.5', (t) async {
    await t.binding.setSurfaceSize(const Size(500, 720));
    addTearDown(() => t.binding.setSurfaceSize(null));
    final calls = f.Calls(f.NativeHandle()), live = ValueNotifier(true);
    await t.pumpWidget(
      app(
        MediaQuery(
          data: const MediaQueryData(textScaler: TextScaler.linear(1.5)),
          child: AgentOutboundBootstrapDialog(
            port: calls.port,
            scope: f.scope,
            record: f.record,
            original: () => live.value,
            originalChanges: live,
          ),
        ),
        ru: true,
      ),
    );
    await tap(t, 'agent-outbound-opt-in');
    await tap(t, 'agent-outbound-prepare');
    await t.ensureVisible(find.byKey(const ValueKey('agent-outbound-approve')));
    await t.pump();
    expect(t.takeException(), isNull);
    expect(calls.requests.length, 1);
    await tap(t, 'agent-outbound-cancel');
    await t.pumpWidget(const SizedBox());
    live.dispose();
  });
  testWidgets(
    'existing association entry requires exact saved Host revision and explicit action, unsupported remains absent',
    (t) async {
      final port = AssociationPort(), calls = f.Calls(f.NativeHandle());
      Future<void> mount({bool supported = true}) async {
        await t.pumpWidget(
          app(
            AgentAssociationDialog(
              port: port,
              scope: f.scope,
              original: () => true,
              outboundPort: supported ? calls.port : null,
            ),
          ),
        );
        await tap(t, 'agent-association-status');
      }

      await mount();
      expect(calls.requests, isEmpty);
      final row = find.byKey(ValueKey('agent-outbound-row-${f.record.id}'));
      expect(row, findsOneWidget);
      await t.ensureVisible(row);
      await t.tap(row);
      await t.pump();
      await t.pump(const Duration(milliseconds: 100));
      expect(find.byType(AgentOutboundBootstrapDialog), findsOneWidget);
      expect(calls.requests, isEmpty);
      await tap(t, 'agent-outbound-close');
      await t.pumpWidget(const SizedBox());
      port.matchingHost = false;
      await mount();
      expect(find.byKey(ValueKey('agent-outbound-row-${f.record.id}')), findsNothing);
      await t.pumpWidget(const SizedBox());
      port.matchingHost = true;
      await mount(supported: false);
      expect(find.byKey(ValueKey('agent-outbound-row-${f.record.id}')), findsNothing);
      await t.pumpWidget(const SizedBox());
    },
  );
}
