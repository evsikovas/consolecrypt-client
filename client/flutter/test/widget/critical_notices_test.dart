import 'dart:async';

import 'package:consolecrypt/app/critical_notice_providers.dart';
import 'package:consolecrypt/app/critical_notice_view.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/navigation_customization.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/app/workspace_tools_controller.dart';
import 'package:consolecrypt/automation/automation_execution_console.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/job_notice_events.dart';
import 'package:consolecrypt/terminal/terminal_minimal_mode.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_ui_proof.dart';
import '../helpers/test_app.dart';

const jobId = '11111111-1111-4111-8111-111111111111';

class _Jobs implements AutomationService, AutomationJobService, AutomationEventService {
  int reads = 0;
  String state = 'failed';
  int revision = 1;
  Completer<AutomationJson>? pending;
  final forbidden = <Symbol>[];
  @override
  Future<AutomationProjectCatalog> projects() async =>
      const AutomationProjectCatalog(version: 1, readOnly: false, deviceLocal: true, projects: []);
  @override
  Future<AutomationJson> jobs() async {
    reads++;
    return pending?.future ??
        {
          'schema_version': 1,
          'read_only': false,
          'jobs': [
            {
              'version': 1,
              'job_id': jobId,
              'state': state,
              'notification_revision': revision,
              'accepted_at': 1,
              'project_id': 'owned-project',
              'host_ids': ['22222222-2222-4222-8222-222222222222'],
              'manifest': {'playbook': 'owned.yml', 'mode': 'check', 'targets': <Object?>[]},
              'runner_result': {
                'version': 1,
                'result': {
                  'exit_code': 2,
                  'summary': {'stats': <String, Object?>{}},
                },
              },
            },
          ],
        };
  }

  @override
  Future<AutomationJson> jobEvents(String id, {required int after, int limit = 50}) async => {
    'events': <Object?>[],
    'cursor': 0,
    'output_truncated': false,
  };
  @override
  dynamic noSuchMethod(Invocation invocation) {
    forbidden.add(invocation.memberName);
    throw StateError('Unexpected operation ${invocation.memberName}');
  }
}

class _Harness {
  _Harness(this.container, this.app, this.events, this.jobs, this.profile);
  final ProviderContainer container;
  final MockBackend app;
  final JobNoticeEventAdapter events;
  final _Jobs jobs;
  final String profile;
  Future<void> emit(WidgetTester tester, {String state = 'failed', int revision = 1}) async {
    jobs.state = state;
    jobs.revision = revision;
    events.ingest({
      'type': 'automation_job_changed',
      'profile_id': profile,
      'job_id': jobId,
      'revision': revision,
      'state': state,
    });
    await settle(tester);
  }
}

Future<_Harness> _open(WidgetTester tester, {double width = 1500, bool russian = false}) async {
  final app = testBackend();
  addTearDown(app.dispose);
  await app.debugCreateUnlockedLocalProfile(name: 'Owned notice profile');
  final profile = app.services.profiles.currentProfiles.active!.id.value;
  final events = JobNoticeEventAdapter(), jobs = _Jobs();
  events.acceptScope(originalEpoch: events.epoch, profileId: profile, unlocked: true);
  addTearDown(events.dispose);
  if (russian) {
    final settings = app.services.settings;
    await settings.updateLocal(settings.currentLocal.copyWith(themeMode: AppThemeMode.dark));
  }
  await pumpApp(
    tester,
    app,
    size: Size(width, 1100),
    locale: russian ? AppLocale.ru : AppLocale.en,
    captureBoundary: true,
    overrides: [automationServiceProvider.overrideWithValue(jobs), criticalJobEventsProvider.overrideWithValue(events)],
  );
  final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
  container.read(routerProvider).go(AppRoutes.terminal);
  await settle(tester);
  final owner = container.read(criticalNoticeOwnerProvider);
  addTearDown(() {
    owner?.clear();
  });
  await container.read(navigationPreferencesControllerProvider).setVisible(profile, 'automation', false);
  await settle(tester);
  return _Harness(container, app, events, jobs, profile);
}

Future<void> _tap(WidgetTester tester, String key) async {
  final finder = find.byKey(ValueKey(key));
  await tester.ensureVisible(finder);
  await tester.pump();
  await tester.tap(finder);
  await settle(tester);
}

void main() {
  testWidgets(
    'hidden job completion arrives on existing push without polling and opens exact old output without replay or nav repin',
    (tester) async {
      final h = await _open(tester);
      expect(h.jobs.reads, 0);
      await h.emit(tester, revision: 2);
      expect(h.jobs.reads, 0);
      final owner = h.container.read(criticalNoticeOwnerProvider)!;
      expect(owner.state.unread, 1);
      await _tap(tester, 'critical-notices-open');
      await _tap(tester, 'critical-notice-open-job:$jobId');
      expect(h.container.read(routerProvider).routeInformationProvider.value.uri.path, AppRoutes.automation);
      expect(tester.widget<AutomationExecutionConsole>(find.byType(AutomationExecutionConsole)).jobId, jobId);
      expect(h.container.read(navigationPreferencesProvider)!.visible('automation'), isFalse);
      expect(h.jobs.forbidden, isEmpty);
      expect(tester.takeException(), isNull);
    },
  );
  testWidgets(
    'lagged is refresh-needed with no fake completion; explicit Refresh reads once and keeps centre metadata-only',
    (tester) async {
      final h = await _open(tester);
      h.events.ingest({'type': 'lagged'});
      await settle(tester);
      expect(h.container.read(criticalNoticeOwnerProvider)!.state.rows, isEmpty);
      expect(h.jobs.reads, 0);
      await _tap(tester, 'critical-notices-open');
      expect(find.byKey(const ValueKey('critical-notices-gap')), findsOneWidget);
      await _tap(tester, 'critical-notices-refresh');
      expect(h.jobs.reads, 1);
      expect(h.container.read(criticalNoticeOwnerProvider)!.state.rows.single.job!.change.jobId, jobId);
      expect(h.jobs.forbidden, isEmpty);
      expect(tester.takeException(), isNull);
    },
  );
  testWidgets('lock clears open centre and rejects a late explicit refresh without opening any section', (
    tester,
  ) async {
    final h = await _open(tester);
    await h.emit(tester);
    await _tap(tester, 'critical-notices-open');
    final owner = h.container.read(criticalNoticeOwnerProvider)!;
    h.jobs.pending = Completer<AutomationJson>();
    await _tap(tester, 'critical-notices-refresh');
    h.events.boundary();
    await h.container.read(vaultServiceProvider).lock();
    await settle(tester);
    h.jobs.pending!.complete({'schema_version': 1, 'read_only': false, 'jobs': <Object?>[]});
    await settle(tester);
    expect(owner.state.rows, isEmpty);
    expect(find.byType(CriticalNoticePanel), findsNothing);
    expect(h.jobs.forbidden, isEmpty);
    expect(tester.takeException(), isNull);
  });
  testWidgets('critical attention stays available in desktop terminal minimal mode without focus theft', (
    tester,
  ) async {
    final h = await _open(tester);
    h.container.read(terminalMinimalModeProvider.notifier).toggle();
    await settle(tester);
    final focus = FocusManager.instance.primaryFocus;
    await h.emit(tester);
    expect(h.container.read(routerProvider).routeInformationProvider.value.uri.path, AppRoutes.terminal);
    expect(FocusManager.instance.primaryFocus, focus);
    expect(find.byKey(const ValueKey('critical-notices-attention')), findsOneWidget);
    final l = tester.element(find.byKey(const ValueKey('critical-notices-attention'))).l10n;
    await tester.tap(
      find.descendant(
        of: find.byKey(const ValueKey('critical-notices-attention')),
        matching: find.text(l.noticesTitle),
      ),
    );
    await settle(tester);
    expect(find.byType(CriticalNoticePanel), findsOneWidget);
    expect(h.jobs.reads, 0);
    expect(h.jobs.forbidden, isEmpty);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  testWidgets('new notification during centre refresh preserves expanded technical row and does not replay execution', (
    tester,
  ) async {
    final h = await _open(tester);
    await h.emit(tester);
    await _tap(tester, 'critical-notices-open');
    await _tap(tester, 'critical-notice-details-job:$jobId');
    h.jobs.pending = Completer<AutomationJson>();
    await _tap(tester, 'critical-notices-refresh');
    await h.emit(tester, revision: 2, state: 'acknowledged');
    h.jobs.pending!.complete({'schema_version': 1, 'read_only': false, 'jobs': <Object?>[]});
    await settle(tester);
    final l = tester.element(find.byType(CriticalNoticePanel)).l10n;
    expect(
      find.byWidgetPredicate((widget) => widget is SelectableText && widget.data == '${l.noticesJobId}: $jobId'),
      findsOneWidget,
    );
    expect(h.jobs.forbidden, isEmpty);
    expect(tester.takeException(), isNull);
  });
  testWidgets(
    'profile switch closes centre, clears old snapshot, and late old-profile events do not enter the new profile',
    (tester) async {
      final h = await _open(tester);
      await h.emit(tester);
      await _tap(tester, 'critical-notices-open');
      final old = h.container.read(criticalNoticeOwnerProvider)!;
      h.events.boundary();
      await h.app.debugCreateUnlockedLocalProfile(name: 'Other owned notice profile');
      await settle(tester);
      final next = h.app.services.profiles.currentProfiles.active!.id.value;
      h.events.acceptScope(originalEpoch: h.events.epoch, profileId: next, unlocked: true);
      await settle(tester);
      h.events.ingest({
        'type': 'automation_job_changed',
        'profile_id': h.profile,
        'job_id': jobId,
        'revision': 2,
        'state': 'succeeded',
      });
      await settle(tester);
      expect(old.state.rows, isEmpty);
      expect(find.byType(CriticalNoticePanel), findsNothing);
      expect(h.container.read(criticalNoticeOwnerProvider)!.state.rows, isEmpty);
      expect(h.jobs.reads, 0);
      expect(h.jobs.forbidden, isEmpty);
      expect(tester.takeException(), isNull);
    },
  );
  testWidgets('a scope-reset one-shot intent cannot open old output after the user enters Automation', (tester) async {
    final h = await _open(tester);
    await h.emit(tester);
    final owner = h.container.read(criticalNoticeOwnerProvider)!;
    h.container.read(criticalNoticeOpenIntentProvider.notifier).choose(owner, owner.state.rows.single.id);
    h.events.boundary();
    await settle(tester);
    h.container.read(routerProvider).go(AppRoutes.automation);
    await settle(tester);
    expect(find.byType(AutomationExecutionConsole), findsNothing);
    expect(h.jobs.reads, 0);
    expect(h.jobs.forbidden, isEmpty);
    expect(tester.takeException(), isNull);
  });
  testWidgets('mobile critical notice is available while the tool overlay hides the normal app bar', (tester) async {
    final h = await _open(tester, width: 430);
    expect(
      tester.getRect(find.byKey(const ValueKey('critical-notices-open'))).left,
      greaterThanOrEqualTo(tester.getRect(find.byKey(const ValueKey('mobile-ai'))).right),
      reason: 'notifications preserve the existing mobile primary-tool order',
    );
    h.container.read(workspaceToolsProvider.notifier).open(WorkspaceTool.notes);
    await settle(tester);
    await h.emit(tester);
    expect(find.byKey(const ValueKey('critical-notices-attention')), findsOneWidget);
    final l = tester.element(find.byKey(const ValueKey('critical-notices-attention'))).l10n;
    await tester.tap(
      find.descendant(
        of: find.byKey(const ValueKey('critical-notices-attention')),
        matching: find.text(l.noticesTitle),
      ),
    );
    await settle(tester);
    expect(find.byType(CriticalNoticePanel), findsOneWidget);
    expect(h.jobs.reads, 0);
    expect(h.jobs.forbidden, isEmpty);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.android}));
  testWidgets('notification actions remain bounded on a narrow enlarged-text layout', (tester) async {
    final h = await _open(tester, width: 580);
    tester.platformDispatcher.textScaleFactorTestValue = 1.6;
    addTearDown(tester.platformDispatcher.clearTextScaleFactorTestValue);
    await h.emit(tester);
    await _tap(tester, 'critical-notices-open');
    expect(find.byKey(const ValueKey('critical-notice-open-job:$jobId')), findsOneWidget);
    expect(h.jobs.forbidden, isEmpty);
    expect(tester.takeException(), isNull);
  });
  for (final width in [580.0, 1500.0]) {
    testWidgets('Russian notification centre readable at width $width', (tester) async {
      await loadAutomationProofFonts(tester);
      final h = await _open(tester, width: width, russian: true);
      await h.emit(tester);
      await _tap(tester, 'critical-notices-open');
      await captureAutomationWorkspace(tester, 'critical-notices-${width < 800 ? 'narrow' : 'wide'}-ru');
      expect(h.jobs.reads, 0);
      expect(h.jobs.forbidden, isEmpty);
      expect(tester.takeException(), isNull);
    }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  }
}
