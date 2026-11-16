import 'dart:async';

import 'package:consolecrypt/app/critical_notice_providers.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/automation/automation_wsl_preparation.dart';
import 'package:consolecrypt/automation/automation_wsl_wizard_entry.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart' show testBackend;

const scope = AutomationScope('11111111-1111-4111-8111-111111111111', true);

class Runner extends Fake implements AutomationRunnerService {}

AppServices replacement(AppServices old) => AppServices(
  profiles: old.profiles,
  auth: old.auth,
  vault: old.vault,
  inventory: old.inventory,
  terminal: old.terminal,
  sftp: old.sftp,
  tunnels: old.tunnels,
  snippets: old.snippets,
  ai: old.ai,
  devices: old.devices,
  sync: old.sync,
  settings: old.settings,
  backups: old.backups,
  files: old.files,
);

class Entry extends ConsumerWidget {
  const Entry(this.service, {super.key});
  final Runner service;
  @override
  Widget build(BuildContext context, WidgetRef ref) => Scaffold(
    body: TextButton(
      onPressed: () => unawaited(
        showAutomationWslWizard(
          context: context,
          ref: ref,
          scope: scope,
          service: service,
          originalCurrent: () => true,
        ),
      ),
      child: const Text('OPEN'),
    ),
  );
}

void main() {
  testWidgets(
    'same-service and same-terminal AppServices replacement retires only the owned wizard under an unrelated modal',
    (tester) async {
      final backend = testBackend(), service = Runner();
      final app = backend.services;
      final router = GoRouter(
        initialLocation: '/automation',
        routes: [
          GoRoute(path: '/automation', builder: (_, _) => Entry(service)),
        ],
      );
      final container = ProviderContainer(
        overrides: [
          appServicesProvider.overrideWithValue(app),
          automationScopeProvider.overrideWithValue(scope),
          automationRunnerServiceProvider.overrideWithValue(service),
          criticalRuntimeEventsProvider.overrideWithValue(null),
          routerProvider.overrideWithValue(router),
        ],
      );
      addTearDown(container.dispose);
      addTearDown(router.dispose);
      await tester.pumpWidget(
        UncontrolledProviderScope(
          container: container,
          child: MaterialApp.router(
            routerConfig: router,
            theme: AppTheme.dark(platform: TargetPlatform.windows),
            supportedLocales: AppLocalizations.supportedLocales,
            localizationsDelegates: const [
              AppLocalizations.delegate,
              ...GlobalMaterialLocalizations.delegates,
            ],
          ),
        ),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.text('OPEN'));
      await tester.pumpAndSettle();
      expect(find.byType(AutomationWslPreparation), findsOneWidget);
      final wizardContext = tester.element(
        find.byType(AutomationWslPreparation),
      );
      unawaited(
        showDialog<void>(
          context: wizardContext,
          builder: (_) => const AlertDialog(content: Text('UNRELATED')),
        ),
      );
      await tester.pumpAndSettle();
      container.updateOverrides([
        appServicesProvider.overrideWithValue(replacement(app)),
        automationScopeProvider.overrideWithValue(scope),
        automationRunnerServiceProvider.overrideWithValue(service),
        criticalRuntimeEventsProvider.overrideWithValue(null),
        routerProvider.overrideWithValue(router),
      ]);
      await tester.pumpAndSettle();
      expect(find.byType(AutomationWslPreparation), findsNothing);
      expect(find.text('UNRELATED'), findsOneWidget);
      expect(router.routeInformationProvider.value.uri.path, '/automation');
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox.shrink());
    },
  );
}
