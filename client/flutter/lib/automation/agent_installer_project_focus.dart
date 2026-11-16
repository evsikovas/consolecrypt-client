import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/automation/automation_agent_installer.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_service.dart';

/// One in-memory source focus from an actual parsed creation receipt.
/// No serialized route payload, execution request, grant, or persistent registry.
final class AgentInstallerProjectFocus {
  AgentInstallerProjectFocus.fromCreated({
    required AgentInstallerCreated receipt,
    required this.hostId,
    required this.hostName,
    required this.returnUri,
    required this.scope,
    required this.service,
    required this.original,
  }) : projectId = receipt.project.id {
    if (returnUri.path != AppRoutes.hosts || returnUri.hasScheme || returnUri.hasAuthority) {
      throw const FormatException('Host return route invalid');
    }
  }
  final String projectId, hostId, hostName;
  final Uri returnUri;
  final AutomationScope scope;
  final AutomationService service;
  final bool Function() original;
  bool _taken = false, _retired = false;
  bool current(AutomationScope currentScope, AutomationService currentService) {
    if (_retired) return false;
    if (!identical(scope, currentScope) || !identical(service, currentService) || !original()) {
      _retired = true;
      return false;
    }
    return true;
  }

  bool take(AutomationScope currentScope, AutomationService currentService) {
    if (_taken || !current(currentScope, currentService)) return false;
    _taken = true;
    return true;
  }
}

/// One explicit Host-to-cAdvisor composer request. No project ID, persisted
/// payload, execution flag or Agent capability is carried or fabricated.
final class AgentCadvisorComposerFocus {
  AgentCadvisorComposerFocus({
    required this.host,
    required this.frame,
    required this.returnUri,
    required this.scope,
    required this.service,
    required this.original,
  }) {
    if (returnUri.path != AppRoutes.hosts ||
        returnUri.hasScheme ||
        returnUri.hasAuthority ||
        !scope.unlocked ||
        frame.profileId != scope.profileId?.toString()) {
      throw const FormatException('Original cAdvisor Host context unavailable');
    }
  }
  final AgentInstallerHost host;
  final AgentInstallerFrame frame;
  final Uri returnUri;
  final AutomationScope scope;
  final AutomationService service;
  final bool Function() original;
  String get hostId => host.id;
  String get hostRevision => host.revision;
  String get hostName => host.name;
  bool _taken = false, _retired = false;
  bool current(AutomationScope currentScope, AutomationService currentService) {
    if (_retired) return false;
    if (!identical(scope, currentScope) || !identical(service, currentService) || !original()) {
      _retired = true;
      return false;
    }
    return true;
  }

  bool take(AutomationScope currentScope, AutomationService currentService) {
    if (_taken || !current(currentScope, currentService)) return false;
    _taken = true;
    return true;
  }
}
