import 'dart:async';

import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/automation/automation_portable_controller.dart';
import 'package:consolecrypt/automation/automation_portable_dialog.dart';
import 'package:consolecrypt/automation/automation_portable_package.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../unit/automation_portable_controller_test.dart' as fixture;

final labels = PortableDialogLabels(
  exportTitle: 'Export encrypted source package',
  importTitle: 'Inspect encrypted source package',
  separatePassword: 'Separate package password',
  confirmPassword: 'Confirm separate password',
  warning: 'Explicit source selection can contain hardcoded secrets. No credential/Vault lookup.',
  knownExcluded: 'Excluded by Native policy',
  wholeProject: 'Whole project',
  chooseFiles: 'Choose files',
  sizeLimit: 'Archive size limit exceeded',
  contents: (files, bytes) => '$files files, $bytes bytes selected',
  export: 'Export new file',
  inspect: 'Inspect package',
  cancel: 'Cancel',
  success: 'Encrypted file saved',
  failed: 'Package operation failed',
  publicationBlocked: 'Managed publication is not connected',
  summary: (files, bytes, excluded) => '$files files, $bytes bytes, $excluded excluded',
);
void main() {
  Future<void> open(
    WidgetTester tester,
    AutomationPortableController c,
    AutomationScope scope, {
    bool importMode = false,
    Size size = const Size(1000, 850),
    double scale = 1,
    double keyboard = 0,
  }) async {
    await tester.binding.setSurfaceSize(size);
    addTearDown(() => tester.binding.setSurfaceSize(null));
    await tester.pumpWidget(
      ProviderScope(
        overrides: [automationScopeProvider.overrideWithValue(scope)],
        child: MaterialApp(
          theme: AppTheme.dark(platform: TargetPlatform.windows),
          builder: (context, child) => MediaQuery(
            data: MediaQuery.of(context).copyWith(
              textScaler: TextScaler.linear(scale),
              viewInsets: EdgeInsets.only(bottom: keyboard),
            ),
            child: child!,
          ),
          locale: const Locale('en'),
          supportedLocales: const [Locale('en')],
          localizationsDelegates: [...GlobalMaterialLocalizations.delegates, AppLocalizations.delegate],
          home: Scaffold(
            body: Builder(
              builder: (context) => TextButton(
                onPressed: () => showDialog<void>(
                  context: context,
                  builder: (_) => AutomationPortableDialog(
                    controller: c,
                    originalScope: scope,
                    labels: labels,
                    importMode: importMode,
                  ),
                ),
                child: const Text('open'),
              ),
            ),
          ),
        ),
      ),
    );
    await tester.tap(find.text('open'));
    await tester.pumpAndSettle();
  }

  testWidgets('whole project exports all eligible files without opening the selection', (tester) async {
    final service = fixture.Service(), files = fixture.Files(), c = fixture.controller(service, files);
    const scope = AutomationScope(fixture.profile, true);
    await open(tester, c, scope);
    expect(service.exports, 0);
    expect(c.selected, {'play.yml', 'roles/demo/tasks/main.yml'});
    expect(find.byType(CheckboxListTile), findsNothing);
    expect(find.text('2 files, 60 bytes selected'), findsOneWidget);
    for (final key in ['portable-password', 'portable-confirm-password']) {
      await tester.enterText(
        find.descendant(of: find.byKey(ValueKey(key)), matching: find.byType(EditableText)),
        'synthetic package password',
      );
    }
    await tester.tap(find.byKey(const ValueKey('portable-submit')));
    await tester.pumpAndSettle();
    expect(service.exports, 1);
    expect(service.request?['selectedPaths'], ['play.yml', 'roles/demo/tasks/main.yml']);
    expect(find.byKey(const ValueKey('portable-exported')), findsOneWidget);
    expect(files.finishes, 1);
    expect(tester.widgetList<EditableText>(find.byType(EditableText)).every((e) => e.controller.text.isEmpty), isTrue);
    await tester.tap(find.text('Cancel'));
    await tester.pumpAndSettle();
    expect(find.byType(AutomationPortableDialog), findsNothing);
  });
  testWidgets('lock removesonlyownroute, cancelsnativepending andwipes displayedpassword', (tester) async {
    var current = true;
    final service = fixture.Service()..exportWait = Completer<PortableExportOutcome>(),
        files = fixture.Files(),
        c = fixture.controller(service, files, current: () => current);
    const scope = AutomationScope(fixture.profile, true);
    await open(tester, c, scope);
    await tester.tap(find.byKey(const ValueKey('portable-choose-files')));
    await tester.pump();
    expect(tester.widget<CheckboxListTile>(find.byKey(const ValueKey('portable-path-.env'))).onChanged, isNull);
    await tester.tap(find.byKey(const ValueKey('portable-path-play.yml')));
    await tester.pump();
    expect(c.selected, {'roles/demo/tasks/main.yml'});
    await tester.ensureVisible(find.byKey(const ValueKey('portable-whole-project')));
    await tester.tap(find.byKey(const ValueKey('portable-whole-project')));
    await tester.pumpAndSettle();
    expect(c.selected, {'play.yml', 'roles/demo/tasks/main.yml'});
    for (final key in ['portable-password', 'portable-confirm-password']) {
      await tester.enterText(
        find.descendant(of: find.byKey(ValueKey(key)), matching: find.byType(EditableText)),
        'synthetic package password',
      );
    }
    await tester.tap(find.byKey(const ValueKey('portable-submit')));
    await tester.pump();
    expect(service.exports, 1);
    final container = ProviderScope.containerOf(tester.element(find.byType(AutomationPortableDialog)));
    current = false;
    container.updateOverrides([
      automationScopeProvider.overrideWithValue(const AutomationScope(fixture.profile, false)),
    ]);
    await tester.pump();
    await tester.pump();
    expect(service.nativeCancel, isTrue);
    expect(find.byType(AutomationPortableDialog), findsNothing);
    expect(find.text('open'), findsOneWidget);
    service.exportWait!.complete(fixture.outcome());
    await tester.pump();
    expect(files.finishes, 0);
    expect(tester.takeException(), isNull);
  });
  testWidgets('oversized whole selection prompts a narrower choice before the save picker', (tester) async {
    final project = fixture.contextJson();
    (project['choices'] as List<Map<String, dynamic>>).first['bytes'] = 1024 * 1024 + 1;
    final service = fixture.Service()
      ..contextWait = (Completer<PortableContext>()..complete(PortableContext.fromJson(project)));
    final files = fixture.Files(), c = fixture.controller(service, files);
    await open(tester, c, const AutomationScope(fixture.profile, true));
    expect(find.byKey(const ValueKey('portable-size-limit')), findsOneWidget);
    expect(files.saves, 0);
    await tester.tap(find.byKey(const ValueKey('portable-choose-files')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('portable-path-play.yml')));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('portable-size-limit')), findsNothing);
    expect(c.selected, {'roles/demo/tasks/main.yml'});
  });
  testWidgets('rejected second inspection removes the previous summary and shows a boxed failure', (tester) async {
    final service = fixture.Service(), files = fixture.Files(), c = fixture.controller(service, files);
    const scope = AutomationScope(fixture.profile, true);
    await open(tester, c, scope, importMode: true);
    Future<void> password() async => tester.enterText(
      find.descendant(of: find.byKey(const ValueKey('portable-password')), matching: find.byType(EditableText)),
      'synthetic package password',
    );
    await password();
    await tester.tap(find.byKey(const ValueKey('portable-submit')));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('portable-import-blocked')), findsOneWidget);
    service.inspectWait = Completer<PortableImportPreview>();
    service.inspectEntered = Completer<void>();
    await password();
    await tester.tap(find.byKey(const ValueKey('portable-submit')));
    await tester.pump();
    expect(service.inspects, 2);
    expect(find.byKey(const ValueKey('portable-import-blocked')), findsNothing);
    service.inspectWait!.completeError(const PortablePackageFailure('portable_context_or_package_unavailable'));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('portable-import-blocked')), findsNothing);
    expect(tester.widget<InfoBanner>(find.byKey(const ValueKey('portable-failure'))).tone, BannerTone.danger);
    expect(tester.takeException(), isNull);
    await tester.tap(find.text('Cancel'));
    await tester.pumpAndSettle();
  });

  testWidgets('foreign initial Native profile scope is rejected before context or picker calls', (tester) async {
    final foreign = PortableScope.fromJson({
      'version': 1,
      'profileId': '44444444-4444-4444-8444-444444444444',
      'notificationScopeId': fixture.nonce,
    });
    final service = fixture.Service(),
        files = fixture.Files(),
        c = AutomationPortableController(
          scope: foreign,
          service: service,
          files: files,
          isOriginalCurrent: () => true,
          projectId: fixture.project,
          projectRevision: 1,
        );
    const original = AutomationScope(fixture.profile, true);
    await open(tester, c, original);
    expect(find.byType(AutomationPortableDialog), findsNothing);
    expect(service.contexts, 0);
    expect(service.exports, 0);
    expect(files.saves, 0);
    expect(files.opens, 0);
    expect(find.text('open'), findsOneWidget);
  });

  testWidgets('narrow large-text dialog remains bounded with keyboard and keeps cancel available', (tester) async {
    final service = fixture.Service(), files = fixture.Files(), c = fixture.controller(service, files);
    const scope = AutomationScope(fixture.profile, true);
    await open(tester, c, scope, size: const Size(420, 760), scale: 1.5, keyboard: 220);
    expect(tester.takeException(), isNull);
    expect(service.exports, 0);
    expect(find.byKey(const ValueKey('portable-password')), findsOneWidget);
    await tester.ensureVisible(find.text('Cancel'));
    await tester.tap(find.text('Cancel'));
    await tester.pumpAndSettle();
    expect(find.byType(AutomationPortableDialog), findsNothing);
    expect(service.exports, 0);
  });

  testWidgets('already locked initial UI scope cannot start context loading even with a stale true predicate', (
    tester,
  ) async {
    final service = fixture.Service(), files = fixture.Files(), c = fixture.controller(service, files);
    const scope = AutomationScope(fixture.profile, false);
    await open(tester, c, scope);
    expect(find.byType(AutomationPortableDialog), findsNothing);
    expect(service.contexts, 0);
    expect(files.saves, 0);
    expect(files.opens, 0);
    expect(find.text('open'), findsOneWidget);
  });
}
