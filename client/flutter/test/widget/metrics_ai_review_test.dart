import 'dart:async';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/monitoring/metrics_ai_native_transport.dart';
import 'package:consolecrypt/monitoring/metrics_ai_review_dialog.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_ui_proof.dart';
import '../helpers/monitoring_fakes.dart';
import '../helpers/test_app.dart';

class _AiMonitoring extends MonitoringTestService implements MetricsAiNativeCapability {
  @override
  bool metricsAiSupported = true;
  final calls = <(MetricsAiNativeOperation, Json)>[];
  Completer<Json>? preparing, sending;
  @override
  Future<Json> metricsAiRequest(MetricsAiNativeOperation operation, Json request) async {
    calls.add((operation, request));
    return switch (operation) {
      MetricsAiNativeOperation.prepare =>
        preparing?.future ??
            {
              'version': 1,
              'ticket': 'owned-one-use',
              'previewText': 'Native sanitized preview: finite 2, unknown unit',
              'external': true,
              'expiresUtc': DateTime.now().toUtc().add(const Duration(minutes: 1)).toIso8601String(),
            },
      MetricsAiNativeOperation.send =>
        sending?.future ??
            {
              'version': 1,
              'explanation': 'Readonly owned explanation',
              'proposedPlaybook': '- name: Readonly proposed task',
              'limitations': ['Unknown clock skew'],
            },
      MetricsAiNativeOperation.discard => {},
    };
  }
}

Future<(ProviderContainer, MockBackend, AiProviderConfig)> _open(
  WidgetTester tester,
  _AiMonitoring service, {
  double width = 1400,
  bool russian = false,
  bool manual = false,
}) async {
  await loadAutomationProofFonts(tester);
  final backend = testBackend();
  addTearDown(backend.dispose);
  await backend.debugCreateUnlockedLocalProfile(name: 'Owned metrics AI profile');
  final provider = await backend.services.ai.saveProvider(
    AiProviderConfig.draft(AiProviderKind.openaiCompatible)
        .copyWith(name: 'Owned AI provider', baseUrl: 'https://provider.private.invalid', chatModel: 'Owned model'),
  );
  if (russian) {
    final settings = backend.services.settings;
    await settings.updateLocal(settings.currentLocal.copyWith(themeMode: AppThemeMode.dark));
  }
  await pumpApp(
    tester,
    backend,
    size: Size(width, 1050),
    locale: russian ? AppLocale.ru : AppLocale.en,
    captureBoundary: true,
    overrides: [monitoringBackendProvider.overrideWithValue(MonitoringTestBackend(service: service))],
  );
  final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
  container.read(routerProvider).go(AppRoutes.monitoring);
  await settle(tester);
  if (manual) await tapKey(tester, 'monitoring-auto-enabled');
  return (container, backend, provider);
}

Future<void> choose(WidgetTester tester) async {
  await tester.ensureVisible(find.byKey(const ValueKey('monitoring-review-ai')));
  expect(tester.widget<GlassButton>(find.byKey(const ValueKey('monitoring-review-ai'))).onPressed, isNotNull);
  await tapKey(tester, 'monitoring-review-ai');
  expect(
    find.text('Owned AI provider'),
    findsOneWidget,
    reason: tester.widgetList<Text>(find.byType(Text)).map((value) => value.data).whereType<String>().join(' | '),
  );
  await tester.tap(find.text('Owned AI provider').last);
  await settle(tester);
}

Future<void> prepare(WidgetTester tester) async {
  await tapKey(tester, 'metrics-ai-sample-0-0');
  await enterKey(tester, 'metrics-ai-question', 'Explain explicitly selected finite and missing values');
  await tapKey(tester, 'metrics-ai-primary');
}

void main() {
  testWidgets('page open and opening AI selector never issue query or provider request', (tester) async {
    final service = _AiMonitoring();
    await _open(tester, service);
    await choose(tester);
    expect(service.queries, 0);
    expect(service.calls, isEmpty);
    await enterKey(tester, 'metrics-ai-question', 'No unselected context');
    await tapKey(tester, 'metrics-ai-primary');
    expect(find.byKey(const ValueKey('metrics-ai-selection-error')), findsOneWidget);
    expect(service.calls, isEmpty);
  });
  testWidgets(
    'default Auto accepted result seeds review without another read; external Send requires explicit consent',
    (tester) async {
      final service = _AiMonitoring();
      await _open(tester, service);
      await tapKey(tester, 'monitoring-query');
      expect(service.queries, 1);
      await choose(tester);
      await prepare(tester);
      expect(service.queries, 1);
      expect(service.calls.where((c) => c.$1 == MetricsAiNativeOperation.prepare), hasLength(1));
      expect(service.calls.single.$2['selectedData'].toString(), isNot(contains('127.0.0.1')));
      expect(find.byKey(const ValueKey('metrics-ai-exact-preview')), findsOneWidget);
      expect(tester.widget<GlassButton>(find.byKey(const ValueKey('metrics-ai-primary'))).onPressed, isNull);
      await tapKey(tester, 'metrics-ai-consent');
      await tapKey(tester, 'metrics-ai-primary');
      expect(service.calls.where((c) => c.$1 == MetricsAiNativeOperation.send), hasLength(1));
      expect(find.byKey(const ValueKey('metrics-ai-playbook')), findsOneWidget);
      expect(find.byKey(const ValueKey('metrics-ai-explanation')), findsOneWidget);
      expect(tester.widget<GlassButton>(find.byKey(const ValueKey('metrics-ai-primary'))).onPressed, isNull);
      expect(service.queries, 1);
      expect(tester.takeException(), isNull);
    },
  );
  testWidgets('manual accepted result edits revoke preview; no silent resend or query', (tester) async {
    final service = _AiMonitoring();
    await _open(tester, service, manual: true);
    await tapKey(tester, 'monitoring-query');
    await choose(tester);
    await prepare(tester);
    await enterKey(tester, 'metrics-ai-question', 'Changed question must be reviewed again');
    expect(find.byKey(const ValueKey('metrics-ai-exact-preview')), findsNothing);
    expect(service.calls.where((c) => c.$1 == MetricsAiNativeOperation.discard), hasLength(1));
    expect(service.calls.where((c) => c.$1 == MetricsAiNativeOperation.send), isEmpty);
    expect(service.queries, 1);
  });
  testWidgets('provider edit invalidates exact preview and cannot send through latest provider', (tester) async {
    final service = _AiMonitoring();
    final (_, backend, provider) = await _open(tester, service, manual: true);
    await tapKey(tester, 'monitoring-query');
    await choose(tester);
    await prepare(tester);
    await backend.services.ai.saveProvider(provider.copyWith(chatModel: 'Changed reviewed model'));
    await settle(tester);
    expect(find.byKey(const ValueKey('metrics-ai-context-error')), findsOneWidget);
    expect(tester.widget<GlassButton>(find.byKey(const ValueKey('metrics-ai-primary'))).onPressed, isNull);
    expect(service.calls.where((c) => c.$1 == MetricsAiNativeOperation.send), isEmpty);
  });
  for (final lock in [false, true]) {
    testWidgets('${lock ? 'locking profile' : 'leaving Monitoring'} during pending preparation denies late preview', (
      tester,
    ) async {
      final service = _AiMonitoring()..preparing = Completer<Json>();
      final (container, _, _) = await _open(tester, service, manual: true);
      await tapKey(tester, 'monitoring-query');
      await choose(tester);
      await tapKey(tester, 'metrics-ai-sample-0-0');
      await enterKey(tester, 'metrics-ai-question', 'Owned pending question');
      await tester.ensureVisible(find.byKey(const ValueKey('metrics-ai-primary')));
      await tester.tap(find.byKey(const ValueKey('metrics-ai-primary')));
      await tester.pump();
      expect(find.byKey(const ValueKey('metrics-ai-progress')), findsOneWidget);
      if (lock) {
        await container.read(vaultServiceProvider).lock();
      } else {
        container.read(routerProvider).go(AppRoutes.terminal);
      }
      await settle(tester);
      service.preparing!.complete({
        'version': 1,
        'ticket': 'late',
        'previewText': 'LATE MUST NOT APPEAR',
        'external': true,
        'expiresUtc': DateTime.now().toUtc().add(const Duration(minutes: 1)).toIso8601String(),
      });
      await settle(tester);
      expect(find.text('LATE MUST NOT APPEAR'), findsNothing);
      expect(find.byType(MetricsAiReviewDialog), findsNothing);
      expect(service.calls.where((c) => c.$1 == MetricsAiNativeOperation.send), isEmpty);
    });
  }
  for (final width in [660.0, 1500.0]) {
    testWidgets('RU read-only exact preview fits $width', (tester) async {
      final service = _AiMonitoring();
      await _open(tester, service, width: width, russian: true, manual: true);
      await tapKey(tester, 'monitoring-query');
      await choose(tester);
      await prepare(tester);
      await tester.ensureVisible(find.byKey(const ValueKey('metrics-ai-exact-preview')));
      await tester.pump();
      expect(tester.takeException(), isNull);
      await captureAutomationWorkspace(tester, 'metrics-ai-preview-${width < 800 ? 'narrow' : 'wide'}-ru');
    });
  }
}
