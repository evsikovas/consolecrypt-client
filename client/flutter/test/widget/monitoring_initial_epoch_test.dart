import 'dart:async';

import 'package:consolecrypt/app/app.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/core/services/runtime_notice_adapter.dart';
import 'package:consolecrypt/monitoring/metrics_ai_native_transport.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

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
      throw StateError('No external provider call');
    }
    return {};
  }
}

void main() {
  testWidgets(
    'original initial prepare is rejected when hub epoch crosses reopen before UI unlocked stage catches up',
    (tester) async {
      final backend = testBackend(), pending = _PendingAi(), events = RuntimeNoticeEventAdapter();
      addTearDown(backend.dispose);
      addTearDown(events.dispose);
      await backend.debugCreateUnlockedLocalProfile(name: 'Owned epoch profile');
      final profile = backend.services.profiles.currentProfiles.active!.id.value;
      await backend.services.ai.saveProvider(
        AiProviderConfig.draft(AiProviderKind.openaiCompatible)
            .copyWith(name: 'Epoch AI provider', baseUrl: 'https://synthetic.invalid', chatModel: 'owned'),
      );
      Map<String, dynamic> wire(String nonce) => {
        'id': profile,
        'active': true,
        'vault_state': 'unlocked',
        'notification_scope_id': nonce,
      };
      events.acceptScope(originalEpoch: events.epoch, nativeProfile: wire('11111111-1111-4111-8111-111111111111'));
      final s = backend.services;
      final services = AppServices(
        profiles: s.profiles,
        auth: s.auth,
        vault: s.vault,
        inventory: s.inventory,
        terminal: s.terminal,
        sftp: s.sftp,
        tunnels: s.tunnels,
        snippets: s.snippets,
        ai: s.ai,
        devices: s.devices,
        sync: s.sync,
        settings: s.settings,
        backups: s.backups,
        files: s.files,
        runtimeNotices: events,
        monitoring: MonitoringTestBackend(service: pending),
      );
      tester.view.physicalSize = const Size(1400, 1050);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);
      await tester.pumpWidget(
        ProviderScope(
          overrides: [
            appServicesProvider.overrideWithValue(services),
            // Models the real boundary-before-refresh window: the existing hub has
            // already fenced the unlock, while derived appStage is still unlocked.
            appStageProvider.overrideWithValue(AppStage.unlocked),
          ],
          child: const ConsoleCryptApp(),
        ),
      );
      await settle(tester);
      final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
      container.read(routerProvider).go(AppRoutes.monitoring);
      await settle(tester);
      // Source editing and custom PromQL are now explicitly in Advanced.
      await tapKey(tester, 'monitoring-view-toggle');
      await tapKey(tester, 'monitoring-auto-enabled');
      await tapKey(tester, 'monitoring-query');
      final original = container.read(monitoringSessionProvider)!;
      final originalScope = container.read(monitoringScopeProvider);
      await tapKey(tester, 'monitoring-review-ai');
      await tester.tap(find.text('Epoch AI provider'));
      await settle(tester);
      await tapKey(tester, 'metrics-ai-sample-0-0');
      await enterKey(tester, 'metrics-ai-question', 'Owned explicit context');
      await tester.tap(find.byKey(const ValueKey('metrics-ai-primary')));
      await tester.pump();
      expect(pending.prepares, 1);
      // Original-emitter boundary is synchronous. No widget/appStage pump occurs
      // between it and the simulated reply from a newly captured Native unlock.
      events.boundary();
      expect(originalScope.current, isFalse, reason: 'Synchronous boundary must deny before stream/UI microtasks');
      events.acceptScope(originalEpoch: events.epoch, nativeProfile: wire('22222222-2222-4222-8222-222222222222'));
      pending.pendingPrepare.complete({
        'version': 1,
        'ticket': 'late-new-native-unlock',
        'previewText': 'LATE NEW SESSION MUST NOT BE SHOWN',
        'external': true,
        'expiresUtc': DateTime.now().toUtc().add(const Duration(minutes: 1)).toIso8601String(),
      });
      await settle(tester);
      expect(find.text('LATE NEW SESSION MUST NOT BE SHOWN'), findsNothing);
      expect(find.byKey(const ValueKey('metrics-ai-exact-preview')), findsNothing);
      expect(pending.sends, 0);
      expect(container.read(monitoringSessionProvider), isNot(same(original)));
    },
  );
}
