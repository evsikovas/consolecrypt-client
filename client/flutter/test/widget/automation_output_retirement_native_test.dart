import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/automation/automation_execution_console.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:crypto/crypto.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

const scope = AutomationScope('owned-captured-page-profile', true);

class CapturedApi extends Fake implements AutomationJobService, AutomationEventService {
  CapturedApi(this.captured);
  final AutomationJson captured;
  final requests = <int>[];
  bool foreign = false;
  String get id => captured['job_id'] as String;
  @override
  Future<AutomationJson> jobs() async => {
    'jobs': [
      {
        'job_id': id,
        'manifest_digest': captured['manifest_digest'],
        'state': 'unknown',
        'manifest': {'playbook': 'owned-native-wire.yml', 'targets': <AutomationJson>[]},
      },
    ],
  };
  @override
  Future<AutomationJson> jobEvents(String id, {required int after, int limit = 50}) async {
    requests.add(after);
    expect(id, this.id);
    // Synthetic prior cursor setup only; the retirement producer/JSON below is untouched Native output.
    if (after == 0) {
      return {
        'job_id': id,
        'manifest_digest': captured['manifest_digest'],
        'state': 'unknown',
        'cursor': 4,
        'output_truncated': false,
        'events': [
          for (var seq = 1; seq <= 4; seq++)
            {
              'version': 1,
              'job_id': id,
              'sequence': seq,
              'timestamp': '2026-11-13T12:00:00Z',
              'type': 'runner_on_ok',
              'payload': {
                'task': 'Prior cursor setup',
                'res': {'changed': false},
              },
            },
        ],
      };
    }
    final result = Map<String, dynamic>.from(captured);
    if (foreign) result['manifest_digest'] = 'b' * 64;
    return result;
  }
}

Widget host(CapturedApi api) => ProviderScope(
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
          scope: scope,
          service: api,
          jobId: api.id,
          collapsed: false,
          onToggle: () {},
          onHistory: () {},
        ),
      ),
    ),
  ),
);
Future<void> settle(WidgetTester tester) async {
  for (var i = 0; i < 5; i++) {
    await tester.pump(const Duration(milliseconds: 100));
  }
}

void main() {
  testWidgets(
    'actual Native serialized retired page passes strict console adapter without a synthetic marker or cursor movement',
    (tester) async {
      final bytes = File('test/fixtures/retention_native_page.json').readAsBytesSync();
      expect(sha256.convert(bytes).toString(), 'b661d373e908a3f00eac9d9a64769431b007b31802d6edc1bc0746d9c1a05f2f');
      final actual = Map<String, dynamic>.from(jsonDecode(utf8.decode(bytes)) as Map);
      expect(actual['cursor'], 4);
      expect(actual['events'], isEmpty);
      for (final foreign in [false, true]) {
        final api = CapturedApi(actual)..foreign = foreign;
        await tester.pumpWidget(host(api));
        await settle(tester);
        expect(api.requests, [0]);
        await tester.pump(const Duration(seconds: 2));
        await settle(tester);
        expect(find.byKey(const ValueKey('automation-retired-output')), foreign ? findsNothing : findsOneWidget);
        expect(find.textContaining('Prior cursor setup'), findsWidgets);
        await tester.pump(const Duration(seconds: 2));
        await settle(tester);
        expect(api.requests, [0, 4, 4]);
        expect(tester.takeException(), isNull);
        await tester.pumpWidget(const SizedBox.shrink());
        await tester.pump();
      }
    },
  );
}
