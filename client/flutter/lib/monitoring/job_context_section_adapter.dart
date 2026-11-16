import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/monitoring/job_context_navigation.dart';

/// Only existing section selection. The caller consumes its original one-shot
/// intent after fresh context checks. Receiving a host/project proposal must
/// never invoke _newTab/openTarget, Docker._connect or Automation._select.
/// Future UI can show the exact proposal beside an explicit separate action.
String jobContextSectionRoute(JobContextNavigationSelection selection) => switch (selection.destination) {
  JobContextDestination.sshHost => AppRoutes.terminal,
  JobContextDestination.dockerHost => AppRoutes.docker,
  JobContextDestination.ansibleProject => AppRoutes.automation,
  JobContextDestination.monitoringBinding => AppRoutes.monitoring,
};
