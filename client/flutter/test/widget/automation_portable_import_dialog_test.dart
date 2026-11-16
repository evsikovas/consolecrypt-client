import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/automation/automation_portable_import_controller.dart';
import 'package:consolecrypt/automation/automation_portable_import_dialog.dart';
import 'package:consolecrypt/automation/automation_portable_import_session.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_ui_proof.dart';
import '../helpers/portable_import_fixture.dart' as f;
import '../helpers/test_app.dart' show testBackend;

PortableImportLabels labels(BuildContext context) {
  final l = context.l10n, language = Localizations.localeOf(context).languageCode;
  final all = jsonDecode(File('test/helpers/portable_import_copy.json').readAsStringSync()) as Map<String, dynamic>;
  final copy = Map<String, dynamic>.from(all[language == 'ru' ? 'ru' : 'en'] as Map);
  String text(String key) => copy['automationPortableImport$key'] as String;
  return PortableImportLabels(
    title: text('Title'),
    warning: text('Warning'),
    chooseFile: text('ChooseFile'),
    noFile: text('NoFile'),
    name: text('Name'),
    password: l.automationPortablePassword,
    prepare: text('Prepare'),
    accept: text('Accept'),
    cancel: l.commonCancel,
    close: l.commonClose,
    edit: l.commonEdit,
    preparing: text('Preparing'),
    accepting: text('Accepting'),
    previewOnly: text('PreviewOnly'),
    normalizedName: text('NormalizedName'),
    futureId: text('FutureId'),
    limitedReview: text('LimitedReview'),
    installed: text('Installed'),
    summary: l.automationPortableSummary,
    issue: (issue) => text(switch (issue) {
      PortableImportIssue.unavailable => 'Unavailable',
      PortableImportIssue.invalidInput => 'InvalidInput',
      PortableImportIssue.changed => 'Changed',
      PortableImportIssue.sourceChanged => 'SourceChanged',
      PortableImportIssue.unsupported => 'Unsupported',
      PortableImportIssue.busy => 'Busy',
      PortableImportIssue.expired => 'Expired',
      PortableImportIssue.cancelled => 'Changed',
      PortableImportIssue.response => 'Response',
      PortableImportIssue.failed => 'Failed',
      PortableImportIssue.recoveryRequired => 'RecoveryRequired',
    }),
    publicationReason: (reason) => text(switch (reason) {
      'publication_platform_unsupported' => 'PlatformUnsupported',
      'publication_location_unavailable' => 'LocationUnavailable',
      'package_tree_unsupported' => 'TreeUnsupported',
      'import_capacity_reached' => 'CapacityReached',
      _ => 'Unsupported',
    }),
  );
}

Future<void> open(
  WidgetTester tester,
  f.Fixture x, {
  double width = 1000,
  double height = 900,
  double scale = 1,
  double keyboard = 0,
  Locale locale = const Locale('en'),
  ValueNotifier<AutomationScope>? scope,
}) async {
  tester.view.physicalSize = Size(width, height);
  tester.view.devicePixelRatio = 1;
  tester.view.viewInsets = FakeViewPadding(bottom: keyboard);
  tester.platformDispatcher.textScaleFactorTestValue = scale;
  addTearDown(tester.platformDispatcher.clearTextScaleFactorTestValue);
  addTearDown(tester.view.reset);
  final backend = testBackend(), original = const AutomationScope(f.profile, true);
  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        appServicesProvider.overrideWithValue(backend.services),
        fileDialogServiceProvider.overrideWithValue(x.files),
        automationScopeProvider.overrideWith((ref) => scope?.value ?? original),
      ],
      child: MaterialApp(
        theme: AppTheme.dark(platform: TargetPlatform.windows),
        locale: locale,
        supportedLocales: AppLocalizations.supportedLocales,
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        home: Builder(
          builder: (context) => Scaffold(
            body: FilledButton(
              key: const ValueKey('open-import'),
              onPressed: () => unawaited(
                showAppDialog<void>(
                  context,
                  secure: true,
                  builder: (context) => AutomationPortableImportDialog(
                    controller: x.controller,
                    originalScope: original,
                    labels: labels(context),
                  ),
                ),
              ),
              child: const Text('Open synthetic review'),
            ),
          ),
        ),
      ),
    ),
  );
  await tester.tap(find.byKey(const ValueKey('open-import')));
  await tester.pumpAndSettle();
}

Future<f.Registration> ready(WidgetTester tester, f.Fixture x) async {
  await tester.tap(find.byKey(const ValueKey('portable-import-choose')));
  await tester.pumpAndSettle();
  await tester.enterText(find.byKey(const ValueKey('portable-import-name')), 'Fresh import');
  await tester.enterText(find.byKey(const ValueKey('portable-import-password')), 'synthetic package password');
  await tester.tap(find.byKey(const ValueKey('portable-import-prepare')));
  await tester.pump();
  final op = x.gateway.calls.single;
  op.acknowledge();
  await tester.pump();
  op.session.previewResult.complete(jsonEncode(f.readyJson(op.intent)));
  await tester.pumpAndSettle();
  return op;
}

void main() {
  testWidgets('opening import review never selects a file, registers, accepts or reports installed', (tester) async {
    final x = f.Fixture();
    addTearDown(x.dispose);
    await open(tester, x);
    expect(x.files.opens, 0);
    expect(x.gateway.calls, isEmpty);
    expect(find.byKey(const ValueKey('portable-import-installed')), findsNothing);
    expect(tester.widget<FilledButton>(find.byKey(const ValueKey('portable-import-prepare'))).onPressed, isNull);
  });
  testWidgets(
    'explicit Prepare shows retained readonly Ready then only explicit Accept can install after actual join',
    (tester) async {
      final x = f.Fixture();
      addTearDown(x.dispose);
      await open(tester, x);
      final op = await ready(tester, x);
      expect(find.byKey(const ValueKey('portable-import-preview-only')), findsOneWidget);
      expect(find.textContaining(f.projectId), findsOneWidget);
      expect(op.session.accepts, 0);
      expect(op.retired.isCompleted, isFalse);
      expect(find.byKey(const ValueKey('portable-import-installed')), findsNothing);
      final reviewed = x.controller.ready!;
      await tester.tap(find.byKey(const ValueKey('portable-import-accept')));
      await tester.pump();
      expect(op.session.accepts, 1);
      op.session.acceptResult.complete(jsonEncode(f.installedJson(reviewed)));
      await tester.pump();
      expect(find.byKey(const ValueKey('portable-import-installed')), findsNothing);
      op.retire();
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('portable-import-installed')), findsOneWidget);
    },
  );
  testWidgets('explicit Edit retires captured Ready before input changes with no hidden reprepare', (tester) async {
    final x = f.Fixture();
    addTearDown(x.dispose);
    await open(tester, x);
    final op = await ready(tester, x);
    await tester.tap(find.byKey(const ValueKey('portable-import-edit')));
    await tester.pump();
    expect(find.byKey(const ValueKey('portable-import-accept')), findsNothing);
    expect(op.cancels, 1);
    op.retire();
    await tester.pumpAndSettle();
    await tester.enterText(find.byKey(const ValueKey('portable-import-name')), 'Fresh import ');
    await tester.pump();
    expect(x.gateway.calls.length, 1);
    expect(op.session.accepts, 0);
  });
  testWidgets('unknown accept response is framed recovery without raw diagnostics or retry button', (tester) async {
    final x = f.Fixture();
    addTearDown(x.dispose);
    await open(tester, x);
    final op = await ready(tester, x);
    await tester.tap(find.byKey(const ValueKey('portable-import-accept')));
    await tester.pump();
    op.session.acceptResult.completeError(StateError('SYNTHETIC_PRIVATE_DIAGNOSTIC'));
    op.retire();
    await tester.pumpAndSettle();
    expect(tester.widget<InfoBanner>(find.byKey(const ValueKey('portable-import-issue'))).tone, BannerTone.danger);
    expect(find.textContaining('SYNTHETIC_PRIVATE'), findsNothing);
    expect(find.byKey(const ValueKey('portable-import-prepare')), findsNothing);
    expect(find.byKey(const ValueKey('portable-import-accept')), findsNothing);
    expect(find.byKey(const ValueKey('portable-import-installed')), findsNothing);
  });
  testWidgets('real Native Ready retirement clears Accept and rebuild cannot renew the expired preview', (
    tester,
  ) async {
    final x = f.Fixture();
    addTearDown(x.dispose);
    await open(tester, x);
    final op = await ready(tester, x);
    op.retire();
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('portable-import-accept')), findsNothing);
    await tester.pump(const Duration(minutes: 10));
    expect(x.gateway.calls.length, 1);
    expect(op.session.accepts, 0);
    expect(x.controller.phase, PortableImportPhase.expired);
  });
  testWidgets('scope lock removes only own import route and late Ready cannot enter another profile', (tester) async {
    final x = f.Fixture();
    addTearDown(x.dispose);
    await open(tester, x);
    final op = await ready(tester, x);
    final context = tester.element(find.byType(AutomationPortableImportDialog));
    unawaited(
      showDialog<void>(
        context: context,
        builder: (_) => const AlertDialog(title: Text('Unrelated dialog')),
      ),
    );
    await tester.pumpAndSettle();
    x.events.boundary();
    await tester.pumpAndSettle();
    expect(find.byType(AutomationPortableImportDialog), findsNothing);
    expect(find.text('Unrelated dialog'), findsOneWidget);
    expect(op.cancels, greaterThanOrEqualTo(1));
  });
  for (final width in [1400.0, 500.0]) {
    testWidgets('Russian retained Ready layout at $width without imported-success claim', (tester) async {
      final x = f.Fixture();
      addTearDown(x.dispose);
      await loadAutomationProofFonts(tester);
      await open(tester, x, width: width, height: 1000, scale: width == 500 ? 1.5 : 1, locale: const Locale('ru'));
      final op = await ready(tester, x);
      expect(tester.takeException(), isNull);
      expect(op.session.accepts, 0);
      expect(find.byKey(const ValueKey('portable-import-installed')), findsNothing);
      final viewport = tester.getRect(find.byType(SingleChildScrollView).last);
      for (final key in ['portable-import-reviewed-name', 'portable-import-future-id', 'portable-import-summary']) {
        await tester.ensureVisible(find.byKey(ValueKey(key)));
        await tester.pumpAndSettle();
        final fact = tester.getRect(find.byKey(ValueKey(key)));
        expect(fact.top, greaterThanOrEqualTo(viewport.top));
        expect(fact.bottom, lessThanOrEqualTo(viewport.bottom));
      }
      await captureAutomationDialog(tester, 'portable-import-ready-ru-${width.toInt()}');
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
  }
  testWidgets('small Russian keyboard layout keeps footer review and close controls reachable', (tester) async {
    final x = f.Fixture();
    addTearDown(x.dispose);
    await open(tester, x, width: 500, height: 760, scale: 1.5, keyboard: 220, locale: const Locale('ru'));
    expect(tester.takeException(), isNull);
    expect(find.byKey(const ValueKey('portable-import-close')).hitTestable(), findsOneWidget);
    expect(find.byKey(const ValueKey('portable-import-prepare')).hitTestable(), findsOneWidget);
  });
}
