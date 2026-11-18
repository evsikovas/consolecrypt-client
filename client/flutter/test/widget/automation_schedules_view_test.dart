import 'dart:async';

import 'package:consolecrypt/app/critical_notice_providers.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_schedules_view.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/services/runtime_notice_adapter.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_projects_fixture.dart';

const _profile = '2df01102-cf11-419a-91a2-31bb1f809897';
const _nonce = 'fixture-schedule-scope';

class _ScheduleFixture extends Projects implements AutomationScheduleCatalogService {
  final requests = <AutomationJson>[];
  @override
  Future<AutomationJson> scheduleCatalog(AutomationJson request) async {
    requests.add(request);
    return {
      ...request,
      'metadataOnly': true,
      'records': <AutomationJson>[
        {
          'projectId': 'project',
          'runnerId': 'server',
          'playbook': 'updates.yml',
          'expiresUtc': '2099-02-10T16:00:00Z',
          'delivery': 'acknowledged',
          'remoteState': 'notObserved',
          'schedule': {
            'hour': 8,
            'minute': 35,
            'zone': 'Europe/Moscow',
            'weekdays': [0, 4],
          },
        },
      ],
    };
  }
}

class _Events implements RuntimeNoticeEvents {
  @override
  RuntimeNoticeFrame currentScope = RuntimeNoticeFrame(
    epoch: 1,
    scope: RuntimeNoticeScope.fromProfile({
      'active': true,
      'vault_state': 'unlocked',
      'id': _profile,
      'notification_scope_id': _nonce,
    }),
  );
  final changes = StreamController<RuntimeNoticeFrame>.broadcast();
  @override
  Stream<RuntimeNoticeFrame> watchRuntimeNotices() => changes.stream;
}

void main() {
  testWidgets('lists acknowledged schedules and only opens the selected project on request', (tester) async {
    final service = _ScheduleFixture();
    final events = _Events();
    addTearDown(events.changes.close);
    const scope = AutomationScope(_profile, true);
    const project = AutomationProject(
      id: 'project',
      name: 'Package updates',
      sourceKind: 'managed',
      readOnly: false,
      revision: 1,
    );
    final opened = <AutomationProject>[];
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          automationScopeProvider.overrideWithValue(scope),
          criticalNoticeScopeProvider.overrideWithValue(const CriticalNoticeScope(_profile, true)),
          criticalRuntimeEventsProvider.overrideWithValue(events),
          automationProjectsProvider.overrideWith(
            (ref) async =>
                const AutomationProjectCatalog(version: 1, deviceLocal: true, readOnly: false, projects: [project]),
          ),
          automationRunnersProvider.overrideWith(
            (ref) async => const AutomationRunnerCatalog(
              readOnly: false,
              runners: [
                AutomationRunnerProfile(
                  id: 'server',
                  name: 'Linux server',
                  readOnly: false,
                  configuration: {'kind': 'service_remote'},
                ),
              ],
            ),
          ),
        ],
        child: MaterialApp(
          localizationsDelegates: AppLocalizations.localizationsDelegates,
          supportedLocales: AppLocalizations.supportedLocales,
          home: Scaffold(
            body: AutomationSchedulesView(
              active: true,
              scope: scope,
              service: service,
              onCreate: (project) async => opened.add(project),
            ),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(service.requests.single, {'version': 1, 'profileId': _profile, 'notificationScopeId': _nonce});
    expect(find.text('updates.yml'), findsOneWidget);
    expect(find.text('Accepted by server'), findsOneWidget);
    expect(find.textContaining('Europe/Moscow'), findsOneWidget);
    expect(opened, isEmpty);
    await tester.tap(find.text('New schedule'));
    await tester.pumpAndSettle();
    expect(opened, [project]);
    events.currentScope = const RuntimeNoticeFrame(epoch: 2, reset: true);
    events.changes.add(events.currentScope);
    await tester.pumpAndSettle();
    expect(find.text('updates.yml'), findsNothing);
    expect(service.requests, hasLength(1));
  });
}
