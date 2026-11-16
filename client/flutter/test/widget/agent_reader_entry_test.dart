import 'dart:async';

import 'package:consolecrypt/app/critical_notice_providers.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/runtime_notice_adapter.dart';
import 'package:consolecrypt/monitoring/agent_reader_dialog.dart';
import 'package:consolecrypt/monitoring/agent_reader_models.dart';
import 'package:consolecrypt/monitoring/agent_reader_port.dart';
import 'package:consolecrypt/monitoring/monitoring_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/monitoring_binding_fakes.dart';
import '../helpers/monitoring_fakes.dart';
import '../helpers/test_app.dart';
import '../support/agent_reader_fixture.dart' as f;

class ReaderService extends MonitoringTestService implements AgentReaderCapability {
  ReaderService(this.agentReaderPort);
  @override
  final AgentReaderNativePort agentReaderPort;
}

Future<(ProviderContainer, f.Port, RuntimeNoticeEventAdapter)> open(WidgetTester tester) async {
  final backend = testBackend(seed: false);
  addTearDown(backend.dispose);
  await backend.debugCreateUnlockedLocalProfile(name: 'Owned Reader entry');
  final profile = backend.services.profiles.currentProfiles.active!, events = RuntimeNoticeEventAdapter();
  addTearDown(events.dispose);
  events.acceptScope(
    originalEpoch: events.epoch,
    nativeProfile: {
      'id': profile.id.value,
      'active': true,
      'vault_state': 'unlocked',
      'notification_scope_id': f.device,
    },
  );
  final epoch = events.epoch,
      scope = MonitoringScope(
        profile.id,
        true,
        nativeScope: events.currentScope.scope,
        isCurrent: () => events.epoch == epoch,
      );
  final port = f.Port(f.selection(profileId: profile.id.value, nonce: f.device));
  final service = ReaderService(port), store = MonitoringBindingTestStore(initial: [f.source], rows: [f.binding]);
  await pumpApp(
    tester,
    backend,
    size: const Size(1400, 1000),
    overrides: [
      criticalRuntimeEventsProvider.overrideWithValue(events),
      monitoringScopeProvider.overrideWithValue(scope),
      monitoringBackendProvider.overrideWithValue(MonitoringTestBackend(store: store, service: service)),
      hostsProvider.overrideWith((ref) => Stream.value([f.host])),
      knownHostsProvider.overrideWith((ref) => Stream.value([f.known])),
      fileDialogServiceProvider.overrideWithValue(f.Files()),
    ],
  );
  final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
  container.read(routerProvider).go(AppRoutes.monitoring);
  await settle(tester);
  await tapKey(tester, 'monitoring-agent-reader-open');
  expect(find.byType(AgentReaderDialog), findsOneWidget);
  return (container, port, events);
}

Future<void> chooseIssuer(WidgetTester tester) async {
  final selectors = find.descendant(of: find.byType(AgentReaderDialog), matching: find.byType(GlassSelect<String?>));
  tester.widget<GlassSelect<String?>>(selectors.at(0)).onChanged!(f.bindingId);
  await settle(tester);
  tester.widget<GlassSelect<String?>>(selectors.at(1)).onChanged!(f.knownId);
  await settle(tester);
  tester
      .widget<GlassSelect<AgentReaderAction>>(
        find.descendant(of: find.byType(AgentReaderDialog), matching: find.byType(GlassSelect<AgentReaderAction>)),
      )
      .onChanged!(AgentReaderAction.issuerInit);
  await settle(tester);
}

void main() {
  testWidgets('actual Monitoring entry remains no-RPC on opening and route hide retires only owned Reader dialog', (
    tester,
  ) async {
    final (container, port, _) = await open(tester);
    expect(port.requests, isEmpty);
    expect(port.statuses, isEmpty);
    container.read(routerProvider).go(AppRoutes.terminal);
    await settle(tester);
    expect(find.byType(AgentReaderDialog), findsNothing);
    expect(port.requests, isEmpty);
    expect(tester.takeException(), isNull);
  });
  testWidgets('actual original Hub boundary removes pending Reader review and cancels late exact handle', (
    tester,
  ) async {
    final (_, port, events) = await open(tester);
    port.pending = Completer();
    await chooseIssuer(tester);
    final button = find.byKey(const ValueKey('agent-prepare'));
    expect(tester.widget<FilledButton>(button).onPressed, isNotNull);
    await tester.ensureVisible(button);
    await tester.pump();
    await tester.tap(button.hitTestable());
    await tester.pump();
    expect(port.handles, hasLength(1));
    final original = port.handles.single;
    events.boundary();
    await settle(tester);
    expect(find.byType(AgentReaderDialog), findsNothing);
    port.pending!.complete(original);
    await settle(tester);
    expect(original.cancels, 1);
    expect(original.confirms, 0);
    expect(port.requests, hasLength(1));
    expect(tester.takeException(), isNull);
  });
}
