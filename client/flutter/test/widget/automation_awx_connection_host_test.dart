import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/automation/automation_awx_connection_form.dart';
import 'package:consolecrypt/automation/automation_awx_connection_host.dart';
import 'package:consolecrypt/automation/automation_awx_controller.dart';
import 'package:consolecrypt/automation/automation_awx_models.dart';
import 'package:consolecrypt/automation/automation_awx_workspace.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/runtime_notice_adapter.dart';
import 'package:consolecrypt/core/widgets/secret_field.dart';
import 'package:consolecrypt/credentials/credential_dialogs.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/awx_copy.dart';
import '../helpers/test_app.dart' show testBackend;
import '../unit/automation_awx_controller_test.dart' as f;
import '../unit/automation_awx_models_test.dart' as m;
import 'automation_awx_workspace_test.dart' show settle, tap;

class Native extends f.Native {
  Native(this.profile);
  final String profile;
  MonitoringSource? saved;
  int? expectedRevision;
  @override
  Future<AwxNativeScope> captureScope() async => AwxNativeScope.fromJson({
    'version': 1,
    'profileId': profile,
    'notificationScopeId': m.nonce,
  });
  @override
  Future<AwxCatalog> saveConnection(
    AwxNativeScope original, {
    required int expectedCatalogRevision,
    required int? expectedRevision,
    required MonitoringSource configuration,
  }) async {
    expect(original.profileId, profile);
    this.expectedRevision = expectedRevision;
    saved = configuration;
    calls.add('save');
    return currentCatalog;
  }
}

Future<(MockBackend, AwxController, Native)> open(WidgetTester tester) async {
  final backend = testBackend(seed: false);
  addTearDown(backend.dispose);
  await backend.debugCreateUnlockedLocalProfile(name: 'Owned AWX form profile');
  final profile = backend.services.profiles.currentProfiles.active!.id.value;
  final hub = RuntimeNoticeEventAdapter(), native = Native(profile);
  addTearDown(hub.dispose);
  var id = 0;
  final c = AwxController(
    profileId: profile,
    events: hub,
    isOriginalUiCurrent: () =>
        backend.services.profiles.currentProfiles.active?.id.value == profile &&
        backend.services.vault.currentStatus.isUnlocked,
    operationId: () =>
        '66666666-6666-4666-8666-${(++id).toString().padLeft(12, '0')}',
    gateway: native,
    now: () => m.now,
  );
  addTearDown(c.dispose);
  await c.loadCatalog();
  tester.view.physicalSize = const Size(1200, 1000);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    ProviderScope(
      overrides: [appServicesProvider.overrideWithValue(backend.services)],
      child: MaterialApp(
        theme: AppTheme.dark(platform: TargetPlatform.windows),
        supportedLocales: AppLocalizations.supportedLocales,
        localizationsDelegates: const [
          AppLocalizations.delegate,
          ...GlobalMaterialLocalizations.delegates,
        ],
        home: Scaffold(
          body: AwxConnectionWorkspace(
            controller: c,
            copy: awxCopy,
            httpsNotice: 'HTTPS and saved refs only',
            onOpenResource: (_) async {},
          ),
        ),
      ),
    ),
  );
  await settle(tester);
  return (backend, c, native);
}

Future<void> fillSource(WidgetTester tester) async {
  await tester.enterText(
    find.byKey(const ValueKey('awx-connection-name')),
    'Owned AWX',
  );
  await tester.enterText(
    find.byKey(const ValueKey('awx-connection-endpoint')),
    'https://awx.example.test/controller',
  );
  await settle(tester);
}

Future<void> basic(WidgetTester tester) async {
  final l = tester.element(find.byType(AwxConnectionForm)).l10n;
  await tester.tap(find.byKey(const ValueKey('awx-connection-auth')));
  await settle(tester);
  await tester.tap(find.text(l.monitoringBasicAuth).last);
  await settle(tester);
}

void main() {
  testWidgets(
    'actual existing secure credential editor refreshes metadata and selects new eligible Basic ref',
    (tester) async {
      final (_, _, native) = await open(tester);
      await tap(tester, 'awx-connection-add');
      await fillSource(tester);
      await basic(tester);
      await tap(tester, 'awx-create-credential');
      expect(find.byType(PasswordCredentialDialog), findsOneWidget);
      expect(
        tester
            .widget<PasswordCredentialDialog>(
              find.byType(PasswordCredentialDialog),
            )
            .requireUsername,
        true,
      );
      await tester.enterText(
        find.byKey(const ValueKey('password-credential-name')),
        'Owned AWX Basic',
      );
      final secret = find.descendant(
        of: find.byType(SecretField),
        matching: find.byType(TextField),
      );
      await tester.enterText(secret, 'synthetic owned credential value');
      await tap(tester, 'credential-save');
      expect(find.byType(PasswordCredentialDialog), findsOneWidget);
      expect(
        ProviderScope.containerOf(tester.element(find.byType(AwxConnectionWorkspace))).read(credentialsProvider).value,
        isEmpty,
      );
      await tester.enterText(
        find.byKey(const ValueKey('password-credential-username')),
        'operator',
      );
      await tap(tester, 'credential-save');
      await settle(tester);
      expect(find.byType(PasswordCredentialDialog), findsNothing);
      final savedCredentials = ProviderScope.containerOf(tester.element(find.byType(AwxConnectionWorkspace))).read(credentialsProvider).value!;
      expect(savedCredentials.length, 1);
      expect(find.text('Owned AWX Basic · operator'), findsOneWidget);
      await tap(tester, 'awx-connection-save');
      expect(native.saved, isNotNull);
      expect(native.saved!.id, '');
      expect(native.expectedRevision, isNull);
      expect(native.saved!.auth, SourceAuth.basicRef);
      expect(native.saved!.credentialId, savedCredentials.single.id.value);
      expect(
        native.saved!.toJson().toString().contains(
          'synthetic owned credential value',
        ),
        false,
      );
      expect(native.calls.contains('launch'), false);
    },
  );
  testWidgets(
    'locking while nested secure editor open closes both original routes and sends no source save',
    (tester) async {
      final (backend, _, native) = await open(tester);
      await tap(tester, 'awx-connection-add');
      await tap(tester, 'awx-create-credential');
      await tester.enterText(
        find.byKey(const ValueKey('password-credential-name')),
        'Do not publish',
      );
      await backend.services.vault.lock();
      await settle(tester);
      expect(find.byType(PasswordCredentialDialog), findsNothing);
      expect(find.byType(AwxConnectionForm), findsNothing);
      expect(native.saved, isNull);
      expect(native.calls.contains('launch'), false);
    },
  );
  testWidgets(
    'catalog replacement during connection editor retires original draft instead of saving with latest CAS',
    (tester) async {
      final (_, c, native) = await open(tester);
      await tap(tester, 'awx-connection-add');
      await fillSource(tester);
      native.currentCatalog = m.catalog(revision: 8);
      await c.loadCatalog();
      await settle(tester);
      expect(find.byType(AwxConnectionForm), findsNothing);
      expect(native.saved, isNull);
    },
  );
  testWidgets(
    'opening/canceling connection edit invalidates review before first await and never launches',
    (tester) async {
      final (_, c, native) = await open(tester);
      c.selectConnection(c.catalog!.connections.single);
      await c.loadTemplates();
      c.selectTemplate(c.templates!.templates.single);
      await c.prepareLaunch();
      await settle(tester);
      expect(c.review, isNotNull);
      final edit = find.text(awxCopy(AwxCopy.editConnection));
      await tester.ensureVisible(edit);
      await tester.tap(edit);
      await settle(tester);
      expect(c.review, isNull);
      expect(native.discards, ['synthetic-one-use']);
      final l = tester.element(find.byType(AwxConnectionForm)).l10n;
      await tester.tap(find.text(l.commonCancel).last);
      await settle(tester);
      expect(native.saved, isNull);
      expect(native.calls.contains('launch'), false);
      expect(c.canLaunch, false);
    },
  );
}
