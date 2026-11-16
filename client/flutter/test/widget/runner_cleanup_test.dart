import 'dart:async';

import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_runner_cleanup.dart';
import 'package:consolecrypt/automation/automation_runner_cleanup_dialog.dart';
import 'package:consolecrypt/automation/automation_runners_dialog.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/terminal_service.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_ui_proof.dart';
import '../helpers/runner_cleanup_fakes.dart';
import '../helpers/test_app.dart' show settle, tapKey;

const scope = AutomationScope('cleanup-profile', true);
Future<ProviderContainer> open(
  WidgetTester tester,
  RunnerCleanupFixtureService service, {
  ValueNotifier<bool>? contextValid,
  Size size = const Size(1200, 900),
  Locale locale = const Locale('en'),
}) async {
  await tester.binding.setSurfaceSize(size);
  addTearDown(() => tester.binding.setSurfaceSize(null));
  await loadAutomationProofFonts(tester);
  await tester.pumpWidget(
    ProviderScope(
      overrides: [automationScopeProvider.overrideWithValue(scope)],
      child: MaterialApp(
        theme: AppTheme.dark(platform: TargetPlatform.windows),
        locale: locale,
        supportedLocales: AppLocalizations.supportedLocales,
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        home: Scaffold(
          body: RepaintBoundary(
            key: const ValueKey('automation-proof-root'),
            child: Builder(
              builder: (context) => TextButton(
                key: const ValueKey('open-cleanup'),
                onPressed: () => showGlassDialog<void>(
                  context,
                  builder: (_) => AutomationRunnerCleanupDialog(
                    scope: scope,
                    service: service,
                    target: cleanupTarget,
                    selectedPython: currentCleanupPython,
                    targetLabel: 'Ubuntu · runner',
                    contextCurrent: () => contextValid?.value ?? true,
                    contextChanges: contextValid,
                  ),
                ),
                child: const Text('Fixture'),
              ),
            ),
          ),
        ),
      ),
    ),
  );
  final container = ProviderScope.containerOf(tester.element(find.byKey(const ValueKey('open-cleanup'))));
  await tapKey(tester, 'open-cleanup');
  return container;
}

Future<void> close(WidgetTester tester) async {
  if (find.byKey(const ValueKey('automation-cleanup-close')).evaluate().isNotEmpty) {
    await tapKey(tester, 'automation-cleanup-close');
  }
  await tester.pumpWidget(const SizedBox.shrink());
  await tester.pump();
}

class _Terminal extends Fake implements TerminalService {}

class _ManagedRunners extends RunnerCleanupFixtureService
    implements AutomationRunnerService, AutomationRunnerSetupService, AutomationRunnerEditingService {
  int catalogs = 0, setups = 0;
  final installs = <bool>[];
  AutomationRunnerSetupFailed? setupError;
  static const profile = AutomationRunnerProfile(
    id: 'runner',
    name: 'Current managed Runner',
    readOnly: false,
    revision: 7,
    configuration: {
      'kind': 'local',
      'launch': {
        'kind': 'native',
        'python': currentCleanupPython,
        'module_dir': '/home/runner/current/src',
        'root': '/home/runner/current/state',
      },
    },
  );
  @override
  Future<AutomationRunnerCatalog> runners() async {
    catalogs++;
    return const AutomationRunnerCatalog(readOnly: false, runners: [profile]);
  }

  @override
  Future<AutomationJson> setupRunner(AutomationJson target, {required String operationId, bool install = false}) async {
    setups++;
    installs.add(install);
    if (setupError != null) throw setupError!;
    return {
      'system': 'Linux',
      'machine': 'x86_64',
      'python': '3.12',
      'user': 'runner',
      'non_root': true,
      'candidates': [
        profile.configuration!['launch'],
        for (final candidate in scanValue.candidates)
          if (candidate.id == cleanupId && candidate.paths != null) candidate.paths,
      ],
      'outdated_candidates': <String>[],
      'install_supported': true,
    };
  }

  @override
  Future<void> cancelRunnerSetup(String operationId) async {}
  @override
  Future<AutomationRunnerProfile> createRunner({required String name, required AutomationJson configuration}) async =>
      throw StateError('No create expected');
  @override
  Future<AutomationJson> connectRunner(String id) async => throw StateError('No connect expected');
  @override
  Future<void> disconnectRunner(String id) async => throw StateError('No disconnect expected');
  @override
  Future<void> forgetRunner(String id, {required int revision}) async => throw StateError('No forget expected');
  @override
  Future<AutomationRunnerProfile> updateRunner(
    String id, {
    required int revision,
    required String name,
    required AutomationJson configuration,
  }) async => throw StateError('No update expected');
}

Future<void> _openRunnerForm(WidgetTester tester, _ManagedRunners service) async {
  await tester.binding.setSurfaceSize(const Size(1200, 900));
  addTearDown(() => tester.binding.setSurfaceSize(null));
  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        automationScopeProvider.overrideWithValue(scope),
        automationRunnerServiceProvider.overrideWithValue(service),
        terminalServiceProvider.overrideWithValue(_Terminal()),
        hostsProvider.overrideWith((ref) => Stream.value([])),
      ],
      child: MaterialApp(
        theme: AppTheme.dark(platform: TargetPlatform.windows),
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        supportedLocales: AppLocalizations.supportedLocales,
        home: Scaffold(
          body: AutomationRunnersDialog(scope: scope, service: service, initialRunner: _ManagedRunners.profile),
        ),
      ),
    ),
  );
  await settle(tester);
}

void main() {
  for (final reason in [RunnerCleanupReason.bootstrapFailed, RunnerCleanupReason.commandTooLarge]) {
    testWidgets('cleanup ${reason.wire} has a specific boxed error and never offers deletion', (tester) async {
      final service = RunnerCleanupFixtureService()
        ..scanError = AutomationRunnerCleanupFailed(reason);
      await open(tester, service);
      final banner = find.byKey(const ValueKey('automation-cleanup-error'));
      expect(tester.widget(banner), isA<InfoBanner>());
      final l = tester.element(banner).l10n;
      final expected = reason == RunnerCleanupReason.bootstrapFailed
          ? l.automationCleanupBootstrapFailed
          : l.automationCleanupCommandTooLarge;
      expect(find.text(expected), findsOneWidget);
      expect(find.text(l.automationCleanupFailed), findsNothing);
      expect(service.plans, isEmpty);
      expect(service.commits, isEmpty);
      expect(tester.takeException(), isNull);
      await close(tester);
    });
  }
  for (final reason in ['setup_bootstrap_failed', 'setup_command_too_large', 'setup_python_missing']) {
    testWidgets('Runner $reason is distinct from PyPI failure and stays inside the error banner', (tester) async {
      final service = _ManagedRunners()..setupError = AutomationRunnerSetupFailed(reason);
      await _openRunnerForm(tester, service);
      await tapKey(tester, 'automation-detect-runner');
      final banner = find.byKey(const ValueKey('automation-runner-setup-error'));
      expect(tester.widget(banner), isA<InfoBanner>());
      final l = tester.element(banner).l10n;
      expect(find.text(switch (reason) {
        'setup_bootstrap_failed' => l.automationSetupBootstrapFailed,
        'setup_python_missing' => l.automationSetupPythonMissing,
        _ => l.automationSetupCommandTooLarge,
      }), findsOneWidget);
      expect(find.text(l.automationSetupFailed), findsNothing);
      expect(service.installs, [false]);
      expect(service.commits, isEmpty);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox.shrink());
      await tester.pump();
    }, variant: const TargetPlatformVariant({TargetPlatform.linux}));
  }
  testWidgets(
    'scan and review expose exact managed paths and material notices; deletion needs a separate single confirmation',
    (tester) async {
      final service = RunnerCleanupFixtureService();
      await open(tester, service);
      expect(service.scans.length, 1);
      expect(service.plans, isEmpty);
      expect(service.commits, isEmpty);
      expect(find.byKey(const ValueKey('automation-cleanup-review-$protectedCleanupId')), findsNothing);
      await tapKey(tester, 'automation-cleanup-review-$cleanupId');
      expect(service.plans.single.$2, [cleanupId]);
      expect(service.commits, isEmpty);
      expect(find.byKey(const ValueKey('automation-cleanup-confirm')), findsOneWidget);
      final l = tester.element(find.byKey(const ValueKey('automation-cleanup-confirm'))).l10n;
      expect(find.text(l.automationCleanupCurrentProfile), findsOneWidget);
      expect(find.text(l.automationCleanupStateNotice), findsOneWidget);
      expect(find.text(cleanupPath), findsOneWidget);
      expect(find.text('$cleanupPath/state'), findsOneWidget);
      expect(find.textContaining('4000'), findsOneWidget);
      expect(find.textContaining('123456'), findsOneWidget);
      expect(find.text(cleanupToken), findsNothing);
      await tapKey(tester, 'automation-cleanup-confirm');
      expect(service.commits.length, 1);
      expect(service.commits.single.$2, cleanupToken);
      expect(service.commits.single.$3, currentCleanupPython);
      expect(service.scans.length, 2); // Read-only authoritative list refresh.
      expect(find.byKey(const ValueKey('automation-cleanup-success')), findsOneWidget);
      expect(find.byKey(const ValueKey('automation-cleanup-confirm')), findsNothing);
      expect(find.byKey(const ValueKey('automation-cleanup-review-$cleanupId')), findsNothing);
      expect(tester.takeException(), isNull);
      await close(tester);
    },
  );

  testWidgets('back from review discards approval and never commits or automatically replans', (tester) async {
    final service = RunnerCleanupFixtureService();
    await open(tester, service);
    await tapKey(tester, 'automation-cleanup-review-$cleanupId');
    await tapKey(tester, 'automation-cleanup-back');
    await tester.pump(const Duration(seconds: 10));
    await settle(tester);
    expect(service.commits, isEmpty);
    expect(service.plans.length, 1);
    expect(find.byKey(const ValueKey('automation-cleanup-confirm')), findsNothing);
    expect(tester.takeException(), isNull);
    await close(tester);
  });

  testWidgets('repeated confirmation consumes actionable approval once while commit is pending', (tester) async {
    final pending = Completer<RunnerCleanupResult>();
    final service = RunnerCleanupFixtureService()..pendingCommit = pending;
    await open(tester, service);
    await tapKey(tester, 'automation-cleanup-review-$cleanupId');
    final confirm = tester.widget<GlassButton>(find.byKey(const ValueKey('automation-cleanup-confirm'))).onPressed!;
    confirm();
    confirm();
    await settle(tester);
    expect(service.commits.length, 1);
    expect(find.byKey(const ValueKey('automation-cleanup-confirm')), findsNothing);
    pending.complete(RunnerCleanupResult.fromJson(cleanupResultJson()));
    await settle(tester);
    expect(service.commits.length, 1);
    expect(tester.takeException(), isNull);
    await close(tester);
  });

  testWidgets('approval expires after sixty seconds and cannot be confirmed after expiry', (tester) async {
    final service = RunnerCleanupFixtureService();
    await open(tester, service);
    await tapKey(tester, 'automation-cleanup-review-$cleanupId');
    await tester.pump(const Duration(seconds: 61));
    await settle(tester);
    expect(find.byKey(const ValueKey('automation-cleanup-confirm')), findsNothing);
    final l = tester.element(find.byKey(const ValueKey('automation-cleanup-error'))).l10n;
    expect(find.text(l.automationCleanupExpired), findsOneWidget);
    expect(service.commits, isEmpty);
    expect(tester.takeException(), isNull);
    await close(tester);
  });

  for (final phase in ['plan', 'review']) {
    testWidgets('target or selected Python context change discards $phase and rejects a late plan', (tester) async {
      final valid = ValueNotifier(true);
      addTearDown(valid.dispose);
      final service = RunnerCleanupFixtureService();
      if (phase == 'plan') service.pendingPlan = Completer<RunnerCleanupPlan>();
      await open(tester, service, contextValid: valid);
      await tapKey(tester, 'automation-cleanup-review-$cleanupId');
      valid.value = false;
      await settle(tester);
      service.pendingPlan?.complete(RunnerCleanupPlan.fromJson(cleanupPlanJson()));
      await settle(tester);
      expect(find.byKey(const ValueKey('automation-cleanup-confirm')), findsNothing);
      expect(service.commits, isEmpty);
      if (phase == 'plan') expect(service.cancelled, isNotEmpty);
      expect(tester.takeException(), isNull);
      await close(tester);
    });
  }

  testWidgets('locking removes the cleanup route and drops the pending plan without committing', (tester) async {
    final service = RunnerCleanupFixtureService()..pendingPlan = Completer<RunnerCleanupPlan>();
    final container = await open(tester, service);
    await tapKey(tester, 'automation-cleanup-review-$cleanupId');
    container.updateOverrides([
      automationScopeProvider.overrideWithValue(const AutomationScope('cleanup-profile', false)),
    ]);
    await settle(tester);
    service.pendingPlan!.complete(RunnerCleanupPlan.fromJson(cleanupPlanJson()));
    await settle(tester);
    expect(find.byType(AutomationRunnerCleanupDialog), findsNothing);
    expect(service.commits, isEmpty);
    expect(service.cancelled, isNotEmpty);
    expect(tester.takeException(), isNull);
    await close(tester);
  });

  testWidgets('unknown ineligible reason is a safe generic label with no Delete and no invented counters', (
    tester,
  ) async {
    final service = RunnerCleanupFixtureService()
      ..scanValue = RunnerCleanupScan.fromJson(
        cleanupScanJson(
          candidates: [
            cleanupCandidateJson(
              eligible: false,
              omitUnsafeFields: true,
              blockedReason: 'synthetic-secret raw unsafe body',
              quarantined: true,
            ),
          ],
        ),
      );
    await open(tester, service);
    expect(find.byKey(const ValueKey('automation-cleanup-review-$cleanupId')), findsNothing);
    final l = tester.element(find.byKey(const ValueKey('automation-cleanup-scan'))).l10n;
    expect(find.text(l.automationCleanupUnsafe), findsOneWidget);
    expect(find.textContaining(l.automationCleanupUnknownCount), findsNWidgets(2));
    expect(find.textContaining('synthetic-secret'), findsNothing);
    expect(service.commits, isEmpty);
    expect(tester.takeException(), isNull);
    await close(tester);
  });

  testWidgets('unsupported Native platform stays read only with no destructive fallback', (tester) async {
    final service = RunnerCleanupFixtureService()
      ..scanError = const AutomationRunnerCleanupFailed(RunnerCleanupReason.unsupported);
    await open(tester, service);
    final l = tester.element(find.byKey(const ValueKey('automation-cleanup-error'))).l10n;
    expect(find.text(l.automationCleanupUnsupported), findsOneWidget);
    expect(find.byKey(const ValueKey('automation-cleanup-confirm')), findsNothing);
    expect(service.commits, isEmpty);
    expect(service.plans, isEmpty);
    expect(tester.takeException(), isNull);
    await close(tester);
  });

  testWidgets('failed commit discards the one-use approval and exposes quarantine only after an explicit new scan', (
    tester,
  ) async {
    final service = RunnerCleanupFixtureService();
    await open(tester, service);
    await tapKey(tester, 'automation-cleanup-review-$cleanupId');
    service.commitError = const AutomationRunnerCleanupFailed(RunnerCleanupReason.failed);
    service.scanValue = RunnerCleanupScan.fromJson(
      cleanupScanJson(candidates: [cleanupCandidateJson(quarantined: true)]),
    );
    await tapKey(tester, 'automation-cleanup-confirm');
    expect(find.byKey(const ValueKey('automation-cleanup-confirm')), findsNothing);
    expect(service.scans.length, 1);
    expect(service.commits.length, 1);
    await tester.pump(const Duration(seconds: 5));
    await settle(tester);
    expect(service.commits.length, 1);
    await tapKey(tester, 'automation-cleanup-scan');
    final l = tester.element(find.byKey(const ValueKey('automation-cleanup-scan'))).l10n;
    expect(find.text(l.automationCleanupQuarantine), findsOneWidget);
    expect(service.commits.length, 1);
    expect(tester.takeException(), isNull);
    await close(tester);
  });

  for (final width in [540.0, 1200.0]) {
    testWidgets('Russian cleanup review keeps exact paths, privacy and state notices readable at width $width', (
      tester,
    ) async {
      final service = RunnerCleanupFixtureService();
      await open(tester, service, locale: const Locale('ru'), size: Size(width, 950));
      await tapKey(tester, 'automation-cleanup-review-$cleanupId');
      await captureAutomationDialog(
        tester,
        width > 1000 ? 'runner-cleanup-review-wide-ru' : 'runner-cleanup-review-narrow-ru',
      );
      expect(service.commits, isEmpty);
      expect(tester.takeException(), isNull);
      await close(tester);
    }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  }

  testWidgets('saved Runner cleanup entry preserves settings and invalidates approval when Python changes', (
    tester,
  ) async {
    final service = _ManagedRunners();
    await _openRunnerForm(tester, service);
    await tapKey(tester, 'automation-open-cleanup');
    expect(service.scans.single.$3, currentCleanupPython);
    expect(service.scans.single.$2, {'kind': 'native'});
    await tapKey(tester, 'automation-cleanup-review-$cleanupId');
    final python = find.byWidgetPredicate(
      (w) => w is TextField && w.controller?.text == currentCleanupPython,
      skipOffstage: false,
    );
    tester.widget<TextField>(python).controller!.text = '/home/runner/different/venv/bin/python';
    await settle(tester);
    expect(find.byKey(const ValueKey('automation-cleanup-confirm')), findsNothing);
    expect(service.commits, isEmpty);
    await tapKey(tester, 'automation-cleanup-close');
    expect(find.widgetWithText(TextField, 'Current managed Runner'), findsOneWidget);
    expect(service.setups, 0);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox.shrink());
    await tester.pump();
  }, variant: const TargetPlatformVariant({TargetPlatform.linux}));

  testWidgets(
    'successful cleanup refreshes parent Find results and never installs or deletes another environment automatically',
    (tester) async {
      final service = _ManagedRunners();
      await _openRunnerForm(tester, service);
      await tapKey(tester, 'automation-detect-runner');
      expect(service.setups, 1);
      expect(find.text('$cleanupPath/venv/bin/python'), findsOneWidget);
      await tapKey(tester, 'automation-open-cleanup');
      await tapKey(tester, 'automation-cleanup-review-$cleanupId');
      await tapKey(tester, 'automation-cleanup-confirm');
      await tapKey(tester, 'automation-cleanup-close');
      expect(service.setups, 2);
      expect(service.installs, [false, false]);
      expect(service.commits.length, 1);
      expect(find.text('$cleanupPath/venv/bin/python'), findsNothing);
      expect(find.widgetWithText(TextField, currentCleanupPython), findsOneWidget);
      // The name can be outside the lazy list's cache after cleanup on larger
      // fallback fonts. Bring the same parent form back into view to check it.
      tester
          .state<ScrollableState>(
            find.descendant(of: find.byType(AutomationRunnersDialog), matching: find.byType(Scrollable)).first,
          )
          .position
          .jumpTo(0);
      await settle(tester);
      expect(find.widgetWithText(TextField, 'Current managed Runner'), findsOneWidget);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox.shrink());
      await tester.pump();
    },
    variant: const TargetPlatformVariant({TargetPlatform.linux}),
  );
}
