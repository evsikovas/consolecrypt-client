import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/automation/automation_dependency_dialog.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_dependency_fixture.dart' as fixture;
const scope = AutomationScope('owned-installer-ui-profile', true);
final labels = DependencyInstallerLabels(
  title: 'Dependencies',
  kind: 'Type',
  role: 'Role',
  collection: 'Collection',
  name: 'Name',
  version: 'Version',
  url: 'Archive HTTPS URL',
  sha256: 'Archive SHA-256',
  runner: 'Runner',
  review: 'Review source',
  download: 'Download',
  diff: 'Review changes',
  install: 'Install',
  cancel: 'Cancel',
  close: 'Close',
  firstInstall:
      'First dependency only; updates and receipt merges are unavailable.',
  managedOnly: 'Use a supported managed project.',
  profileUnsupported: 'This project location does not support installation.',
  excludedEntries: 'Git control entries are not supported.',
  platformUnsupported: 'This platform is not supported.',
  networkNotice: 'This action contacts the reviewed source.',
  preparing: 'Preparing',
  published: 'Installed in a new project revision; no playbook ran.',
  uncertain: 'Installation outcome needs review. Do not retry.',
  cancelRequested: 'Cancellation requested',
  failureText: (code) => 'Fixed failure: ${code.name}',
);
Future<ProviderContainer> open(
  WidgetTester tester,
  fixture.Service service, {
  String sourceKind = 'managed',
}) async {
  await tester.binding.setSurfaceSize(const Size(1100, 1200));
  addTearDown(() => tester.binding.setSurfaceSize(null));
  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        automationScopeProvider.overrideWithValue(scope),
        automationRunnersProvider.overrideWith(
          (_) async => const AutomationRunnerCatalog(
            readOnly: false,
            runners: [
              AutomationRunnerProfile(
                id: fixture.runner,
                name: 'Fixture runner',
                readOnly: false,
                revision: 1,
              ),
            ],
          ),
        ),
      ],
      child: MaterialApp(
        theme: AppTheme.dark(platform: TargetPlatform.windows),
        locale: const Locale('en'),
        supportedLocales: AppLocalizations.supportedLocales,
        localizationsDelegates: const [
          AppLocalizations.delegate,
          ...GlobalMaterialLocalizations.delegates,
        ],
        home: Scaffold(
          body: Builder(
            builder: (context) => TextButton(
              key: const ValueKey('open-installer'),
              onPressed: () => showDialog<void>(
                context: context,
                barrierDismissible: false,
                builder: (_) => AutomationDependencyDialog(
                  scope: scope,
                  service: service,
                  project: AutomationProject(
                    id: fixture.project,
                    name: 'Owned project',
                    readOnly: false,
                    sourceKind: sourceKind,
                    revision: 1,
                  ),
                  labels: labels,
                  sourceCurrent: () => true,
                ),
              ),
              child: const Text('Open'),
            ),
          ),
        ),
      ),
    ),
  );
  await tester.tap(find.byKey(const ValueKey('open-installer')));
  await tester.pumpAndSettle();
  return ProviderScope.containerOf(
    tester.element(find.byType(AutomationDependencyDialog)),
    listen: false,
  );
}

Future<void> tap(WidgetTester tester, String key) async {
  final finder = find.byKey(ValueKey(key));
  await tester.ensureVisible(finder);
  await tester.tap(finder);
  await tester.pumpAndSettle();
}

Future<void> fill(WidgetTester tester) async {
  await tap(tester, 'dependency-runner');
  await tester.tap(find.textContaining('Fixture runner').last);
  await tester.pumpAndSettle();
  for (final field in [
    ('dependency-name', 'demo'),
    ('dependency-version', '1.2.3'),
    ('dependency-url', fixture.proposal.url),
    ('dependency-sha', fixture.archive),
  ]) {
    await tester.ensureVisible(find.byKey(ValueKey(field.$1)));
    await tester.enterText(find.byKey(ValueKey(field.$1)), field.$2);
  }
  await tester.pump();
}

Future<void> finish(WidgetTester tester) async {
  await tester.pumpWidget(const SizedBox.shrink());
  await tester.pump();
}

void main() {
  testWidgets(
    'manual Runner/source review does not start network until explicit Download',
    (tester) async {
      final service = fixture.Service();
      await open(tester, service);
      expect(service.requests, isEmpty);
      expect(
        tester
            .widget<TextButton>(find.byKey(const ValueKey('dependency-review')))
            .onPressed,
        isNull,
      );
      await fill(tester);
      await tap(tester, 'dependency-review');
      expect(service.requests.map((x) => x['type']), ['review']);
      expect(find.text('https://fixtures.invalid'), findsOneWidget);
      expect(find.byKey(const ValueKey('dependency-download')), findsOneWidget);
      await tap(tester, 'dependency-download');
      expect(service.requests[1], {
        'type': 'start',
        'reviewId': fixture.reviewId,
      });
      expect(service.requests.any((x) => x['type'] == 'publish'), false);
      await finish(tester);
    },
  );
  testWidgets(
    'existing diff review then explicit Install sends accept and accepted-only publish',
    (tester) async {
      final service = fixture.Service();
      await open(tester, service);
      await fill(tester);
      await tap(tester, 'dependency-review');
      await tap(tester, 'dependency-download');
      expect(
        tester
            .widget<FilledButton>(
              find.byKey(const ValueKey('dependency-install')),
            )
            .onPressed,
        isNull,
      );
      await tap(tester, 'dependency-diff');
      await tap(tester, 'automation-diff-reviewed');
      await tap(tester, 'dependency-install');
      expect(
        service.requests
            .where((x) => x['type'] == 'accept' || x['type'] == 'publish')
            .map((x) => x['type']),
        ['accept', 'publish'],
      );
      expect(find.text(labels.published), findsOneWidget);
      await finish(tester);
    },
  );
  testWidgets(
    'unsupported profile and Directory project never enable Download',
    (tester) async {
      final service = fixture.Service()..supported = false;
      await open(tester, service);
      await fill(tester);
      await tap(tester, 'dependency-review');
      expect(find.text(labels.profileUnsupported), findsOneWidget);
      expect(
        tester
            .widget<FilledButton>(
              find.byKey(const ValueKey('dependency-download')),
            )
            .onPressed,
        isNull,
      );
      expect(service.requests.map((x) => x['type']), ['review']);
      await finish(tester);
      final other = fixture.Service();
      await open(tester, other, sourceKind: 'directory');
      expect(find.text(labels.managedOnly), findsOneWidget);
      expect(other.requests, isEmpty);
      await finish(tester);
    },
  );
  testWidgets(
    'scope replacement clears old dialog and cannot send later approval',
    (tester) async {
      final service = fixture.Service();
      final container = await open(tester, service);
      await fill(tester);
      await tap(tester, 'dependency-review');
      await tap(tester, 'dependency-download');
      container.updateOverrides([
        automationScopeProvider.overrideWithValue(
          const AutomationScope('owned-installer-ui-profile', false),
        ),
        automationRunnersProvider.overrideWith(
          (_) async =>
              const AutomationRunnerCatalog(readOnly: false, runners: []),
        ),
      ]);
      await tester.pump();
      expect(find.byKey(const ValueKey('dependency-install')), findsNothing);
      expect(service.requests.any((x) => x['type'] == 'publish'), false);
      await finish(tester);
    },
  );
  testWidgets(
    'uncertain publication uses common framed danger and no retry or backend text',
    (tester) async {
      final service = fixture.Service();
      await open(tester, service);
      await fill(tester);
      await tap(tester, 'dependency-review');
      await tap(tester, 'dependency-download');
      await tap(tester, 'dependency-diff');
      await tap(tester, 'automation-diff-reviewed');
      service.handler = (action) {
        if (action['type'] == 'publish') {
          throw StateError('SYNTHETIC_BACKEND_MARKER');
        }
        return Future.value(service.defaultReply(action));
      };
      await tap(tester, 'dependency-install');
      expect(
        tester
            .widget<InfoBanner>(find.byKey(const ValueKey('dependency-error')))
            .tone,
        BannerTone.danger,
      );
      expect(find.text(labels.uncertain), findsOneWidget);
      expect(find.textContaining('SYNTHETIC_BACKEND_MARKER'), findsNothing);
      expect(find.byKey(const ValueKey('dependency-install')), findsNothing);
      expect(find.byKey(const ValueKey('dependency-review')), findsNothing);
      expect(service.requests.where((x) => x['type'] == 'publish').length, 1);
      await finish(tester);
    },
  );
}
