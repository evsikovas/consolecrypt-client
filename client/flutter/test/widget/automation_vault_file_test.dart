import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/automation/automation_vault_file_dialog.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

const _profile = '11111111-1111-4111-8111-111111111111';
const _scope = AutomationScope(_profile, true);
const _project = AutomationProject(
  id: '22222222-2222-4222-8222-222222222222',
  name: 'Synthetic Vault',
  revision: 7,
  readOnly: false,
);
const _clear = AutomationProjectFile(
  path: 'vars.yml',
  text: 'value: fixture\n',
  sha256: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
  lineEndings: 'lf',
);
const _encrypted = AutomationProjectFile(
  path: 'vars.yml',
  text: '\$ANSIBLE_VAULT;1.1;AES256\n012345\n',
  sha256: 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
  lineEndings: 'lf',
);

class _Service implements AutomationVaultFileService {
  final requests = <AutomationJson>[];
  SecretText? receivedPassword;
  AutomationProjectFile? result;
  Completer<AutomationJson>? pendingContext;
  Completer<AutomationProjectFile>? pendingTransform;
  bool active = true;
  AutomationProjectFile? saved;
  @override
  Future<AutomationJson> vaultFileContext(String projectId) async =>
      pendingContext?.future ??
      {
        'frame': {
          'version': 1,
          'profileId': _profile,
          'notificationScopeId': 'original-native-nonce',
          'projectId': projectId,
          'projectRevision': 7,
        },
      };
  @override
  Future<AutomationProjectFile> transformVaultFile(AutomationJson request, {SecretText? password}) async {
    requests.add(request);
    receivedPassword = password;
    if (password != null) expect(password.expose(), 'synthetic-password');
    return pendingTransform?.future ?? result ?? _encrypted;
  }
}

Future<void> _mount(WidgetTester tester, _Service service, {AutomationProjectFile file = _clear}) async {
  final backend = testBackend(seed: false);
  addTearDown(backend.dispose);
  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        appServicesProvider.overrideWithValue(backend.services),
        automationScopeProvider.overrideWithValue(_scope),
      ],
      child: MaterialApp(
        theme: AppTheme.dark(platform: TargetPlatform.windows),
        locale: const Locale('en'),
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        supportedLocales: AppLocalizations.supportedLocales,
        home: Consumer(
          builder: (context, ref, _) => Scaffold(
            body: TextButton(
              key: const ValueKey('open-vault-file'),
              onPressed: () async {
                service.saved = await showAutomationVaultFileDialog(
                  context,
                  ref,
                  scope: _scope,
                  project: _project,
                  file: file,
                  service: service,
                  originalCurrent: () => service.active,
                );
              },
              child: const Text('Open'),
            ),
          ),
        ),
      ),
    ),
  );
  await settle(tester);
  await tapKey(tester, 'open-vault-file');
}

Future<void> _typePassword(WidgetTester tester) async {
  final field = find.descendant(
    of: find.byKey(const ValueKey('automation-vault-file-password')),
    matching: find.byType(TextFormField),
  );
  expect(
    tester.widget<EditableText>(find.descendant(of: field, matching: find.byType(EditableText))).obscureText,
    isTrue,
  );
  await tester.enterText(field, 'synthetic-password');
  await settle(tester);
}

void main() {
  testWidgets('masked manual password stays out of request JSON and is wiped after CAS', (tester) async {
    final service = _Service();
    await _mount(tester, service);
    await _typePassword(tester);
    await tapKey(tester, 'automation-vault-file-confirm');
    expect(service.saved, same(_encrypted));
    expect(service.receivedPassword!.isWiped, isTrue);
    expect(service.requests.single, containsPair('passwordSource', 'manual'));
    expect(service.requests.single, containsPair('expectedSha256', _clear.sha256));
    expect(service.requests.single, containsPair('notificationScopeId', 'original-native-nonce'));
    expect(jsonEncode(service.requests), isNot(contains('synthetic-password')));
    expect(tester.takeException(), isNull);
  });

  testWidgets('project password source sends no password bytes or arbitrary path', (tester) async {
    final service = _Service();
    await _mount(tester, service);
    tester.widget<GlassSelect<bool>>(find.byKey(const ValueKey('automation-vault-file-source'))).onChanged!(true);
    await settle(tester);
    expect(find.byKey(const ValueKey('automation-vault-file-password')), findsNothing);
    await tapKey(tester, 'automation-vault-file-confirm');
    expect(service.receivedPassword, isNull);
    expect(service.requests.single, containsPair('passwordSource', 'projectFile'));
    expect(service.requests.single.keys, isNot(contains('passwordPath')));
    expect(service.saved, same(_encrypted));
    expect(tester.takeException(), isNull);
  });

  testWidgets('encrypted YAML uses explicit decrypt and saves only observed changed file', (tester) async {
    final service = _Service()..result = _clear;
    await _mount(tester, service, file: _encrypted);
    expect(find.text('Decrypt YAML'), findsNWidgets(2));
    await _typePassword(tester);
    await tapKey(tester, 'automation-vault-file-confirm');
    expect(service.requests.single, containsPair('action', 'decrypt'));
    expect(service.saved, same(_clear));
    expect(service.receivedPassword!.isWiped, isTrue);
    expect(tester.takeException(), isNull);
  });

  testWidgets('retired original selection ignores late transform and still wipes secret', (tester) async {
    final service = _Service()..pendingTransform = Completer();
    await _mount(tester, service);
    await _typePassword(tester);
    await tapKey(tester, 'automation-vault-file-confirm');
    service.active = false;
    service.pendingTransform!.complete(_encrypted);
    await settle(tester);
    expect(service.saved, isNull);
    expect(service.receivedPassword!.isWiped, isTrue);
    expect(tester.takeException(), isNull);
  });
}
