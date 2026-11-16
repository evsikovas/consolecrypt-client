import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/app/app.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/bridge/rust_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/monitoring/metrics_ai_native_transport.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_providers.dart';
import 'package:consolecrypt/src/rust/api/app.dart';
import 'package:consolecrypt/src/rust/frb_generated.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/global_event_fence_fakes.dart';
import '../helpers/monitoring_fakes.dart';
import '../helpers/test_app.dart';

class _PendingAi extends MonitoringTestService implements MetricsAiNativeCapability {
  final pendingPrepare = Completer<Json>();
  int prepares = 0, sends = 0;
  @override
  bool get metricsAiSupported => true;
  @override
  Future<Json> metricsAiRequest(MetricsAiNativeOperation operation, Json request) async {
    if (operation == MetricsAiNativeOperation.prepare) {
      prepares++;
      return pendingPrepare.future;
    }
    if (operation == MetricsAiNativeOperation.send) {
      sends++;
      throw StateError('No actual provider call is allowed');
    }
    return {};
  }
}

void main() {
  for (final cause in ['lagged', 'error', 'done', 'malformed', 'domainGap', 'availabilityOnly']) {
    testWidgets(
      'real existing hub $cause while initial prepare is pending ${cause == 'domainGap' ? 'preserves unrelated preview' : 'retires original preview before fresh profile completion'}',
      (tester) async {
        final appBackend = testBackend(), pending = _PendingAi(), native = GlobalEventNativeFixture();
        await appBackend.debugCreateUnlockedLocalProfile(name: 'Owned global event profile');
        final profile = appBackend.services.profiles.currentProfiles.active!.id.value;
        native.current = globalEventProfiles(profile, 'unlocked');
        RustLib.initMock(api: native);
        final hub = RustBackend(
          const CoreInfo(
            dataDir: '/unused-owned-fixture',
            platform: 'windows',
            coreVersion: 'fixture',
            alreadyInitialized: false,
          ),
        );
        addTearDown(() async {
          await hub.dispose();
          await native.events.close();
          await native.prompts.close();
          RustLib.dispose();
          await appBackend.dispose();
        });
        await hub.start();
        await flushGlobalEvents();
        await appBackend.services.ai.saveProvider(
          AiProviderConfig.draft(AiProviderKind.openaiCompatible)
              .copyWith(name: 'Owned pending provider', baseUrl: 'https://synthetic.invalid', chatModel: 'owned'),
        );
        final b = appBackend.services;
        final services = AppServices(
          profiles: b.profiles,
          auth: b.auth,
          vault: b.vault,
          inventory: b.inventory,
          terminal: b.terminal,
          sftp: b.sftp,
          tunnels: b.tunnels,
          snippets: b.snippets,
          ai: b.ai,
          devices: b.devices,
          sync: b.sync,
          settings: b.settings,
          backups: b.backups,
          files: b.files,
          runtimeNotices: hub.runtimeNoticeEvents,
          monitoring: MonitoringTestBackend(service: pending),
        );
        tester.view.physicalSize = const Size(1400, 1050);
        tester.view.devicePixelRatio = 1;
        addTearDown(tester.view.reset);
        await tester.pumpWidget(
          ProviderScope(
            overrides: [
              appServicesProvider.overrideWithValue(services),
              // This is the causal boundary-before-profiles-refresh interval. The
              // existing hub must retire synchronously even while appStage lags.
              appStageProvider.overrideWithValue(AppStage.unlocked),
            ],
            child: const ConsoleCryptApp(),
          ),
        );
        await settle(tester);
        final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
        container.read(routerProvider).go(AppRoutes.monitoring);
        await settle(tester);
        await tapKey(tester, 'monitoring-auto-enabled');
        await tapKey(tester, 'monitoring-query');
        final original = container.read(monitoringSessionProvider)!, scope = container.read(monitoringScopeProvider);
        await tapKey(tester, 'monitoring-review-ai');
        await tester.tap(find.text('Owned pending provider'));
        await settle(tester);
        await tapKey(tester, 'metrics-ai-sample-0-0');
        await enterKey(tester, 'metrics-ai-question', 'Owned synthetic selected context');
        await tester.tap(find.byKey(const ValueKey('metrics-ai-primary')));
        await tester.pump();
        expect(pending.prepares, 1);
        Completer<String>? profileRefresh;
        addTearDown(() {
          final ownedRefresh = profileRefresh;
          if (ownedRefresh != null && !ownedRefresh.isCompleted) {
            ownedRefresh.complete(globalEventProfiles(profile, 'unlocked', globalEventScopeB));
          }
        });
        switch (cause) {
          case 'lagged':
            profileRefresh = native.pendingProfiles = Completer<String>();
            native.events.add(jsonEncode({'type': 'lagged'}));
          case 'error':
            native.events.addError(StateError('unrendered-private-sentinel'));
          case 'done':
            await tester.runAsync(() => native.events.close());
          case 'malformed':
            native.events.add('{broken');
          case 'availabilityOnly':
            final epoch = hub.runtimeNoticeEvents.epoch;
            hub.runtimeNoticeEvents.streamLost(closed: true);
            expect(hub.runtimeNoticeEvents.epoch, epoch);
          case 'domainGap':
            native.events.add(
              jsonEncode({'type': 'critical_notice_gap', 'profile_id': profile, 'scope_id': globalEventScopeA}),
            );
        }
        // No UI pump or fresh Native profile completion before this check.
        final originalStillCurrentBeforePump = scope.current;
        pending.pendingPrepare.complete({
          'version': 1,
          'ticket': 'owned-pending-preview',
          'previewText': 'EXACT OWNED PREVIEW AFTER EVENT',
          'external': true,
          'expiresUtc': DateTime.now().toUtc().add(const Duration(minutes: 1)).toIso8601String(),
        });
        await settle(tester);
        if (cause == 'domainGap') {
          expect(find.byKey(const ValueKey('metrics-ai-exact-preview')), findsOneWidget);
          expect(find.text('EXACT OWNED PREVIEW AFTER EVENT'), findsOneWidget);
          expect(container.read(monitoringSessionProvider), same(original));
        } else {
          expect(find.text('EXACT OWNED PREVIEW AFTER EVENT'), findsNothing);
          expect(find.byKey(const ValueKey('metrics-ai-exact-preview')), findsNothing);
          expect(container.read(monitoringSessionProvider), isNot(same(original)));
          if (cause == 'done' || cause == 'availabilityOnly') {
            expect(container.read(monitoringScopeProvider).current, isFalse);
            expect(container.read(monitoringSessionProvider), isNull);
            expect(find.byKey(const ValueKey('monitoring-review-ai')), findsNothing);
          }
        }
        expect(
          originalStillCurrentBeforePump,
          cause == 'domainGap',
          reason: 'Captured immediately at the global callback, before any UI pump or refresh completion',
        );
        if (profileRefresh != null) {
          profileRefresh.complete(globalEventProfiles(profile, 'unlocked', globalEventScopeB));
          await settle(tester);
          expect(find.text('EXACT OWNED PREVIEW AFTER EVENT'), findsNothing);
        }
        expect(native.eventSubscriptions, 1);
        expect(pending.sends, 0);
      },
    );
  }
}
