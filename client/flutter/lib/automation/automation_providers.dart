import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

final automationServiceProvider = Provider<AutomationService?>((ref) => ref.watch(appServicesProvider).automation);

final automationRunnerServiceProvider = Provider<AutomationRunnerService?>((ref) {
  final service = ref.watch(automationServiceProvider);
  return service is AutomationRunnerService ? service as AutomationRunnerService : null;
});

final automationJobServiceProvider = Provider<AutomationJobService?>((ref) {
  final service = ref.watch(automationServiceProvider);
  return service is AutomationJobService ? service as AutomationJobService : null;
});

final automationRunnersProvider = FutureProvider.autoDispose<AutomationRunnerCatalog?>((ref) async {
  final scope = ref.watch(automationScopeProvider);
  final service = ref.watch(automationRunnerServiceProvider);
  if (!scope.unlocked || service == null) return null;
  final catalog = await service.runners();
  if (!AppPlatform.isMobile) return catalog;
  return AutomationRunnerCatalog(
    readOnly: catalog.readOnly,
    runners: catalog.runners.where((runner) => runner.configuration?['kind'] != 'local').toList(),
  );
});

final class AutomationScope {
  const AutomationScope(this.profileId, this.unlocked);
  final Object? profileId;
  final bool unlocked;
}

final automationScopeProvider = Provider<AutomationScope>(
  (ref) => AutomationScope(
    ref.watch(activeProfileProvider.select((profile) => profile?.id)),
    ref.watch(appStageProvider) == AppStage.unlocked,
  ),
);

final automationProjectsProvider = FutureProvider.autoDispose<AutomationProjectCatalog?>((ref) async {
  final scope = ref.watch(automationScopeProvider);
  final service = ref.watch(automationServiceProvider);
  if (!scope.unlocked || service == null) return null;
  return service.projects();
});
