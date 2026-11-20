import 'dart:async';

import 'package:consolecrypt/ai/local/local_ai_hardware.dart';
import 'package:consolecrypt/ai/local/local_ai_runtime.dart';
import 'package:consolecrypt/ai/local_ai_providers.dart';
import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/settings/local_ai_settings.dart';
import 'package:flutter/services.dart' show rootBundle;
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/fake_local_ai_runtime.dart';
import '../helpers/test_app.dart' show testBackend;

class UnjoinedRuntime extends FakeLocalAiRuntime {
  UnjoinedRuntime() {
    state.value = LocalAiRuntimeStatus(
      phase: LocalAiPhase.error,
      isInstalled: true,
      runtimeInstalled: true,
      installedModelIds: {LocalAiCatalog.model.id},
      message: 'runtime_stop_failed',
    );
  }
  final joined = Completer<void>();
  @override
  Future<void> stop() async {
    stops++;
    emit(LocalAiPhase.stopping);
    await joined.future;
    emit(LocalAiPhase.stopped);
  }
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  setUpAll(() async {
    await rootBundle.loadString('assets/licenses/LOCAL-AI-MODEL-NOTICES.txt');
  });
  for (final language in ['en', 'ru']) {
    testWidgets('unconfirmed local model exit $language exposes only explicit Stop retry until actual completion', (
      tester,
    ) async {
      final backend = testBackend(), runtime = UnjoinedRuntime();
      addTearDown(backend.dispose);
      addTearDown(runtime.dispose);
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
          ],
          child: MaterialApp(
            locale: Locale(language),
            theme: AppTheme.dark(platform: TargetPlatform.windows),
            supportedLocales: AppLocalizations.supportedLocales,
            localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
            home: const Scaffold(body: SingleChildScrollView(child: LocalAiSettings())),
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('local-ai-stop-unconfirmed')), findsOneWidget);
      expect(find.byKey(const ValueKey('local-ai-start')), findsNothing);
      final stop = find.byKey(const ValueKey('local-ai-stop'));
      await tester.ensureVisible(stop);
      await tester.pumpAndSettle();
      expect(tester.widget<GlassButton>(stop).onPressed, isNotNull);
      await tester.tap(stop);
      await tester.pumpAndSettle();
      expect(runtime.stops, 1);
      expect(runtime.starts, 0);
      expect(runtime.removals, 0);
      expect(runtime.installs, 0);
      expect(tester.widget<GlassButton>(stop).onPressed, isNull);
      expect(find.byKey(const ValueKey('local-ai-start')), findsNothing);
      runtime.joined.complete();
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('local-ai-stop-unconfirmed')), findsNothing);
      expect(find.byKey(const ValueKey('local-ai-start')), findsOneWidget);
      expect(runtime.starts, 0);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox.shrink());
    });
  }
}
