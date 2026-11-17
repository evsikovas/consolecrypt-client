import 'package:consolecrypt/app/critical_notice_providers.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/automation/automation_agent_installer.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/runtime_notice_adapter.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';
import '../support/agent_installer_fixture.dart' as f;
import 'automation_exporter_template_test.dart' show Projects;

class _Source extends Projects implements AutomationAgentInstallerService {
  int reads = 0, creates = 0;
  @override
  Future<AutomationJson> agentInstallerContext(AutomationJson frame) async {
    reads++;
    return {
      ...f.context(),
      'profileId': frame['profileId'],
      'notificationScopeId': frame['notificationScopeId'],
    };
  }

  @override
  Future<AutomationJson> createAgentInstallerProject(
    AutomationJson request,
  ) async {
    creates++;
    throw StateError('cancel must never create');
  }
}

void main() {
  testWidgets(
    'Automation menu excludes Agent deployment even when the service supports it',
    (tester) async {
      final service = _Source(),
          events = RuntimeNoticeEventAdapter(),
          backend = testBackend();
      addTearDown(backend.dispose);
      addTearDown(events.dispose);
      await backend.debugCreateUnlockedLocalProfile(
        name: 'Owned source workflow',
      );
      await pumpApp(
        tester,
        backend,
        overrides: [
          automationServiceProvider.overrideWithValue(service),
          criticalRuntimeEventsProvider.overrideWithValue(events),
        ],
      );
      final container = ProviderScope.containerOf(
        tester.element(find.byType(Navigator).first),
      );
      final profile = container.read(activeProfileProvider)!;
      events.acceptScope(
        originalEpoch: events.epoch,
        nativeProfile: {
          'id': profile.id.value,
          'active': true,
          'vault_state': 'unlocked',
          'notification_scope_id': 'opaque original source parent',
        },
      );
      container.read(routerProvider).go(AppRoutes.automation);
      await settle(tester);
      expect(service.reads, 0);
      expect(service.creates, 0);
      await tapKey(tester, 'automation-project-actions');
      expect(find.byKey(const ValueKey('automation-agent-install-template')), findsNothing);
      expect(find.byKey(const ValueKey('automation-new-project')), findsOneWidget);
      expect(service.reads, 0);
      expect(
        find.byKey(const ValueKey('agent-installer-dialog')),
        findsNothing,
      );
      expect(service.creates, 0);
      expect(service.projectsCreated, isEmpty);
      expect(service.expected, isEmpty);
      expect(tester.takeException(), null);
    },
    variant: const TargetPlatformVariant({TargetPlatform.windows}),
  );
}
