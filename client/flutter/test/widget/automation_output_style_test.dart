import 'dart:convert';

import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/automation/automation_event_view.dart';
import 'package:consolecrypt/automation/automation_execution_console.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/monitoring/monitoring_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../support/automation_audit_fixture.dart' as audit;

class _CompletedConsoleApi extends Fake implements AutomationJobService, AutomationEventService {
  _CompletedConsoleApi(this.state);
  final String state;
  int catalogs = 0, pages = 0, cancelled = 0;
  @override
  Future<AutomationJson> jobs() async {
    catalogs++;
    return {
      'jobs': [
        {
          'job_id': 'completed',
          'state': state,
          'manifest': {
            'playbook': 'playbook.yml',
            'targets': [
              {'alias': 'approved', 'address': '192.0.2.1'},
            ],
          },
          'runner_result': {
            'result': {
              'exit_code': 0,
              'summary': {
                'stats': {
                  'ok': {'approved': 1},
                },
              },
            },
          },
        },
      ],
    };
  }

  @override
  Future<AutomationJson> jobEvents(String id, {required int after, int limit = 50}) async {
    pages++;
    return {'events': <AutomationJson>[], 'cursor': after, 'output_truncated': false, 'state': state};
  }

  @override
  Future<void> cancelJob(String id) async {
    cancelled++;
  }
}

class _AuditConsoleApi extends Fake implements AutomationJobService, AutomationEventService {
  int catalogs = 0, pages = 0;
  final row = audit.fixture(count: 30)
    ..['state'] = 'running'
    ..remove('runner_result');
  @override
  Future<AutomationJson> jobs() async {
    catalogs++;
    return {
      'jobs': [jsonDecode(jsonEncode(row)) as AutomationJson],
    };
  }

  @override
  Future<AutomationJson> jobEvents(String id, {required int after, int limit = 50}) async {
    pages++;
    return {
      'job_id': audit.jobId,
      'manifest_digest': 'a' * 64,
      'events': <AutomationJson>[],
      'cursor': after,
      'output_truncated': false,
      'state': 'running',
    };
  }
}

void main() {
  testWidgets(
    'recorded audit is available before results and refresh preserves technical tab and target page without extra read',
    (tester) async {
      final service = _AuditConsoleApi();
      const scope = AutomationScope(audit.profileId, true);
      await tester.binding.setSurfaceSize(const Size(900, 1000));
      addTearDown(() => tester.binding.setSurfaceSize(null));
      await tester.pumpWidget(
        ProviderScope(
          overrides: [
            automationScopeProvider.overrideWithValue(scope),
            monitoringScopeProvider.overrideWithValue(const MonitoringScope(null, false)),
            monitoringSessionProvider.overrideWithValue(null),
          ],
          child: MaterialApp(
            locale: const Locale('en'),
            supportedLocales: AppLocalizations.supportedLocales,
            localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
            home: Scaffold(
              body: AutomationExecutionConsole(
                scope: scope,
                service: service,
                jobId: audit.jobId,
                collapsed: false,
                onToggle: () {},
                onHistory: () {},
              ),
            ),
          ),
        ),
      );
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 100));
      final technical = find.byKey(const ValueKey('automation-console-technical-tab'));
      await tester.tap(technical);
      await tester.pump();
      final section = find.byKey(const ValueKey('automation-audit-${audit.jobId}'));
      await tester.ensureVisible(section);
      await tester.tap(find.text('Admission audit'));
      await tester.pumpAndSettle();
      expect(find.byKey(ValueKey('audit-target-${audit.hostId(0)}')), findsOneWidget);
      expect(find.byKey(ValueKey('audit-target-${audit.hostId(25)}')), findsNothing);
      final next = find.byKey(const ValueKey('audit-next-${audit.jobId}'));
      final list = find.byKey(const ValueKey('automation-console-technical-scroll-${audit.jobId}'));
      final scroll = tester.widget<ListView>(list).controller!;
      scroll.jumpTo(scroll.position.maxScrollExtent);
      await tester.pump();
      expect(next.hitTestable(), findsOneWidget);
      final calls = (service.catalogs, service.pages);
      await tester.tap(next);
      await tester.pump();
      expect((service.catalogs, service.pages), calls);
      expect(find.byKey(ValueKey('audit-target-${audit.hostId(25)}')), findsOneWidget);
      await tester.pump(const Duration(seconds: 3));
      await tester.pump();
      expect(service.catalogs, greaterThan(calls.$1));
      expect(tester.widget<GlassButton>(technical).style, GlassButtonStyle.prominent);
      expect(find.byKey(ValueKey('audit-target-${audit.hostId(25)}')), findsOneWidget);
      expect(find.textContaining('HIDDEN_'), findsNothing);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox());
    },
  );

  for (final width in [360.0, 1000.0]) {
    testWidgets('console header stays compact at width $width and keeps selected tabs after layout and polling', (
      tester,
    ) async {
      await tester.binding.setSurfaceSize(Size(width, 420));
      addTearDown(() => tester.binding.setSurfaceSize(null));
      final service = _CompletedConsoleApi('running');
      const scope = AutomationScope('fixture-profile', true);
      await tester.pumpWidget(
        ProviderScope(
          overrides: [
            automationScopeProvider.overrideWithValue(scope),
            monitoringScopeProvider.overrideWithValue(const MonitoringScope(null, false)),
            monitoringSessionProvider.overrideWithValue(null),
          ],
          child: MaterialApp(
            theme: AppTheme.dark(platform: TargetPlatform.windows),
            locale: const Locale('ru'),
            supportedLocales: AppLocalizations.supportedLocales,
            localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
            home: Scaffold(
              body: SizedBox(
                height: 300,
                child: AutomationExecutionConsole(
                  scope: scope,
                  service: service,
                  jobId: 'completed',
                  collapsed: false,
                  onToggle: () {},
                  onHistory: () {},
                ),
              ),
            ),
          ),
        ),
      );
      for (var i = 0; i < 8; i++) {
        await tester.pump(const Duration(milliseconds: 100));
      }
      final header = find.byKey(const ValueKey('automation-console-header'));
      expect(tester.getSize(header).height, lessThanOrEqualTo(width >= 760 ? 36 : 64));
      final output = find.byKey(const ValueKey('automation-console-output-tab'));
      final technical = find.byKey(const ValueKey('automation-console-technical-tab'));
      expect(
        tester.getRect(technical).overlaps(tester.getRect(find.byKey(const ValueKey('automation-console-history')))),
        false,
      );
      expect(find.textContaining('Журнал задач, только чтение'), findsNothing);
      expect(find.byTooltip('Следить за журналом задач'), findsOneWidget);
      await tester.tap(technical);
      await tester.pump();
      expect(tester.widget<GlassButton>(technical).style, GlassButtonStyle.prominent);
      expect(tester.widget<GlassButton>(output).style, GlassButtonStyle.plain);
      await tester.pump(const Duration(seconds: 5));
      await tester.pump();
      expect(tester.widget<GlassButton>(technical).style, GlassButtonStyle.prominent);
      await tester.binding.setSurfaceSize(Size(width >= 760 ? 360 : 1000, 420));
      await tester.pump();
      expect(tester.widget<GlassButton>(technical).style, GlassButtonStyle.prominent);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox.shrink());
      await tester.pump();
    });
  }
  for (final state in ['succeeded', 'failed']) {
    testWidgets('completed $state console hides follow stops drained polling and exposes view-only close callback', (
      tester,
    ) async {
      final service = _CompletedConsoleApi(state);
      var closed = 0;
      const scope = AutomationScope('fixture-profile', true);
      await tester.pumpWidget(
        ProviderScope(
          overrides: [
            automationScopeProvider.overrideWithValue(scope),
            monitoringScopeProvider.overrideWithValue(const MonitoringScope(null, false)),
            monitoringSessionProvider.overrideWithValue(null),
          ],
          child: MaterialApp(
            theme: AppTheme.dark(platform: TargetPlatform.windows),
            locale: const Locale('en'),
            supportedLocales: AppLocalizations.supportedLocales,
            localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
            home: Scaffold(
              body: SizedBox(
                height: 300,
                child: AutomationExecutionConsole(
                  scope: scope,
                  service: service,
                  jobId: 'completed',
                  collapsed: false,
                  onToggle: () {},
                  onHistory: () {},
                  onClose: () {
                    closed++;
                  },
                ),
              ),
            ),
          ),
        ),
      );
      for (var i = 0; i < 8; i++) {
        await tester.pump(const Duration(milliseconds: 100));
      }
      expect(find.byKey(const ValueKey('automation-console-follow')), findsNothing);
      expect(find.byKey(const ValueKey('automation-console-close')), findsOneWidget);
      final catalogs = service.catalogs, pages = service.pages;
      await tester.pump(const Duration(seconds: 5));
      await tester.pump();
      expect(service.catalogs, catalogs);
      expect(service.pages, pages);
      await tester.tap(find.byKey(const ValueKey('automation-console-close')));
      await tester.pump();
      expect(closed, 1);
      expect(service.cancelled, 0);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox.shrink());
      await tester.pump();
    });
  }

  testWidgets('Output context colors follow native event kind, not task text or ANSI', (tester) async {
    final events = [
      {
        'sequence': 1,
        'type': 'playbook_on_play_start',
        'payload': {'play': 'Fixture play'},
      },
      {
        'sequence': 2,
        'type': 'playbook_on_task_start',
        'payload': {'task': 'failed is just a task label'},
      },
      {
        'sequence': 3,
        'type': 'runner_on_ok',
        'payload': {
          'host': 'approved',
          'task': 'Unchanged',
          'res': {'changed': false},
        },
      },
      {
        'sequence': 4,
        'type': 'runner_on_ok',
        'payload': {
          'host': 'approved',
          'task': 'Updated',
          'res': {'changed': true},
        },
      },
      {
        'sequence': 5,
        'type': 'runner_on_skipped',
        'payload': {'host': 'approved', 'task': 'Skipped'},
      },
      {
        'sequence': 6,
        'type': 'runner_on_failed',
        'payload': {'host': 'approved', 'task': 'Failure'},
      },
      {
        'sequence': 7,
        'type': 'runner_on_unreachable',
        'payload': {'host': 'approved', 'task': 'Offline'},
      },
      {
        'sequence': 8,
        'type': 'runner_summary',
        'payload': {
          'stats': {
            'ok': {'approved': 0},
          },
        },
      },
    ];
    await tester.pumpWidget(
      MaterialApp(
        theme: AppTheme.dark(platform: TargetPlatform.windows),
        locale: const Locale('en'),
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        supportedLocales: AppLocalizations.supportedLocales,
        home: Scaffold(
          body: SingleChildScrollView(
            child: AutomationHumanEvents(
              events: events,
              job: const {
                'manifest': {
                  'targets': [
                    {'alias': 'approved', 'address': '192.0.2.1'},
                  ],
                },
              },
              technical: false,
            ),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    final palette = GlassTokens.of(tester.element(find.byType(AutomationHumanEvents))).palette;
    Color? color(String text) => tester
        .widget<SelectableText>(find.byWidgetPredicate((w) => w is SelectableText && w.data == text))
        .style
        ?.color;
    expect(color('PLAY [Fixture play]'), palette.accent);
    expect(color('TASK [failed is just a task label]'), palette.accent);
    expect(color('ok [192.0.2.1] — Unchanged'), palette.success);
    expect(color('changed [192.0.2.1] — Updated'), palette.warning);
    expect(color('skipped [192.0.2.1] — Skipped'), palette.secondary);
    expect(color('failed [192.0.2.1] — Failure'), palette.danger);
    expect(color('unreachable [192.0.2.1] — Offline'), palette.danger);
    expect(color('PLAY RECAP'), palette.info);
    expect(find.textContaining('ok=0'), findsOneWidget);
    expect(find.textContaining('changed=—'), findsOneWidget);
    expect(find.text('Technical event details'), findsNothing);
    expect(tester.takeException(), isNull);
  });
}
