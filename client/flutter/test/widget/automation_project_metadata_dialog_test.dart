import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/automation/automation_project_metadata.dart';
import 'package:consolecrypt/automation/automation_project_metadata_dialog.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../support/project_metadata_fixture.dart' as f;

final publish = find.byKey(const ValueKey('project-metadata-publish'));
final refresh = find.byKey(const ValueKey('project-metadata-refresh'));
final row = find.byKey(const ValueKey('project-metadata-record-${f.projectId}'));

final class _ConsentFixture extends f.MetadataFake
    implements AutomationProjectSettingsSyncService, ProjectSettingsReview {
  int confirmations = 0;
  int cancellations = 0;
  final scope = {'version': 1, 'profileId': f.profile, 'notificationScopeId': f.nonce, 'scope': 'allPersonalVaults'};

  @override
  Future<ProjectSettingsReview> prepareProjectSettingsSync(String profileId, String nonce) async {
    expect(profileId, f.profile);
    expect(nonce, f.nonce);
    return this;
  }

  @override
  Future<String> details() async => jsonEncode({...scope, 'canConfirm': true, 'releasedMinimumClientVersion': '2.0.0'});

  @override
  Future<String> confirm() async {
    confirmations++;
    return jsonEncode({...scope, 'action': 'accountPolicyConfirmed'});
  }

  @override
  Future<String> recheckCheckpoint() async => throw StateError('Expected owner confirmation');

  @override
  void cancel() => cancellations++;
}

Future<ValueNotifier<bool>> host(WidgetTester tester, f.MetadataFake service, {String language = 'en'}) async {
  final live = ValueNotifier(true);
  addTearDown(live.dispose);
  await tester.pumpWidget(
    MaterialApp(
      locale: Locale(language),
      supportedLocales: AppLocalizations.supportedLocales,
      localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
      home: Scaffold(
        body: Builder(
          builder: (context) => TextButton(
            key: const ValueKey('open-metadata'),
            onPressed: () => showGlassDialog<void>(
              context,
              builder: (_) => AutomationProjectMetadataDialog(
                service: service,
                project: f.project,
                profileId: f.profile,
                nonce: f.nonce,
                originalCurrent: () => live.value,
                originalLive: live,
              ),
            ),
            child: const Text('Open owned project metadata'),
          ),
        ),
      ),
    ),
  );
  return live;
}

Future<void> open(WidgetTester tester) async {
  await tester.tap(find.byKey(const ValueKey('open-metadata')));
  await tester.pump();
  await tester.pump(const Duration(milliseconds: 350));
}

Future<void> tap(WidgetTester tester, Finder button) async {
  await tester.ensureVisible(button);
  await tester.tap(button);
  await tester.pump();
  await tester.pump(const Duration(milliseconds: 350));
}

void main() {
  testWidgets('account setup presents the minimum and waits for explicit owner confirmation', (tester) async {
    final service = _ConsentFixture()..candidate = true;
    await host(tester, service, language: 'ru');
    await open(tester);
    final l = AppLocalizations.of(tester.element(publish));
    await tap(tester, find.widgetWithText(GlassButton, l.automationSettingsSyncSetup));
    expect(find.text(l.automationSettingsSyncConfirmHint('2.0.0')), findsOneWidget);
    expect(service.confirmations, 0);
    await tap(tester, find.widgetWithText(GlassButton, l.automationSettingsSyncEnable));
    expect(find.text(l.automationSettingsSyncEnabled), findsOneWidget);
    expect(service.confirmations, 1);
    expect(service.publishes, 0);
    expect(service.cancellations, 1);
  });

  testWidgets('opening reads context once and time/rebuild never publishes', (tester) async {
    final service = f.MetadataFake()..candidate = true;
    await host(tester, service);
    expect(service.reads, 0);
    expect(service.publishes, 0);
    await open(tester);
    expect(service.reads, 1);
    expect(service.catalogs, 0);
    expect(service.publishes, 0);
    await tester.pump(const Duration(seconds: 65));
    expect(service.reads, 1);
    expect(service.publishes, 0);
    expect(tester.widget<GlassButton>(publish).onPressed, isNotNull);
  });

  testWidgets('undeclared compatibility floor leaves explicit publication disabled', (tester) async {
    final service = f.MetadataFake();
    await host(tester, service);
    await open(tester);
    final l = AppLocalizations.of(tester.element(publish));
    expect(find.text(l.automationMetadataCompatibility), findsOneWidget);
    expect(tester.widget<GlassButton>(publish).onPressed, isNull);
    await tap(tester, publish);
    expect(service.publishes, 0);
    expect(service.reads, 1);
  });

  testWidgets('explicit candidate publication sends once and renders only a locked reference', (tester) async {
    // This synthetic floor is test admission metadata, not a released version.
    final service = f.MetadataFake()..candidate = true;
    await host(tester, service);
    await open(tester);
    expect(row, findsNothing);
    await tap(tester, publish);
    expect(service.publishes, 1);
    expect(service.request, {
      ...(f.context(candidate: true)['frame'] as AutomationJson),
      'expected_reference_revision': 0,
    });
    expect(row, findsOneWidget);
    expect(find.descendant(of: row, matching: find.byIcon(Icons.lock_outline)), findsOneWidget);
    expect(find.descendant(of: row, matching: find.byType(GlassButton)), findsNothing);
    final l = AppLocalizations.of(tester.element(publish));
    expect(find.text(l.automationMetadataPublished), findsOneWidget);
    expect(tester.widget<GlassButton>(publish).onPressed, isNull);
    await tap(tester, publish);
    expect(service.publishes, 1);
  });

  testWidgets('scope retirement while context waits never adopts the late candidate', (tester) async {
    final service = f.MetadataFake()..pending = Completer<AutomationJson>();
    final live = await host(tester, service);
    await open(tester);
    expect(service.reads, 1);
    live.value = false;
    await tester.pump();
    service.pending!.complete(f.context(candidate: true));
    await tester.pump();
    final l = AppLocalizations.of(tester.element(publish));
    expect(find.text(l.automationMetadataChanged), findsOneWidget);
    expect(row, findsNothing);
    expect(tester.widget<GlassButton>(publish).onPressed, isNull);
    expect(tester.widget<GlassButton>(refresh).onPressed, isNull);
    expect(service.publishes, 0);
    expect(tester.takeException(), isNull);
  });

  testWidgets('scope retirement after send never adopts a late published row', (tester) async {
    final service = f.MetadataFake()
      ..candidate = true
      ..publishing = Completer<AutomationJson>();
    final live = await host(tester, service);
    await open(tester);
    await tap(tester, publish);
    expect(service.publishes, 1);
    live.value = false;
    await tester.pump();
    service.publishing!.complete(f.reply(published: true));
    await tester.pump();
    final l = AppLocalizations.of(tester.element(publish));
    expect(find.text(l.automationMetadataChanged), findsOneWidget);
    expect(find.text(l.automationMetadataPublished), findsNothing);
    expect(row, findsNothing);
    expect(tester.widget<GlassButton>(publish).onPressed, isNull);
    expect(service.publishes, 1);
  });

  testWidgets('uncertain publication needs explicit catalog read and never retries itself', (tester) async {
    final service = f.MetadataFake()
      ..candidate = true
      ..published = {'version': 99};
    await host(tester, service);
    await open(tester);
    await tap(tester, publish);
    final l = AppLocalizations.of(tester.element(publish));
    expect(find.text(l.automationMetadataUnknown), findsOneWidget);
    expect(tester.widget<GlassButton>(publish).onPressed, isNull);
    await tester.pump(const Duration(seconds: 65));
    expect(service.publishes, 1);
    expect(service.reads, 1);
    expect(service.catalogs, 0);
    await tap(tester, refresh);
    expect(service.catalogs, 1);
    expect(service.reads, 1);
    expect(service.publishes, 1);
    expect(tester.widget<GlassButton>(publish).onPressed, isNull);
  });

  for (final language in ['ru', 'en']) {
    testWidgets('compact $language keeps disclosures and actions visible without overlap', (tester) async {
      tester.view.physicalSize = const Size(640, 480);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      final service = f.MetadataFake();
      await host(tester, service, language: language);
      await open(tester);
      final l = AppLocalizations.of(tester.element(publish));
      final close = find.widgetWithText(GlassButton, l.commonClose);
      for (final button in [publish, refresh, close]) {
        expect(button.hitTestable(), findsOneWidget);
        final rect = tester.getRect(button);
        expect(rect.left, greaterThanOrEqualTo(0));
        expect(rect.right, lessThanOrEqualTo(640));
        expect(rect.bottom, lessThanOrEqualTo(480));
      }
      expect(tester.getRect(publish).overlaps(tester.getRect(refresh)), isFalse);
      expect(tester.getRect(publish).overlaps(tester.getRect(close)), isFalse);
      // Full disclosures remain scrollable; actions stay outside the viewport.
      for (final text in [l.automationMetadataHint, l.automationMetadataCompatibility]) {
        final disclosure = find.text(text);
        await tester.ensureVisible(disclosure);
        await tester.pump();
        expect(tester.getRect(disclosure).bottom, lessThanOrEqualTo(tester.getRect(refresh).top));
      }
      expect(find.text(l.automationMetadataCompatibility), findsOneWidget);
      expect(tester.takeException(), isNull);
      expect(service.publishes, 0);
    });
  }
}
