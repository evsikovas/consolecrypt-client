import 'dart:async';

import 'package:consolecrypt/ai/local/local_ai_hardware.dart';
import 'package:consolecrypt/ai/local/local_ai_runtime.dart';
import 'package:consolecrypt/ai/local_ai_providers.dart';
import 'package:consolecrypt/app/app.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:flutter/services.dart' show rootBundle;
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/fake_local_ai_runtime.dart';
import '../helpers/test_app.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  setUpAll(() async {
    // Cache real asset I/O outside the per-widget fake-async zones.
    await rootBundle.loadString('assets/licenses/LOCAL-AI-MODEL-NOTICES.txt');
  });
  for (final language in [AppLocale.en, AppLocale.ru]) {
    testWidgets('local AI settings $language install progress, default and removal use device state', (tester) async {
      final backend = testBackend();
      addTearDown(backend.dispose);
      await backend.debugSignInDemoAndUnlock();
      setTestLocale(backend, language);
      final runtime = FakeLocalAiRuntime(installed: false)..installGate = Completer<void>();
      addTearDown(runtime.dispose);
      tester.view.physicalSize = const Size(1200, 1000);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);
      await tester.pumpWidget(
        ProviderScope(
          overrides: [
            appServicesProvider.overrideWithValue(backend.services),
            deviceLocalAiAvailableProvider.overrideWithValue(true),
            localAiRuntimeProvider.overrideWithValue(runtime),
            localAiHardwareProvider.overrideWith(
              (ref) async =>
                  const LocalAiHardware(platform: LocalAiPlatform.windowsX64, totalMemoryBytes: 8 * 1024 * 1024 * 1024),
            ),
            aiServiceProvider.overrideWithValue(backend.services.ai),
          ],
          child: const ConsoleCryptApp(),
        ),
      );
      await settle(tester);
      final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
      container.read(routerProvider).go(AppRoutes.settings);
      await settle(tester);
      await tester.ensureVisible(find.byKey(const ValueKey('local-ai-default')));
      await tester.pump();
      expect(tester.widget<CheckboxListTile>(find.byKey(const ValueKey('local-ai-default'))).value, isTrue);
      await tapKey(tester, 'local-ai-default');
      expect(backend.services.settings.currentLocal.preferDeviceLocalAi, isFalse);
      expect(runtime.starts, 0);
      await tapKey(tester, 'local-ai-install');
      expect(runtime.installs, 1);
      expect(tester.widget<LinearProgressIndicator>(find.byKey(const ValueKey('local-ai-progress'))).value, .5);
      expect(find.byKey(const ValueKey('local-ai-cancel')), findsOneWidget);
      await tapKey(tester, 'local-ai-cancel');
      expect(runtime.cancellations, 1);
      runtime.installGate!.complete();
      await settle(tester);
      expect(find.byKey(const ValueKey('local-ai-install')), findsNothing);
      await tapKey(tester, 'local-ai-start');
      expect(runtime.starts, 1);
      expect(find.byKey(const ValueKey('local-ai-stop')), findsOneWidget);
      await tapKey(tester, 'local-ai-stop');
      expect(runtime.stops, 1);
      await tapKey(tester, 'local-ai-remove');
      expect(runtime.removals, 1);
      expect(find.byKey(const ValueKey('local-ai-install')), findsOneWidget);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox.shrink());
      await settle(tester);
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
    testWidgets('local AI settings $language preserve downloads and default while switching all models', (
      tester,
    ) async {
      final backend = testBackend();
      addTearDown(backend.dispose);
      await backend.debugSignInDemoAndUnlock();
      setTestLocale(backend, language);
      final runtime = FakeLocalAiRuntime();
      addTearDown(runtime.dispose);
      tester.view.physicalSize = const Size(1200, 1000);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);
      await tester.pumpWidget(
        ProviderScope(
          overrides: [
            appServicesProvider.overrideWithValue(backend.services),
            deviceLocalAiAvailableProvider.overrideWithValue(true),
            localAiRuntimeProvider.overrideWithValue(runtime),
            localAiHardwareProvider.overrideWith(
              (ref) async =>
                  const LocalAiHardware(platform: LocalAiPlatform.windowsX64, totalMemoryBytes: 8 * 1024 * 1024 * 1024),
            ),
            aiServiceProvider.overrideWithValue(backend.services.ai),
          ],
          child: const ConsoleCryptApp(),
        ),
      );
      await settle(tester);
      final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
      container.read(routerProvider).go(AppRoutes.settings);
      await settle(tester);
      for (final model in LocalAiCatalog.models.skip(1)) {
        await tapKey(tester, 'local-ai-model');
        expect(find.byKey(ValueKey('local-ai-recommended-${LocalAiCatalog.qwen4B.id}')).hitTestable(), findsOneWidget);
        await tester.tap(find.textContaining(model.label).last);
        await settle(tester);
        expect(runtime.selectedModel.id, model.id);
        expect(backend.services.settings.currentLocal.preferDeviceLocalAi, isTrue);
        final l = tester.element(find.byKey(const ValueKey('local-ai-settings'))).l10n;
        expect(find.text(l.localAiRamRecommendation(model.recommendedRamGiB)), findsOneWidget);
        expect(find.byKey(const ValueKey('local-ai-selected-missing')), findsOneWidget);
        expect(runtime.status.installedModelIds, contains(LocalAiCatalog.model.id));
        expect(backend.services.settings.currentLocal.preferDeviceLocalAi, isTrue);
        await tapKey(tester, 'local-ai-install');
        expect(runtime.status.isInstalled, isTrue);
      }
      expect(runtime.status.installedModelIds, LocalAiCatalog.models.map((m) => m.id).toSet());
      await tapKey(tester, 'local-ai-remove');
      expect(runtime.status.installedModelIds, {LocalAiCatalog.model.id, LocalAiCatalog.qwen4B.id});
      expect(runtime.selectedModel.id, LocalAiCatalog.qwen9B.id);
      expect(backend.services.settings.currentLocal.preferDeviceLocalAi, isTrue);
      expect(find.byKey(const ValueKey('local-ai-selected-missing')), findsOneWidget);
      // Simulate the persisted selection being restored after an async offline scan.
      await runtime.selectModel(LocalAiCatalog.model.id);
      await settle(tester);
      expect(
        tester
            .widget<DropdownButtonFormField<String>>(
              find.descendant(
                of: find.byKey(const ValueKey('local-ai-model')),
                matching: find.byType(DropdownButtonFormField<String>),
              ),
            )
            .initialValue,
        LocalAiCatalog.model.id,
      );
      expect(runtime.starts, 0);
      final noticesButton = find.byKey(const ValueKey('local-ai-notices'));
      await tester.ensureVisible(noticesButton);
      await tester.pump();
      await tester.runAsync(() async {
        await tester.tap(noticesButton);
        await rootBundle.loadString('assets/licenses/LOCAL-AI-MODEL-NOTICES.txt');
      });
      await settle(tester);
      expect(find.byType(SelectableText), findsWidgets);
      expect(
        find.byWidgetPredicate(
          (widget) => widget is SelectableText && (widget.data?.contains('Qwen3.5 9B Q4_K_M') ?? false),
        ),
        findsOneWidget,
      );
      final l = tester.element(find.byKey(const ValueKey('local-ai-settings'))).l10n;
      await tester.tap(find.text(l.commonClose).last);
      await settle(tester);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox.shrink());
      await settle(tester);
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
  }
}
