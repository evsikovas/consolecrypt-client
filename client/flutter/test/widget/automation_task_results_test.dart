import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/automation/automation_event_view.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_ui_proof.dart';
import '../helpers/test_app.dart' show settle, tapKey;

const jobId = '11111111-1111-4111-8111-111111111111';
final digest = 'a' * 64;
AutomationJson job() => {
  'job_id': jobId,
  'manifest_digest': digest,
  'manifest': {
    'targets': [
      {'alias': 'cc_a'},
      {'alias': 'cc_b'},
    ],
  },
};
AutomationJson event(
  int seq, {
  String type = 'runner_on_ok',
  String host = 'cc_a',
  String task = 'Same task name',
  String taskId = '22222222-2222-4222-8222-222222222222',
  AutomationJson? result,
}) => {
  'version': 1,
  'job_id': jobId,
  'sequence': seq,
  'timestamp': '2026-11-13T12:00:00Z',
  'type': type,
  'payload': {
    'host': host,
    'task': task,
    'task_uuid': taskId,
    'task_action': 'ansible.builtin.command',
    'res': result ?? {'changed': false, 'rc': 0},
  },
};
AutomationJson page(int after, List<AutomationJson> events) => {
  'job_id': jobId,
  'manifest_digest': digest,
  'events': events,
  'cursor': events.isEmpty ? after : events.last['sequence'],
  'output_truncated': false,
  'state': 'running',
};

class Events implements AutomationEventService {
  final calls = <(int, int)>[];
  Completer<AutomationJson>? pending;
  bool disconnected = false;
  AutomationJson Function(int) response = (after) => after == 0
      ? page(0, [
          event(1),
          event(
            2,
            type: 'runner_on_failed',
            host: 'cc_b',
            result: {'changed': true, 'rc': 7, 'msg': 'Synthetic failure'},
          ),
        ])
      : page(after, []);
  @override
  Future<AutomationJson> jobEvents(
    String id, {
    required int after,
    int limit = 50,
  }) async {
    expect(id, jobId);
    calls.add((after, limit));
    if (disconnected) throw StateError('DO_NOT_RENDER_PRIVATE_TRANSPORT_ERROR');
    return pending?.future ?? response(after);
  }
}

final defaultScope = Object();
Future<void> open(
  WidgetTester tester,
  Events api, {
  Object? scope,
  bool active = true,
  bool Function()? current,
  AutomationJson? record,
  Locale locale = const Locale('en'),
  double width = 1000,
  double scale = 1,
}) async {
  await tester.binding.setSurfaceSize(Size(width, 1000));
  addTearDown(() => tester.binding.setSurfaceSize(null));
  await loadAutomationProofFonts(tester);
  await tester.pumpWidget(
    MaterialApp(
      locale: locale,
      theme: AppTheme.dark(platform: TargetPlatform.windows),
      supportedLocales: AppLocalizations.supportedLocales,
      localizationsDelegates: const [
        AppLocalizations.delegate,
        ...GlobalMaterialLocalizations.delegates,
      ],
      builder: (context, child) => RepaintBoundary(
        key: const ValueKey('automation-proof-root'),
        child: MediaQuery(
          data: MediaQuery.of(context)
              .copyWith(textScaler: TextScaler.linear(scale)),
          child: child!,
        ),
      ),
      home: Scaffold(
        body: SingleChildScrollView(
          child: AutomationTaskResults(
            service: api,
            job: record ?? job(),
            scopeIdentity: scope ?? defaultScope,
            active: active,
            isCurrent: current ?? () => true,
          ),
        ),
      ),
    ),
  );
  await settle(tester);
}

Future<void> expand(WidgetTester tester) =>
    tapKey(tester, 'automation-task-results-$jobId');

void main() {
  test('projection preserves separate real event/task/host identities, changed and rc without invented zeros', () {
    final rows = automationTaskResultPage(
      page(0, [
        event(1),
        event(2, host: 'cc_b'),
        event(3, taskId: '33333333-3333-4333-8333-333333333333', result: {}),
      ]),
      jobId: jobId,
      digest: digest,
      after: 0,
      aliases: automationApprovedAliases(job()),
    ).rows;
    expect(rows, hasLength(3));
    expect(rows.map((row) => row['sequence']), [1, 2, 3]);
    expect(rows.map((row) => row['task_id']).toSet(), hasLength(2));
    expect(rows.first['changed'], isFalse);
    expect(rows.first['rc'], 0);
    expect(rows.last.containsKey('changed'), isFalse);
    expect(rows.last.containsKey('rc'), isFalse);
  });
  test('no_log and censored are applied before result fields; arbitrary fields and controls never survive projection', () {
    for (final censor in [
      {'censored': true},
      {'_ansible_no_log': true},
      {'censored': 'hidden'},
    ]) {
      final result = {
        'changed': true,
        'rc': 99,
        'msg': 'SECRET_RESULT',
        'stdout': 'SECRET_STDOUT',
        ...censor,
      };
      final rows = automationTaskResultPage(
        page(0, [event(1, type: 'runner_on_failed', result: result)]),
        jobId: jobId,
        digest: digest,
        after: 0,
        aliases: automationApprovedAliases(job()),
      ).rows;
      expect(rows.single['censored'], isTrue);
      expect(jsonEncode(rows), isNot(contains('SECRET')));
      expect(rows.single.containsKey('changed'), isFalse);
      expect(rows.single.containsKey('rc'), isFalse);
    }
    final projected = automationTaskResultPage(
      page(0, [
        event(
          1,
          type: 'runner_on_failed',
          task: '\x1b]52;c;INJECT\x07Task',
          result: {
            'msg': '\x1b[31mFailure\x1b[0m',
            'password': 'DO_NOT_PROJECT',
            'stdout': 'DO_NOT_PROJECT',
            'res': {'arbitrary': 'DO_NOT_PROJECT'},
          },
        ),
      ]),
      jobId: jobId,
      digest: digest,
      after: 0,
      aliases: automationApprovedAliases(job()),
    ).rows.single;
    expect(projected['task'], 'Task');
    expect(projected['error'], 'Failure');
    expect(jsonEncode(projected), isNot(contains('DO_NOT_PROJECT')));
  });
  test('foreign hosts are excluded; malformed or missing task UUID never merges by name', () {
    final legacy = event(1)
      ..['payload'] = {
        'host': 'cc_a',
        'task': 'Same',
        'res': <String, Object?>{},
      };
    final rows = automationTaskResultPage(
      page(0, [
        legacy,
        event(2, taskId: 'bad'),
        event(3, host: 'private_host'),
      ]),
      jobId: jobId,
      digest: digest,
      after: 0,
      aliases: automationApprovedAliases(job()),
    ).rows;
    expect(rows, hasLength(2));
    expect(rows.every((row) => row['task_id'] == null), isTrue);
    expect(rows.map((row) => row['sequence']), [1, 2]);
  });
  test('verified-page boundaries reject replay, gap, wrong job/digest, future version and oversized pages', () {
    final mutations = <AutomationJson>[
      page(0, [event(2)]),
      page(0, [event(1), event(1)]),
      page(0, [event(1)..['version'] = 2]),
      page(0, [event(1)..['job_id'] = 'foreign']),
      page(0, [event(1)])..['job_id'] = 'foreign',
      page(0, [event(1)])..['manifest_digest'] = 'b' * 64,
      page(0, [event(1)])..['cursor'] = 2,
      page(0, [for (var i = 1; i <= 51; i++) event(i)]),
    ];
    for (final invalid in mutations) {
      expect(
        () => automationTaskResultPage(
          invalid,
          jobId: jobId,
          digest: digest,
          after: 0,
          aliases: automationApprovedAliases(job()),
        ),
        throwsFormatException,
      );
    }
  });
  testWidgets(
    'screen opening never reads; explicit task expansion requests exactly one bounded page',
    (tester) async {
      final api = Events();
      await open(tester, api);
      expect(api.calls, isEmpty);
      await expand(tester);
      expect(api.calls, [(0, 50)]);
      expect(
        find.byKey(const ValueKey('automation-task-result-1')),
        findsOneWidget,
      );
      expect(
        find.byKey(const ValueKey('automation-task-result-2')),
        findsOneWidget,
      );
      await tester.enterText(
        find.byKey(const ValueKey('automation-task-host-filter')),
        'cc_b',
      );
      await settle(tester);
      expect(api.calls, hasLength(1));
      expect(
        find.byKey(const ValueKey('automation-task-result-1')),
        findsNothing,
      );
      expect(
        find.byKey(const ValueKey('automation-task-result-2')),
        findsOneWidget,
      );
      await tapKey(tester, 'automation-task-result-2');
      expect(find.text('Synthetic failure'), findsOneWidget);
    },
  );
  testWidgets(
    'cursor paging, reconnect retry and back navigation never reexecute or skip a failed page',
    (tester) async {
      final api = Events()
        ..response = (after) => after == 0
            ? page(0, [for (var i = 1; i <= 50; i++) event(i)])
            : after == 50
            ? page(50, [event(51), event(52)])
            : page(after, []);
      await open(tester, api);
      await expand(tester);
      api.disconnected = true;
      await tapKey(tester, 'automation-task-next');
      expect(api.calls.last, (50, 50));
      expect(
        find.byKey(const ValueKey('automation-task-read-error')),
        findsOneWidget,
      );
      expect(find.textContaining('DO_NOT_RENDER_PRIVATE'), findsNothing);
      api.disconnected = false;
      await tapKey(tester, 'automation-task-refresh');
      expect(api.calls, [(0, 50), (50, 50), (50, 50)]);
      expect(
        find.byKey(const ValueKey('automation-task-result-51')),
        findsOneWidget,
      );
      await tapKey(tester, 'automation-task-previous');
      expect(api.calls.last, (0, 50));
      expect(
        find.byKey(const ValueKey('automation-task-result-51')),
        findsNothing,
      );
      expect(
        find.byKey(const ValueKey('automation-task-result-1')),
        findsOneWidget,
      );
      expect(tester.widget<TextButton>(find.byKey(const ValueKey('automation-task-first'))).onPressed, isNull);
      await tapKey(tester, 'automation-task-next');
      expect(api.calls.last, (50, 50));
      await tapKey(tester, 'automation-task-first');
      expect(api.calls.last, (0, 50));
      expect(tester.widget<TextButton>(find.byKey(const ValueKey('automation-task-previous'))).onPressed, isNull);
    },
  );
  testWidgets(
    'ordinary parent polling preserves task expansion and local filter without another page request',
    (tester) async {
      final api = Events();
      await open(tester, api);
      await expand(tester);
      await tester.enterText(
        find.byKey(const ValueKey('automation-task-host-filter')),
        'cc_b',
      );
      await settle(tester);
      await tapKey(tester, 'automation-task-result-2');
      await open(tester, api, record: job());
      expect(find.text('Synthetic failure'), findsOneWidget);
      expect(
        tester
            .widget<TextField>(
              find.byKey(const ValueKey('automation-task-host-filter')),
            )
            .controller!
            .text,
        'cc_b',
      );
      expect(api.calls, hasLength(1));
    },
  );
  testWidgets(
    'original scope replacement rejects pending page before adopting a later unlocked profile',
    (tester) async {
      final api = Events()..pending = Completer<AutomationJson>();
      await open(tester, api, scope: Object());
      await tester.tap(
        find.byKey(const ValueKey('automation-task-results-$jobId')),
      );
      await tester.pump();
      expect(api.calls, hasLength(1));
      await open(tester, api, scope: Object());
      api.pending!.complete(page(0, [event(1, task: 'OLD_PROFILE_RESULT')]));
      await settle(tester);
      expect(find.textContaining('OLD_PROFILE_RESULT'), findsNothing);
      expect(
        find.byKey(const ValueKey('automation-task-result-1')),
        findsNothing,
      );
      expect(api.calls, hasLength(1));
    },
  );
  testWidgets(
    'stop waiting and section hide deny late pages without cancelling an accepted job',
    (tester) async {
      final api = Events()..pending = Completer<AutomationJson>();
      await open(tester, api);
      await tester.tap(
        find.byKey(const ValueKey('automation-task-results-$jobId')),
      );
      await tester.pump();
      await tester.pump(
        const Duration(milliseconds: 300),
      ); // expansion must expose a hittable Cancel control
      await tapKey(tester, 'automation-task-cancel-read');
      expect(
        tester
            .widget<TextButton>(
              find.byKey(const ValueKey('automation-task-refresh')),
            )
            .onPressed,
        isNull,
      );
      api.pending!.complete(page(0, [event(1, task: 'LATE_CANCELLED_WAIT')]));
      await settle(tester);
      expect(find.textContaining('LATE_CANCELLED_WAIT'), findsNothing);
      api.pending = Completer<AutomationJson>();
      await tester.tap(find.byKey(const ValueKey('automation-task-refresh')));
      await tester.pump();
      await open(tester, api, active: false);
      api.pending!.complete(page(0, [event(1, task: 'HIDDEN_RESULT')]));
      await open(tester, api, active: true);
      expect(find.textContaining('HIDDEN_RESULT'), findsNothing);
      expect(api.calls, hasLength(2));
    },
  );
  testWidgets(
    'censored module failures expose status but no result body or invented changed/exit code',
    (tester) async {
      final api = Events()
        ..response = (_) => page(0, [
          event(
            1,
            type: 'runner_on_failed',
            result: {
              '_ansible_no_log': true,
              'changed': true,
              'rc': 77,
              'msg': 'PRIVATE_NO_LOG',
              'stdout': 'PRIVATE_NO_LOG',
            },
          ),
        ]);
      await open(tester, api);
      await expand(tester);
      await tapKey(tester, 'automation-task-result-1');
      final l = tester.element(find.byType(AutomationTaskResults)).l10n;
      expect(find.text(l.automationEventCensored), findsOneWidget);
      expect(find.textContaining('PRIVATE_NO_LOG'), findsNothing);
      expect(
        find.byKey(const ValueKey('automation-task-error-1')),
        findsNothing,
      );
    },
  );
  testWidgets(
    'RU compact results wrap safely on narrow screen with enlarged text',
    (tester) async {
      final api = Events();
      await open(
        tester,
        api,
        locale: const Locale('ru'),
        width: 460,
        scale: 1.8,
      );
      await expand(tester);
      await tapKey(tester, 'automation-task-result-2');
      expect(tester.takeException(), isNull);
      await captureAutomationWorkspace(tester, 'run06-task-results-narrow-ru');
    },
  );
}
