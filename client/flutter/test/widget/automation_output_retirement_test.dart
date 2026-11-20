import 'dart:async';

import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/automation/automation_execution_console.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

const a = '11111111-1111-4111-8111-111111111111', b = '22222222-2222-4222-8222-222222222222';
const scope = AutomationScope('original-profile', true);
String digest(String id) => (id == a ? 'a' : 'b') * 64;
AutomationJson row(String id) => {
  'job_id': id,
  'manifest_digest': digest(id),
  'state': 'unknown',
  'manifest': {
    'playbook': id == a ? 'A.yml' : 'B.yml',
    'targets': [
      {'alias': 'approved', 'address': '192.0.2.1'},
    ],
  },
};
AutomationJson page(String id, int after, {bool retired = false}) => {
  'job_id': id,
  'manifest_digest': digest(id),
  'events': <AutomationJson>[],
  'cursor': after,
  'output_truncated': false,
  'state': 'unknown',
  if (retired)
    'artifact_retirement': {'version': 1, 'retired_at': 1800000000, 'last_sequence': 9, 'output_bytes': 1024},
};

class Api extends Fake implements AutomationJobService, AutomationEventService {
  final requests = <(String, int)>[];
  AutomationJson Function(String, int)? reply;
  Completer<AutomationJson>? pending;
  @override
  Future<AutomationJson> jobs() async => {
    'jobs': [row(a), row(b)],
  };
  @override
  Future<AutomationJson> jobEvents(String id, {required int after, int limit = 50}) async {
    requests.add((id, after));
    if (id == a && after == 1 && pending != null) return pending!.future;
    if (reply != null) return reply!(id, after);
    if (id == a && after == 0) {
      return page(id, 0)
        ..['cursor'] = 1
        ..['events'] = [
          {
            'version': 1,
            'job_id': id,
            'sequence': 1,
            'timestamp': '2026-11-13T12:00:00Z',
            'type': 'runner_on_ok',
            'payload': {
              'host': 'approved',
              'task': 'Saved details A',
              'res': {'changed': false},
            },
          },
        ];
    }
    return page(id, after, retired: id == a);
  }
}

Widget host(Api service, {String id = a}) => ProviderScope(
  overrides: [automationScopeProvider.overrideWithValue(scope)],
  child: MaterialApp(
    theme: AppTheme.dark(platform: TargetPlatform.windows),
    locale: const Locale('en'),
    supportedLocales: AppLocalizations.supportedLocales,
    localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
    home: Scaffold(
      body: SizedBox(
        height: 390,
        child: AutomationExecutionConsole(
          key: const ValueKey('same-console'),
          scope: scope,
          service: service,
          jobId: id,
          collapsed: false,
          onToggle: () {},
          onHistory: () {},
        ),
      ),
    ),
  ),
);
Future<void> settle(WidgetTester tester) async {
  for (var i = 0; i < 6; i++) {
    await tester.pump(const Duration(milliseconds: 100));
  }
}

void main() {
  testWidgets('actual page refresh preserves retirement notice original cursor and selected technical tab', (
    tester,
  ) async {
    final api = Api();
    await tester.pumpWidget(host(api));
    await settle(tester);
    expect(api.requests, [(a, 0)]);
    await tester.tap(find.byKey(const ValueKey('automation-console-technical-tab')));
    await tester.pump();
    await tester.pump(const Duration(seconds: 2));
    await settle(tester);
    expect(find.byKey(const ValueKey('automation-retired-technical')), findsOneWidget);
    expect(find.textContaining('The source reports that detailed output was removed'), findsWidgets);
    api.reply = (id, after) => page(id, after);
    await tester.pump(const Duration(seconds: 2));
    await settle(tester);
    expect(api.requests.where((r) => r.$1 == a).map((r) => r.$2), [0, 1, 1]);
    expect(find.byKey(const ValueKey('automation-retired-technical')), findsOneWidget);
    expect(
      tester.widget<GlassButton>(find.byKey(const ValueKey('automation-console-technical-tab'))).style,
      GlassButtonStyle.prominent,
    );
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox.shrink());
    await tester.pump();
  });
  testWidgets('future malformed foreign-job and foreign-digest metadata cannot add notice or advance cursor', (
    tester,
  ) async {
    for (final fault in ['future', 'shape', 'job', 'digest', 'cursor']) {
      final api = Api();
      await tester.pumpWidget(host(api));
      await settle(tester);
      api.reply = (id, after) {
        final result = page(id, after, retired: true);
        final marker = result['artifact_retirement'] as AutomationJson;
        switch (fault) {
          case 'future':
            marker['version'] = 2;
          case 'shape':
            marker['private'] = 'NEVER_DISPLAY';
          case 'job':
            result['job_id'] = b;
          case 'digest':
            result['manifest_digest'] = digest(b);
          case 'cursor':
            result['cursor'] = 9;
        }
        return result;
      };
      await tester.pump(const Duration(seconds: 2));
      await settle(tester);
      expect(find.byKey(const ValueKey('automation-retired-output')), findsNothing);
      expect(find.textContaining('NEVER_DISPLAY'), findsNothing);
      api.reply = (id, after) => page(id, after);
      await tester.pump(const Duration(seconds: 2));
      await settle(tester);
      expect(api.requests.last, (a, 1));
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox.shrink());
      await tester.pump();
    }
  });
  testWidgets('late original-job retirement cannot populate another job in the same console and scope', (tester) async {
    final api = Api();
    await tester.pumpWidget(host(api));
    await settle(tester);
    api.pending = Completer<AutomationJson>();
    await tester.pump(const Duration(seconds: 2));
    await settle(tester);
    expect(api.requests.last, (a, 1));
    await tester.pumpWidget(host(api, id: b));
    await settle(tester);
    expect(api.requests.last, (b, 0));
    api.pending!.complete(page(a, 1, retired: true));
    await settle(tester);
    expect(find.byKey(const ValueKey('automation-retired-output')), findsNothing);
    expect(find.textContaining('Saved details A'), findsNothing);
    expect(find.textContaining('B.yml'), findsOneWidget);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox.shrink());
    await tester.pump();
  });
}
