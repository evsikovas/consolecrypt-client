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
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_ui_proof.dart';
import '../helpers/runner_cleanup_fakes.dart';
import '../helpers/test_app.dart' show settle, tapKey;

const _scope = AutomationScope('owned-candidate-ui-profile', true);
const _oldPython = '$cleanupPath/venv/bin/python';

class _Terminal extends Fake implements TerminalService {}

class _Candidates extends RunnerCleanupFixtureService
    implements AutomationRunnerService, AutomationRunnerSetupService, AutomationRunnerEditingService {
  int finds = 0, updates = 0, disconnects = 0;
  Completer<AutomationRunnerProfile>? pendingUpdate;
  bool blockSelected = false;
  static const profile = AutomationRunnerProfile(
    id: 'owned-runner',
    name: 'Keep this saved Runner name',
    revision: 7,
    readOnly: false,
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
  Future<AutomationRunnerCatalog> runners() async => const AutomationRunnerCatalog(readOnly: false, runners: [profile]);
  @override
  Future<AutomationJson> setupRunner(AutomationJson target, {required String operationId, bool install = false}) async {
    expect(install, isFalse);
    finds++;
    return {
      'system': 'Linux',
      'machine': 'x86_64',
      'python': '3.14',
      'user': 'runner',
      'non_root': true,
      'candidates': [
        profile.configuration!['launch'],
        for (final c in scanValue.candidates)
          if (c.id == cleanupId && c.paths != null) c.paths,
      ],
      'outdated_candidates': [_oldPython],
      'install_supported': true,
    };
  }

  @override
  Future<RunnerCleanupScan> scanRunnerCleanup(
    AutomationJson target, {
    required String operationId,
    String? selectedPython,
  }) async {
    final result = await super.scanRunnerCleanup(target, operationId: operationId, selectedPython: selectedPython);
    if (blockSelected && selectedPython == _oldPython) {
      return RunnerCleanupScan.fromJson(
        cleanupScanJson(
          candidates: [
            cleanupCandidateJson(eligible: false, blockedReason: 'cleanup_selected', omitUnsafeFields: true),
          ],
        ),
      );
    }
    return result;
  }

  @override
  Future<AutomationRunnerProfile> updateRunner(
    String id, {
    required int revision,
    required String name,
    required AutomationJson configuration,
  }) async {
    updates++;
    return pendingUpdate?.future ?? Future.value(profile);
  }

  @override
  Future<void> cancelRunnerSetup(String operationId) async {}
  @override
  Future<AutomationRunnerProfile> createRunner({required String name, required AutomationJson configuration}) async =>
      throw StateError('Fixture must not save');
  @override
  Future<AutomationJson> connectRunner(String id) async => throw StateError('Fixture must not connect');
  @override
  Future<void> disconnectRunner(String id) async {
    disconnects++;
  }

  @override
  Future<void> forgetRunner(String id, {required int revision}) async => throw StateError('Fixture must not forget');
}

Future<ProviderContainer> _open(
  WidgetTester tester,
  _Candidates service, {
  Locale locale = const Locale('en'),
  Size size = const Size(1200, 1000),
  double textScale = 1,
}) async {
  await loadAutomationProofFonts(tester);
  await tester.binding.setSurfaceSize(size);
  addTearDown(() => tester.binding.setSurfaceSize(null));
  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        automationScopeProvider.overrideWithValue(_scope),
        automationRunnerServiceProvider.overrideWithValue(service),
        terminalServiceProvider.overrideWithValue(_Terminal()),
        hostsProvider.overrideWith((ref) => Stream.value([])),
      ],
      child: MaterialApp(
        theme: AppTheme.dark(platform: TargetPlatform.linux),
        builder: (context, child) => MediaQuery(
          data: MediaQuery.of(context).copyWith(textScaler: TextScaler.linear(textScale)),
          child: child!,
        ),
        locale: locale,
        supportedLocales: AppLocalizations.supportedLocales,
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        home: const Scaffold(body: SizedBox()),
      ),
    ),
  );
  final context = tester.element(find.byType(Scaffold));
  final container = ProviderScope.containerOf(context);
  unawaited(
    showGlassDialog<void>(
      context,
      builder: (_) => AutomationRunnersDialog(scope: _scope, service: service, initialRunner: _Candidates.profile),
    ),
  );
  await settle(tester);
  await tapKey(tester, 'automation-detect-runner');
  expect(find.text('Linux \u00b7 x86_64 \u00b7 Python 3.14 \u00b7 runner'), findsOneWidget);
  return container;
}

Future<void> _dispose(WidgetTester tester) async {
  await tester.pumpWidget(const SizedBox.shrink());
  await tester.pump();
}

void main() {
  testWidgets(
    'row trash makes one fresh exact plan, needs confirm, rescans parent and preserves selected Runner/form',
    (tester) async {
      final service = _Candidates();
      await _open(tester, service);
      await tapKey(tester, 'automation-runner-delete-candidate-$_oldPython');
      expect(service.scans.length, 1);
      expect(service.plans.single.$2, [cleanupId]);
      expect(service.commits, isEmpty);
      expect(find.byKey(const ValueKey('automation-cleanup-confirm')), findsOneWidget);
      await tapKey(tester, 'automation-cleanup-confirm');
      expect(service.commits.single.$2, cleanupToken);
      expect(service.commits.single.$3, currentCleanupPython);
      expect(service.finds, 2);
      expect(find.byType(AutomationRunnerCleanupDialog), findsNothing);
      expect(find.byKey(const ValueKey('automation-runner-delete-candidate-$_oldPython')), findsNothing);
      final selected = tester.widget<CheckboxListTile>(
        find.byKey(const ValueKey('automation-runner-candidate-$currentCleanupPython')),
      );
      expect(selected.value, isTrue);
      tester
          .state<ScrollableState>(
            find.descendant(of: find.byType(AutomationRunnersDialog), matching: find.byType(Scrollable)).first,
          )
          .position
          .jumpTo(0);
      await settle(tester);
      expect(find.widgetWithText(TextField, 'Keep this saved Runner name'), findsOneWidget);
      expect(tester.takeException(), isNull);
      await _dispose(tester);
    },
    variant: const TargetPlatformVariant({TargetPlatform.linux}),
  );

  testWidgets(
    'selected old row is protected, explicit clear allows new scan, remaining candidate is never silently selected',
    (tester) async {
      final service = _Candidates()..blockSelected = true;
      await _open(tester, service);
      await tapKey(tester, 'automation-runner-candidate-$_oldPython');
      await tapKey(tester, 'automation-runner-delete-candidate-$_oldPython');
      expect(service.plans, isEmpty);
      expect(service.commits, isEmpty);
      final l = tester.element(find.byType(AutomationRunnerCleanupDialog)).l10n;
      expect(find.text(l.automationCleanupSelected), findsOneWidget);
      expect(find.text(l.automationRunnerSelectionProtectedHint), findsWidgets);
      await tapKey(tester, 'automation-cleanup-close');
      await tapKey(tester, 'automation-runner-clear-selection');
      await tapKey(tester, 'automation-runner-delete-candidate-$_oldPython');
      expect(service.plans.single.$3, isNull);
      await tapKey(tester, 'automation-cleanup-confirm');
      final remaining = tester.widget<CheckboxListTile>(
        find.byKey(const ValueKey('automation-runner-candidate-$currentCleanupPython')),
      );
      expect(remaining.value, isFalse);
      expect(service.finds, 2);
      expect(service.commits.length, 1);
      expect(tester.takeException(), isNull);
      await _dispose(tester);
    },
    variant: const TargetPlatformVariant({TargetPlatform.linux}),
  );

  for (final mismatch in ['missing', 'duplicate', 'samePythonOtherModule', 'registeredWithoutPaths']) {
    testWidgets('fresh $mismatch never retargets a row delete or offers another candidate', (tester) async {
      final service = _Candidates();
      await _open(tester, service);
      final other = cleanupCandidateJson(id: protectedCleanupId);
      if (mismatch == 'samePythonOtherModule') (other['paths'] as Map)['module_dir'] = '/different/module';
      service.scanValue = RunnerCleanupScan.fromJson(
        cleanupScanJson(
          candidates: switch (mismatch) {
            'missing' => [],
            'duplicate' => [cleanupCandidateJson(), other],
            'samePythonOtherModule' => [other],
            _ => [cleanupCandidateJson(eligible: false, blockedReason: 'cleanup_registered', omitUnsafeFields: true)],
          },
        ),
      );
      await tapKey(tester, 'automation-runner-delete-candidate-$_oldPython');
      expect(service.plans, isEmpty);
      expect(service.commits, isEmpty);
      final l = tester.element(find.byType(AutomationRunnerCleanupDialog)).l10n;
      expect(find.text(l.automationRunnerCandidateUnavailable), findsOneWidget);
      expect(find.byKey(const ValueKey('automation-cleanup-confirm')), findsNothing);
      expect(find.byKey(const ValueKey('automation-cleanup-review-$protectedCleanupId')), findsNothing);
      if (mismatch == 'registeredWithoutPaths') expect(find.text(l.automationCleanupRegistered), findsOneWidget);
      expect(tester.takeException(), isNull);
      await _dispose(tester);
    }, variant: const TargetPlatformVariant({TargetPlatform.linux}));
  }

  testWidgets('locking while targeted scan is pending cancels it and ignores its late candidate', (tester) async {
    final service = _Candidates();
    final container = await _open(tester, service);
    service.pendingScan = Completer<RunnerCleanupScan>();
    await tapKey(tester, 'automation-runner-delete-candidate-$_oldPython');
    container.updateOverrides([
      automationScopeProvider.overrideWithValue(const AutomationScope('owned-candidate-ui-profile', false)),
      automationRunnerServiceProvider.overrideWithValue(service),
      terminalServiceProvider.overrideWithValue(_Terminal()),
      hostsProvider.overrideWith((ref) => Stream.value([])),
    ]);
    await settle(tester);
    service.pendingScan!.complete(service.scanValue);
    await settle(tester);
    expect(service.cancelled, isNotEmpty);
    expect(service.plans, isEmpty);
    expect(service.commits, isEmpty);
    expect(find.byType(AutomationRunnerCleanupDialog), findsNothing);
    expect(tester.takeException(), isNull);
    await _dispose(tester);
  }, variant: const TargetPlatformVariant({TargetPlatform.linux}));

  testWidgets('changed Native plan receipt is rejected before confirmation', (tester) async {
    final service = _Candidates()..pendingPlan = Completer<RunnerCleanupPlan>();
    await _open(tester, service);
    await tapKey(tester, 'automation-runner-delete-candidate-$_oldPython');
    service.pendingPlan!.complete(
      RunnerCleanupPlan.fromJson(
        cleanupPlanJson(
          target: const {'kind': 'native'},
          candidates: [
            {...cleanupCandidateJson(), 'receiptSha256': 'f' * 64},
          ],
        ),
      ),
    );
    await settle(tester);
    expect(service.commits, isEmpty);
    expect(find.byKey(const ValueKey('automation-cleanup-confirm')), findsNothing);
    expect(tester.takeException(), isNull);
    await _dispose(tester);
  }, variant: const TargetPlatformVariant({TargetPlatform.linux}));
  for (final (width, textScale) in [(540.0, 1.0), (1200.0, 1.0), (540.0, 1.35)]) {
    testWidgets('Save stays beside Back and Close outside Runner body at Russian width $width scale $textScale', (
      tester,
    ) async {
      final service = _Candidates()..pendingUpdate = Completer<AutomationRunnerProfile>();
      await _open(tester, service, locale: const Locale('ru'), size: Size(width, 950), textScale: textScale);
      final footer = find.byKey(const ValueKey('automation-runner-footer'));
      final save = find.byKey(const ValueKey('automation-runner-save'));
      expect(find.descendant(of: footer, matching: save), findsOneWidget);
      expect(find.ancestor(of: save, matching: find.byType(ListView)), findsNothing);
      final l = tester.element(save).l10n;
      expect(find.descendant(of: footer, matching: find.text(l.commonBack)), findsOneWidget);
      expect(find.descendant(of: footer, matching: find.text(l.commonClose)), findsOneWidget);
      await tester.ensureVisible(find.byKey(const ValueKey('automation-runner-delete-candidate-$_oldPython')));
      await settle(tester);
      await captureAutomationDialog(
        tester,
        textScale > 1
            ? 'runner-candidates-footer-narrow-large-ru'
            : width > 1000
            ? 'runner-candidates-footer-wide-ru'
            : 'runner-candidates-footer-narrow-ru',
      );
      // Footer remains enabled and visible even when the long form is scrolled.
      await tapKey(tester, 'automation-runner-save');
      expect(service.updates, 1);
      expect(tester.widget<GlassButton>(save).busy, isTrue);
      expect(tester.widget<GlassButton>(save).onPressed, isNull);
      service.pendingUpdate!.complete(_Candidates.profile);
      await settle(tester);
      expect(service.updates, 1);
      expect(tester.takeException(), isNull);
      await _dispose(tester);
    }, variant: const TargetPlatformVariant({TargetPlatform.linux}));
  }

  testWidgets('explicit clear resets all path fields and disables footer Save until real input exists', (tester) async {
    final service = _Candidates();
    await _open(tester, service);
    await tapKey(tester, 'automation-runner-clear-selection');
    final save = find.byKey(const ValueKey('automation-runner-save'));
    expect(tester.widget<GlassButton>(save).onPressed, isNull);
    final pathFields = find.byWidgetPredicate(
      (w) =>
          w is TextField &&
          [currentCleanupPython, '/home/runner/current/src', '/home/runner/current/state'].contains(w.controller?.text),
      skipOffstage: false,
    );
    expect(pathFields, findsNothing);
    expect(service.plans, isEmpty);
    expect(service.commits, isEmpty);
    expect(service.updates, 0);
    expect(tester.takeException(), isNull);
    await _dispose(tester);
  }, variant: const TargetPlatformVariant({TargetPlatform.linux}));
}
