import 'package:consolecrypt/automation/agent_installer_project_focus.dart';
import 'package:consolecrypt/automation/automation_agent_installer.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/agent_installer_fixture.dart' as f;
import '../widget/automation_exporter_template_test.dart' show Projects;

AgentInstallerCreated receipt() {
  final c = AgentInstallerContext.parse(
    f.context(),
    AgentInstallerFrame(profileId: f.profileId, notificationScopeId: f.nonce),
  );
  final request = c.request(
    name: 'Owned source',
    host: c.hosts.single,
    operation: AgentInstallerOperation.remove,
    start: false,
  );
  return AgentInstallerCreated.parse(f.created(request), c, request);
}

void main() {
  test(
    'cAdvisor focus carries actual selected Native Host/frame once with no project receipt or stale-scope adoption',
    () {
      final scope = const AutomationScope(f.profileId, true), service = Projects();
      final context = AgentInstallerContext.parse(
        f.context(),
        AgentInstallerFrame(profileId: f.profileId, notificationScopeId: f.nonce),
      );
      var live = true;
      AgentCadvisorComposerFocus focus() => AgentCadvisorComposerFocus(
        host: context.hosts.single,
        frame: context.frame,
        returnUri: Uri.parse('/hosts'),
        scope: scope,
        service: service,
        original: () => live,
      );
      final selected = focus();
      expect(selected.hostId, f.hostId);
      expect(selected.hostRevision, context.hosts.single.revision);
      expect(selected.frame.notificationScopeId, f.nonce);
      expect(selected.take(scope, service), isTrue);
      expect(selected.take(scope, service), isFalse);
      expect(service.projectsCreated, isEmpty);
      expect(service.expected, isEmpty);
      live = false;
      expect(selected.current(scope, service), isFalse);
      live = true;
      expect(selected.current(scope, service), isFalse);
      final otherScope = focus();
      expect(otherScope.take(AutomationScope(scope.profileId, true), service), isFalse);
      expect(otherScope.take(scope, service), isFalse);
    },
  );

  test('receipt focus contains exact created project ID and consumes once in same original scope only', () {
    final scope = const AutomationScope(f.profileId, true), service = Projects();
    var original = true;
    final focus = AgentInstallerProjectFocus.fromCreated(
      receipt: receipt(),
      hostId: f.hostId,
      hostName: 'Saved source',
      returnUri: Uri.parse('/hosts'),
      scope: scope,
      service: service,
      original: () => original,
    );
    expect(focus.projectId, f.projectId);
    expect(focus.take(scope, service), true);
    expect(focus.take(scope, service), false);
    original = false;
    expect(focus.current(scope, service), false);
    original = true;
    expect(focus.current(scope, service), false);
    expect(service.projectsCreated, isEmpty);
    expect(service.expected, isEmpty);
  });
  test('another scope or non-Hosts destination cannot adopt a local receipt focus', () {
    final scope = const AutomationScope(f.profileId, true), service = Projects();
    final focus = AgentInstallerProjectFocus.fromCreated(
      receipt: receipt(),
      hostId: f.hostId,
      hostName: 'Saved source',
      returnUri: Uri.parse('/hosts'),
      scope: scope,
      service: service,
      original: () => true,
    );
    expect(focus.take(AutomationScope(scope.profileId, scope.unlocked), service), false);
    expect(focus.take(scope, service), false);
    expect(
      () => AgentInstallerProjectFocus.fromCreated(
        receipt: receipt(),
        hostId: f.hostId,
        hostName: 'Saved source',
        returnUri: Uri.parse('https://example.invalid/hosts'),
        scope: scope,
        service: service,
        original: () => true,
      ),
      throwsFormatException,
    );
  });
}
