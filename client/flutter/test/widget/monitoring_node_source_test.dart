import 'dart:async';

import 'package:consolecrypt/app/critical_notice_providers.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/automation/automation_agent_installer.dart';
import 'package:consolecrypt/automation/automation_dependency_dialog.dart' show DependencyOriginalGuard;
import 'package:consolecrypt/automation/automation_monitoring_node.dart';
import 'package:consolecrypt/automation/automation_monitoring_node_dialog.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/runtime_notice_adapter.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';
import '../support/agent_installer_fixture.dart' as f;
import 'automation_exporter_template_test.dart' show Projects;

class Api extends Fake implements AutomationMonitoringNodeService, AutomationAgentInstallerService {
  int reads = 0;
  final requests = <AutomationJson>[];
  Completer<AutomationJson>? pending;
  @override
  Future<AutomationJson> agentInstallerContext(AutomationJson frame) async {
    reads++;
    return f.context();
  }

  @override
  Future<AutomationJson> createMonitoringNodeProject(AutomationJson r) async {
    requests.add(r);
    return pending?.future ?? (f.created(r)..['claimedRelease'] = null);
  }
}

class Harness {
  final api = Api(), events = RuntimeNoticeEventAdapter(), live = ValueNotifier(true);
  final scope = const AutomationScope(f.profileId, true);
  AutomationProject? selected;
  Future<void> pump(WidgetTester t, {Locale locale = const Locale('en'), double scale = 1}) async {
    events.acceptScope(
      originalEpoch: events.epoch,
      nativeProfile: {'id': f.profileId, 'active': true, 'vault_state': 'unlocked', 'notification_scope_id': f.nonce},
    );
    final backend = testBackend();
    addTearDown(backend.dispose);
    addTearDown(events.dispose);
    addTearDown(live.dispose);
    await t.pumpWidget(
      ProviderScope(
        overrides: [
          appServicesProvider.overrideWithValue(backend.services),
          automationScopeProvider.overrideWithValue(scope),
          criticalRuntimeEventsProvider.overrideWithValue(events),
        ],
        child: MaterialApp(
          locale: locale,
          supportedLocales: AppLocalizations.supportedLocales,
          localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
          builder: (context, child) => MediaQuery(
            data: MediaQuery.of(context).copyWith(textScaler: TextScaler.linear(scale)),
            child: child!,
          ),
          home: Scaffold(
            body: Builder(
              builder: (context) => TextButton(
                key: const ValueKey('open-monitoring-node'),
                onPressed: () async {
                  selected = await showGlassDialog<AutomationProject>(
                    context,
                    builder: (_) => DependencyOriginalGuard(
                      original: live,
                      builder: (_) => AutomationMonitoringNodeDialog(
                        scope: scope,
                        service: api,
                        contextService: api,
                        originalCurrent: () => live.value,
                        originalLive: live,
                      ),
                    ),
                  );
                },
                child: const Text('Open'),
              ),
            ),
          ),
        ),
      ),
    );
    await tapKey(t, 'open-monitoring-node');
  }

  Future<void> fill(WidgetTester t) async {
    await t.enterText(find.byKey(const ValueKey('monitoring-node-name')), 'Owned monitoring node');
    await t.testTextInput.receiveAction(TextInputAction.done);
    await settle(t);
    await tapKey(t, 'monitoring-node-host');
    await t.tap(find.text('Existing SSH source target').last);
    await settle(t);
  }
}

class ParentService extends Projects implements AutomationAgentInstallerService, AutomationMonitoringNodeService {
  int contextReads = 0, creates = 0;
  @override
  Future<AutomationJson> agentInstallerContext(AutomationJson frame) async {
    contextReads++;
    return {...f.context(), 'profileId': frame['profileId'], 'notificationScopeId': frame['notificationScopeId']};
  }

  @override
  Future<AutomationJson> createMonitoringNodeProject(AutomationJson request) async {
    creates++;
    throw StateError('Cancelled source must not create');
  }
}

void main() {
  testWidgets('explicit source form shows prerequisites and creates one bound project; open is separate from run', (
    t,
  ) async {
    final h = Harness();
    await h.pump(t);
    expect(h.api.reads, 1);
    expect(h.api.requests, isEmpty);
    expect(find.textContaining('already mounted separate volume'), findsOneWidget);
    await h.fill(t);
    await tapKey(t, 'monitoring-node-create');
    expect(h.api.requests.length, 1);
    expect(h.api.requests.single['operation'], 'install');
    expect(h.api.requests.single['expectedHostRevision'], '9007199254740993');
    expect(find.byKey(const ValueKey('monitoring-node-open')), findsOneWidget);
    expect(h.selected, isNull);
    expect(find.textContaining('running version and metrics availability have not been checked'), findsOneWidget);
    await tapKey(t, 'monitoring-node-open');
    expect(h.selected?.id, f.projectId);
  });
  testWidgets('restore is an explicit distinct source choice with configuration-only limits', (t) async {
    final h = Harness();
    await h.pump(t);
    await h.fill(t);
    await tapKey(t, 'monitoring-node-operation');
    await t.tap(find.text('Prepare configuration recovery').last);
    await settle(t);
    await tapKey(t, 'monitoring-node-create');
    expect(h.api.requests.single['operation'], 'restore');
    expect(h.api.requests.single.containsKey('approved'), false);
    expect((h.api.requests.single['plan'] as Map)['receiver_enabled'], false);
  });
  testWidgets('insufficient RAM stays editable and sends zero create requests', (t) async {
    final h = Harness();
    await h.pump(t);
    await h.fill(t);
    await t.enterText(find.byKey(const ValueKey('monitoring-node-memory')), '1536');
    await t.testTextInput.receiveAction(TextInputAction.done);
    await tapKey(t, 'monitoring-node-create');
    expect(h.api.requests, isEmpty);
    expect(find.textContaining('Check the plan bounds'), findsOneWidget);
  });
  testWidgets('original scope retirement prevents late created project publication and automatic retry', (t) async {
    final h = Harness();
    await h.pump(t);
    await h.fill(t);
    h.api.pending = Completer();
    await tapKey(t, 'monitoring-node-create');
    h.live.value = false;
    await settle(t);
    h.api.pending!.complete(f.created(h.api.requests.single)..['claimedRelease'] = null);
    await settle(t);
    expect(h.selected, isNull);
    expect(h.api.requests.length, 1);
    expect(find.byKey(const ValueKey('monitoring-node-open')), findsNothing);
  });
  testWidgets('cancel performs zero creations and Russian prerequisites fit 500px enlarged layout', (t) async {
    t.view.physicalSize = const Size(500, 1000);
    t.view.devicePixelRatio = 1;
    addTearDown(t.view.reset);
    final h = Harness();
    await h.pump(t, locale: const Locale('ru'), scale: 1.5);
    expect(t.takeException(), isNull);
    await tapKey(t, 'monitoring-node-close');
    expect(h.api.requests, isEmpty);
    expect(h.selected, isNull);
    expect(t.takeException(), isNull);
  });
  testWidgets('actual Automation menu reaches monitoring-node form; cancel creates no project or run', (t) async {
    final service = ParentService(), events = RuntimeNoticeEventAdapter(), backend = testBackend();
    addTearDown(backend.dispose);
    addTearDown(events.dispose);
    await backend.debugCreateUnlockedLocalProfile(name: 'Owned monitoring source');
    await pumpApp(
      t,
      backend,
      overrides: [
        automationServiceProvider.overrideWithValue(service),
        criticalRuntimeEventsProvider.overrideWithValue(events),
      ],
    );
    final container = ProviderScope.containerOf(t.element(find.byType(Navigator).first)),
        profile = container.read(activeProfileProvider)!;
    events.acceptScope(
      originalEpoch: events.epoch,
      nativeProfile: {
        'id': profile.id.value,
        'active': true,
        'vault_state': 'unlocked',
        'notification_scope_id': 'owned monitoring source parent',
      },
    );
    container.read(routerProvider).go(AppRoutes.automation);
    await settle(t);
    expect(service.contextReads, 0);
    await tapKey(t, 'automation-project-actions');
    await tapKey(t, 'automation-monitoring-node-template');
    expect(find.byKey(const ValueKey('monitoring-node-dialog')), findsOneWidget);
    expect(service.contextReads, 1);
    await tapKey(t, 'monitoring-node-close');
    expect(service.creates, 0);
    expect(service.projectsCreated, isEmpty);
    expect(service.expected, isEmpty);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
}
