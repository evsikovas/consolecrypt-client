import 'dart:async';

import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/automation/automation_awx_connection_form.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../unit/automation_awx_models_test.dart' as f;
import 'automation_awx_workspace_test.dart' show settle;

Credential credential({
  String id = f.credentialId,
  String? username = 'operator',
  CredentialKind kind = CredentialKind.password,
  bool secret = true,
}) => Credential(
  id: ObjectId(id),
  name: 'Synthetic credential',
  kind: kind,
  createdAt: f.now,
  updatedAt: f.now,
  username: username,
  secretId: secret
      ? const ObjectId('55555555-5555-4555-8555-555555555555')
      : null,
);
Future<void> open(
  WidgetTester tester, {
  List<Credential>? credentials,
  bool loading = false,
  bool failed = false,
  bool Function()? current,
  Future<Credential?> Function(SourceAuth)? create,
  ValueChanged<MonitoringSource>? save,
  double width = 800,
  double scale = 1,
  bool edit = false,
}) async {
  tester.view.physicalSize = Size(width, 900);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    MaterialApp(
      theme: AppTheme.dark(platform: TargetPlatform.windows),
      localizationsDelegates: const [
        AppLocalizations.delegate,
        ...GlobalMaterialLocalizations.delegates,
      ],
      supportedLocales: AppLocalizations.supportedLocales,
      builder: (context, child) => MediaQuery(
        data: MediaQuery.of(context)
            .copyWith(textScaler: TextScaler.linear(scale)),
        child: child!,
      ),
      home: Scaffold(
        body: AwxConnectionForm(
          original: edit ? f.connection() : null,
          title: 'Owned AWX connection',
          disclosure: 'Device/profile-local settings',
          httpsNotice: 'HTTPS and saved refs only',
          credentials: credentials ?? [credential()],
          credentialsLoading: loading,
          credentialsFailed: failed,
          isCurrent: current ?? () => true,
          onChanged: () {},
          onCreateCredential: create ?? (_) async => null,
          onSave: save ?? (_) {},
          onCancel: () {},
        ),
      ),
    ),
  );
  await settle(tester);
}

Future<void> select(WidgetTester tester, String key, String label) async {
  final finder = find.byKey(ValueKey(key));
  await tester.ensureVisible(finder);
  await tester.tap(finder);
  await settle(tester);
  await tester.tap(find.text(label).last);
  await settle(tester);
}

Future<void> fill(
  WidgetTester tester, {
  String endpoint = 'https://awx.example.test/controller',
}) async {
  await tester.enterText(
    find.byKey(const ValueKey('awx-connection-name')),
    'New AWX',
  );
  await tester.enterText(
    find.byKey(const ValueKey('awx-connection-endpoint')),
    endpoint,
  );
  await settle(tester);
}

Future<void> save(WidgetTester tester) async {
  final b = find.byKey(const ValueKey('awx-connection-save'));
  await tester.ensureVisible(b);
  await tester.tap(b);
  await settle(tester);
}

void main() {
  test('eligibility uses existing password refs, Basic needs valid username, no secret resolution', () {
    expect(
      awxCredentialEligible(credential(username: null), SourceAuth.bearerRef),
      true,
    );
    expect(
      awxCredentialEligible(credential(username: null), SourceAuth.basicRef),
      false,
    );
    expect(
      awxCredentialEligible(credential(username: ' '), SourceAuth.basicRef),
      false,
    );
    expect(
      awxCredentialEligible(
        credential(username: 'bad:user'),
        SourceAuth.basicRef,
      ),
      false,
    );
    expect(
      awxCredentialEligible(
        credential(kind: CredentialKind.sshPrivateKey),
        SourceAuth.bearerRef,
      ),
      false,
    );
    expect(
      awxCredentialEligible(credential(secret: false), SourceAuth.bearerRef),
      false,
    );
    expect(awxCredentialEligible(credential(), SourceAuth.basicRef), true);
  });
  testWidgets(
    'only selected saved credential ID is returned; no token/password form field',
    (tester) async {
      MonitoringSource? saved;
      await open(tester, save: (v) => saved = v);
      await fill(tester);
      await select(
        tester,
        'awx-connection-credential',
        'Synthetic credential · operator',
      );
      await save(tester);
      expect(saved!.id, '');
      expect(saved!.credentialId, f.credentialId);
      expect(saved!.auth, SourceAuth.bearerRef);
      expect(
        saved!.toJson().keys,
        unorderedEquals(['id', 'name', 'endpoint', 'route', 'tls', 'auth']),
      );
      expect(
        find.byWidgetPredicate((w) => w is TextField && w.obscureText),
        findsNothing,
      );
    },
  );
  testWidgets(
    'explicit HTTP Basic rejected without silently rewriting endpoint',
    (tester) async {
      MonitoringSource? saved;
      await open(tester, save: (v) => saved = v);
      await fill(tester, endpoint: 'http://awx.example.test');
      await select(
        tester,
        'awx-connection-auth',
        tester.element(find.byType(AwxConnectionForm)).l10n.monitoringBasicAuth,
      );
      await select(
        tester,
        'awx-connection-credential',
        'Synthetic credential · operator',
      );
      await save(tester);
      expect(saved, isNull);
      expect(
        find.byKey(const ValueKey('awx-connection-invalid')),
        findsOneWidget,
      );
      expect(
        tester
            .widget<TextField>(
              find.byKey(const ValueKey('awx-connection-endpoint')),
            )
            .controller!
            .text,
        'http://awx.example.test',
      );
    },
  );
  testWidgets('Basic cannot silently use a password ref lacking username', (
    tester,
  ) async {
    await open(tester, credentials: [credential(username: null)]);
    await fill(tester);
    await select(tester, 'awx-connection-credential', 'Synthetic credential');
    await select(
      tester,
      'awx-connection-auth',
      tester.element(find.byType(AwxConnectionForm)).l10n.monitoringBasicAuth,
    );
    expect(
      tester
          .widget<GlassButton>(
            find.byKey(const ValueKey('awx-connection-save')),
          )
          .onPressed,
      isNull,
    );
  });
  testWidgets(
    'metadata loading/error disables Save without treating empty as ready',
    (tester) async {
      await open(tester, loading: true, edit: true);
      expect(
        tester
            .widget<GlassButton>(
              find.byKey(const ValueKey('awx-connection-save')),
            )
            .onPressed,
        isNull,
      );
      await open(tester, failed: true, edit: true);
      expect(
        tester
            .widget<GlassButton>(
              find.byKey(const ValueKey('awx-connection-save')),
            )
            .onPressed,
        isNull,
      );
    },
  );
  testWidgets(
    'pending creation after original scope retirement never selects returned credential',
    (tester) async {
      var current = true;
      final pending = Completer<Credential?>();
      await open(tester, current: () => current, create: (_) => pending.future);
      await tester.tap(find.byKey(const ValueKey('awx-create-credential')));
      await settle(tester);
      current = false;
      pending.complete(credential());
      await settle(tester);
      expect(
        tester
            .widget<GlassButton>(
              find.byKey(const ValueKey('awx-connection-save')),
            )
            .onPressed,
        isNull,
      );
    },
  );
  testWidgets(
    'edit returns same connection ID; Native generates IDs only on create',
    (tester) async {
      MonitoringSource? saved;
      await open(tester, edit: true, save: (v) => saved = v);
      await save(tester);
      expect(saved!.id, f.connectionId);
      expect(saved!.credentialId, f.credentialId);
    },
  );
  testWidgets(
    '420px scale1.5 form keeps fixed Save/Cancel and bounded scrolling fields',
    (tester) async {
      await open(tester, width: 420, scale: 1.5);
      expect(tester.takeException(), isNull);
      final b = find.byKey(const ValueKey('awx-connection-save'));
      expect(b.hitTestable(), findsOneWidget);
      expect(find.byType(SingleChildScrollView), findsOneWidget);
    },
  );
}
