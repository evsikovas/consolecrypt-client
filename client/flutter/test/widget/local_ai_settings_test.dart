import 'dart:async';

import 'package:consolecrypt/ai/local_ai_providers.dart';
import 'package:consolecrypt/app/app.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/fake_local_ai_runtime.dart';
import '../helpers/test_app.dart';

void main() {
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
  }
}
