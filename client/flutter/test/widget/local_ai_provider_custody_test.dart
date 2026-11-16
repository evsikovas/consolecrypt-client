import 'dart:async';
import 'dart:io';

import 'package:consolecrypt/ai/local/local_ai_hardware.dart';
import 'package:consolecrypt/ai/local/local_ai_runtime.dart';
import 'package:consolecrypt/ai/local/src/local_ai_process.dart';
import 'package:consolecrypt/ai/local_ai_providers.dart';
import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/settings/local_ai_settings.dart';
import 'package:flutter/services.dart' show rootBundle;
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/fake_local_ai_runtime.dart';
import '../helpers/test_app.dart' show testBackend;

class HeldProcess extends Fake implements Process {
  final exited = Completer<int>();
  bool finishOnKill = false;
  @override
  Future<int> get exitCode => exited.future;
  @override
  bool kill([ProcessSignal signal = ProcessSignal.sigterm]) {
    if (finishOnKill && !exited.isCompleted) exited.complete(0);
    return false;
  }
}

class OwnedRuntime extends FakeLocalAiRuntime {
  OwnedRuntime() {
    custody.attach(
      child,
      onExit: () {},
      onUnknownExit: () {},
      grace: const Duration(milliseconds: 1),
      forceGrace: const Duration(milliseconds: 1),
    );
    emit(LocalAiPhase.ready);
  }
  final child = HeldProcess(), custody = LocalAiProcessCustody();
  bool endpointValid = true;
  @override
  Future<void> stop() {
    stops++;
    endpointValid = false;
    emit(LocalAiPhase.stopping);
    return custody.stopAfter(null).then((_) => emit(LocalAiPhase.stopped)).catchError((Object error) {
      state.value = LocalAiRuntimeStatus(
        phase: LocalAiPhase.error,
        isInstalled: true,
        runtimeInstalled: true,
        installedModelIds: {selectedModel.id},
        message: 'runtime_stop_failed',
      );
      throw const LocalAiException('runtime_stop_failed');
    });
  }
}

AppServices withExit(AppServices old, ApprovedExitActions actions) => AppServices(
  profiles: old.profiles,
  auth: old.auth,
  vault: old.vault,
  inventory: old.inventory,
  terminal: old.terminal,
  sftp: old.sftp,
  tunnels: old.tunnels,
  snippets: old.snippets,
  ai: old.ai,
  devices: old.devices,
  sync: old.sync,
  settings: old.settings,
  backups: old.backups,
  files: old.files,
  approvedExitActions: actions,
);
void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  setUpAll(() async {
    await rootBundle.loadString('assets/licenses/LOCAL-AI-MODEL-NOTICES.txt');
  });
  testWidgets(
    'same container AppServices replacement retains original runtime and owned child; failed stop keeps retry UI and rebinds exit cleanup',
    (tester) async {
      final backend = testBackend(), oldExit = ApprovedExitActions(), nextExit = ApprovedExitActions();
      addTearDown(backend.dispose);
      final runtimes = <OwnedRuntime>[];
      final oldServices = withExit(backend.services, oldExit), nextServices = withExit(backend.services, nextExit);
      final runtimeOverride = localAiRuntimeProvider.overrideWith((ref) {
        final runtime = OwnedRuntime();
        runtimes.add(runtime);
        return bindLocalAiRuntimeLifecycle(ref, runtime);
      });
      final availableOverride = deviceLocalAiAvailableProvider.overrideWithValue(true);
      final hardwareOverride = localAiHardwareProvider.overrideWith(
        (ref) async =>
            const LocalAiHardware(platform: LocalAiPlatform.windowsX64, totalMemoryBytes: 8 * 1024 * 1024 * 1024),
      );
      final container = ProviderContainer(
        overrides: [
          appServicesProvider.overrideWithValue(oldServices),
          availableOverride,
          runtimeOverride,
          hardwareOverride,
        ],
      );
      addTearDown(container.dispose);
      await tester.pumpWidget(
        UncontrolledProviderScope(
          container: container,
          child: MaterialApp(
            theme: AppTheme.dark(platform: TargetPlatform.windows),
            supportedLocales: AppLocalizations.supportedLocales,
            localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
            home: const Scaffold(body: SingleChildScrollView(child: LocalAiSettings())),
          ),
        ),
      );
      await tester.pumpAndSettle();
      final original = container.read(localAiRuntimeProvider) as OwnedRuntime;
      final process = original.custody.current;
      container.updateOverrides([
        appServicesProvider.overrideWithValue(nextServices),
        availableOverride,
        runtimeOverride,
        hardwareOverride,
      ]);
      expect(identical(container.read(localAiRuntimeProvider), original), isTrue);
      expect(original.endpointValid, isFalse);
      expect(original.stops, 1);
      await tester.pumpAndSettle();
      expect(runtimes, hasLength(1));
      expect(identical(original.custody.current, process), isTrue);
      expect(find.byKey(const ValueKey('local-ai-stop-unconfirmed')), findsOneWidget);
      expect(find.byKey(const ValueKey('local-ai-start')), findsNothing);
      await oldExit.run();
      expect(original.stops, 1);
      original.child.finishOnKill = true;
      final stop = find.byKey(const ValueKey('local-ai-stop'));
      await tester.ensureVisible(stop);
      await tester.pumpAndSettle();
      await tester.tap(stop);
      await tester.pumpAndSettle();
      expect(original.stops, 2);
      expect(original.custody.current, isNull);
      expect(original.starts, 0);
      expect(runtimes, hasLength(1));
      expect(find.byKey(const ValueKey('local-ai-start')), findsOneWidget);
      await nextExit.run();
      expect(original.stops, 3);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox.shrink());
    },
  );
}
