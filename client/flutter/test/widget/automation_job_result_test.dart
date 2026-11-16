import 'package:consolecrypt/automation/automation_job_result.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../support/automation_audit_fixture.dart' as audit;

AutomationJson fixture({int count = 2}) => {
  'job_id': 'result-fixture',
  'manifest': {
    'environment_digest': 'approved-environment',
    'targets': [
      for (var i = 0; i < count; i++) {'alias': 'node-${i.toString().padLeft(3, '0')}'},
    ],
  },
  'runner_result': {
    'environment': {
      'version': 1,
      'environment_digest': 'approved-environment',
      'identity': {
        'system': 'Linux',
        'machine': 'x86_64',
        'python': '3.12.3',
        'helper': '0.1.0',
        'packages': {
          'ansible-core': {'actual': '2.20.11'},
        },
      },
    },
    'result': {
      'exit_code': 2,
      'remote_changes_may_be_partial': true,
      'summary': {
        'stats': {
          'ok': {'node-000': 2, 'unapproved-alias': 99},
          'changed': {'node-000': 0},
          'dark': {'node-000': -1},
          'failures': {'node-001': 1.5},
        },
      },
    },
  },
};

Future<void> pumpResult(WidgetTester tester, AutomationJson job) async {
  tester.view.physicalSize = const Size(360, 720);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    MaterialApp(
      locale: const Locale('en'),
      supportedLocales: AppLocalizations.supportedLocales,
      localizationsDelegates: AppLocalizations.localizationsDelegates,
      home: Scaffold(
        body: SingleChildScrollView(child: AutomationJobResult(job: job)),
      ),
    ),
  );
  await tester.pumpAndSettle();
}

Future<void> expand(WidgetTester tester, String title) async {
  await tester.ensureVisible(find.text(title));
  await tester.tap(find.text(title));
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('Native control-loss envelope exposes partial warning without inventing host outcome or exit code', (
    tester,
  ) async {
    final job = fixture()..['state'] = 'unknown';
    // Exact unknown_control/change_state projection in automation_jobs.rs:
    // the Native observation is directly in row.runner_result, not .result.
    job['runner_result'] = {
      'reason': 'control_channel_or_storage_lost',
      'control_failure': 'connection_lost',
      'remote_changes_may_be_partial': true,
    };
    await pumpResult(tester, job);
    final banner = find.byKey(const ValueKey('automation-result-partial-result-fixture'));
    expect(banner, findsOneWidget);
    expect(tester.widget<InfoBanner>(banner).tone, BannerTone.warning);
    expect(find.textContaining('does not roll them back'), findsOneWidget);
    expect(find.textContaining('Exit code:'), findsNothing);
    expect(find.textContaining('connection_lost'), findsNothing);
    await expand(tester, 'Results for approved hosts');
    expect(find.text('Changed: no data'), findsNWidgets(2));
    expect(find.text('OK: no data'), findsNWidgets(2));
    expect(find.text('Changed: 0'), findsNothing);
    expect(find.text('OK: 0'), findsNothing);
    expect(tester.takeException(), isNull);
  });
  testWidgets('Native pre-dispatch interruption and malformed partial flags do not invent completed changes', (
    tester,
  ) async {
    for (final flag in [false, 'true']) {
      final job = fixture()..['state'] = 'interrupted';
      job['runner_result'] = {
        'reason': 'vault_material_unavailable_before_dispatch',
        'remote_changes_may_be_partial': flag,
      };
      await tester.pumpWidget(const SizedBox());
      await pumpResult(tester, job);
      expect(find.byKey(const ValueKey('automation-result-partial-result-fixture')), findsNothing);
      expect(find.textContaining('Exit code:'), findsNothing);
      await expand(tester, 'Results for approved hosts');
      expect(find.text('Changed: no data'), findsNWidgets(2));
      expect(find.text('Changed: 0'), findsNothing);
      expect(tester.takeException(), isNull);
    }
  });

  testWidgets('approved targets only; missing, invalid and zero counts stay distinct on a narrow screen', (
    tester,
  ) async {
    await pumpResult(tester, fixture());
    expect(find.textContaining('does not roll them back'), findsOneWidget);
    expect(find.byType(InfoBanner), findsOneWidget);
    await expand(tester, 'Results for approved hosts');
    expect(find.text('node-000'), findsOneWidget);
    expect(find.text('node-001'), findsOneWidget);
    expect(find.textContaining('unapproved-alias'), findsNothing);
    expect(find.text('OK: 2'), findsOneWidget);
    expect(find.text('Changed: 0'), findsOneWidget);
    expect(find.text('OK: no data'), findsOneWidget);
    expect(find.text('Unreachable: no data'), findsNWidgets(2));
    expect(find.text('Failed: no data'), findsNWidgets(2));
    expect(tester.takeException(), isNull);
  });

  testWidgets('historical environment must match the approved digest and supported receipt version', (tester) async {
    final job = fixture();
    await pumpResult(tester, job);
    await expand(tester, 'Execution environment versions');
    expect(find.text('Python: 3.12.3'), findsOneWidget);
    expect(find.text('ansible-core: 2.20.11'), findsOneWidget);
    final receipt = (job['runner_result'] as AutomationJson)['environment'] as AutomationJson;
    receipt['environment_digest'] = 'unrelated';
    await tester.pumpWidget(const SizedBox());
    await pumpResult(tester, job);
    await expand(tester, 'Execution environment versions');
    expect(find.text('Python: 3.12.3'), findsNothing);
    expect(find.textContaining('Current versions do not replace historical evidence'), findsOneWidget);
    receipt['environment_digest'] = 'approved-environment';
    receipt['version'] = 2;
    await tester.pumpWidget(const SizedBox());
    await pumpResult(tester, job);
    await expand(tester, 'Execution environment versions');
    expect(find.text('Python: 3.12.3'), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('host results are paged and legacy technical output is bounded and collapsed', (tester) async {
    await pumpResult(tester, fixture(count: 60));
    await expand(tester, 'Results for approved hosts');
    expect(find.text('node-000'), findsOneWidget);
    expect(find.text('node-025'), findsNothing);
    final next = find.byKey(const ValueKey('automation-result-next'));
    await tester.ensureVisible(next);
    await tester.tap(next);
    await tester.pumpAndSettle();
    expect(find.text('node-000'), findsNothing);
    expect(find.text('node-025'), findsOneWidget);
    await tester.pumpWidget(const SizedBox());
    await pumpResult(tester, {
      'job_id': 'legacy',
      'runner_result': {'output': 'X' * 20000},
    });
    expect(find.textContaining('XXXXXXXX'), findsNothing);
    await expand(tester, 'Result technical details');
    final previews = tester.widgetList<SelectableText>(find.byType(SelectableText));
    expect(previews.single.data!.length, lessThanOrEqualTo(16385));
    expect(find.textContaining('XXXXXXXX'), findsNothing);
    await expand(tester, 'Results for approved hosts');
    expect(find.textContaining('legacy record has no approved alias list'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });
  testWidgets('technical details withhold arbitrary legacy values and preserve exact recorded unknown flags/time', (
    tester,
  ) async {
    final row = audit.fixture(), raw = row['runner_result'] as AutomationJson;
    raw['credential_refs'] = ['RAW_CREDENTIAL_CANARY'];
    raw['legacy_private'] = 'RAW_SECRET_CANARY';
    (raw['result'] as AutomationJson)['stdout'] = 'RAW_OUTPUT_CANARY';
    await pumpResult(tester, row);
    await expand(tester, 'Result technical details');
    expect(find.textContaining('CANARY'), findsNothing);
    expect(find.textContaining('"exit_code": -15'), findsOneWidget);
    expect(find.textContaining('"timed_out": false'), findsOneWidget);
    expect(find.textContaining('"remote_changes_may_be_partial": true'), findsOneWidget);
    expect(find.textContaining('2023-11-14T22:13:22Z'), findsOneWidget);
    expect(find.textContaining('succeeded'), findsNothing);
    expect(tester.takeException(), isNull);
  });
  testWidgets('no_log result and summary are not traversed even after explicit technical expansion', (tester) async {
    final row = audit.fixture(), raw = row['runner_result'] as AutomationJson, result = raw['result'] as AutomationJson;
    result['summary'] = {
      '_ansible_no_log': true,
      'stats': {
        'ok': {'cc_0': 987},
      },
      'raw': 'NOLOG_CANARY',
    };
    await pumpResult(tester, row);
    await expand(tester, 'Results for approved hosts');
    expect(find.text('OK: 987'), findsNothing);
    expect(find.text('OK: no data'), findsNWidgets(2));
    await expand(tester, 'Result technical details');
    expect(find.textContaining('NOLOG_CANARY'), findsNothing);
    expect(find.textContaining('"exit_code": -15'), findsOneWidget);
    result['censored'] = true;
    await tester.pumpWidget(const SizedBox());
    await pumpResult(tester, row);
    await expand(tester, 'Result technical details');
    expect(find.textContaining('"exit_code"'), findsNothing);
    expect(find.textContaining('NOLOG_CANARY'), findsNothing);
    expect(tester.takeException(), isNull);
  });
  testWidgets('environment expansion withholds legacy secret text at matching digest', (tester) async {
    final job = audit.fixture(),
        raw = job['runner_result'] as AutomationJson,
        identity = (raw['environment'] as AutomationJson)['identity'] as AutomationJson;
    identity['python'] = 'KNOWN_LEGACY_SECRET';
    identity['helper'] = 'KNOWN_LEGACY_SECRET';
    identity['packages'] = {
      'ansible-core': {'actual': 'KNOWN_LEGACY_SECRET'},
    };
    await pumpResult(tester, job);
    await expand(tester, 'Execution environment versions');
    expect(find.textContaining('KNOWN_LEGACY_SECRET'), findsNothing);
    expect(find.textContaining('Linux'), findsOneWidget);
    expect(find.textContaining('x86_64'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });
}
