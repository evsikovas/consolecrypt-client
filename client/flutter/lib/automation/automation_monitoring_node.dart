import 'package:consolecrypt/automation/automation_service.dart';

/// Creates reviewed source only. Installation uses the ordinary Signed Run
/// workflow and independently checks the actual target's resources and storage.
abstract interface class AutomationMonitoringNodeService {
  Future<AutomationJson> createMonitoringNodeProject(AutomationJson request);
}
