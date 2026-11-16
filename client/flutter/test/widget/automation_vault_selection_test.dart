import 'dart:async';

import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/automation/automation_vault_selection.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart' show settle, tapKey, isEnabled;

const profile = '11111111-1111-4111-8111-111111111111';
const project = '22222222-2222-4222-8222-222222222222';
const credential = '33333333-3333-4333-8333-333333333333';
const scope = AutomationScope(profile, true);
AutomationJson contextValue() => {
  'version': 1,
  'referencedCredentialId': credential,
  'diffSupported': false,
  'frame': {
    'version': 1,
    'profileId': profile,
    'notificationScopeId': '44444444-4444-4444-8444-444444444444',
    'projectId': project,
    'projectRevision': 7,
    'catalogSha256': 'a' * 64,
    'revisionDigest': 'b' * 64,
    'directoryIdentity': 'c' * 64,
    'referenceSha256': null,
  },
};

class VaultService implements AutomationVaultSelectionService {
  final requests = <AutomationJson>[];
  Completer<AutomationJson>? pending;
  AutomationJson response = contextValue();
  @override
  Future<AutomationJson> vaultContext(String id) async {
    expect(id, project);
    return pending?.future ?? response;
  }

  @override
  Future<AutomationJson> selectVault(AutomationJson selection) async {
    requests.add(selection);
    return pending?.future ?? response;
  }
}

Future<void> mount(
  WidgetTester tester,
  VaultService service,
  List<({bool enabled, AutomationJson? selection})> changes, {
  AutomationScope ownerScope = scope,
}) async {
  final date = DateTime.utc(2026);
  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        automationScopeProvider.overrideWithValue(ownerScope),
        credentialsProvider.overrideWith(
          (_) => Stream.value([
            Credential(
              id: const ObjectId(credential),
              name: 'Vault fixture password',
              kind: CredentialKind.password,
              secretId: const ObjectId('55555555-5555-4555-8555-555555555555'),
              createdAt: date,
              updatedAt: date,
            ),
            Credential(
              id: const ObjectId('66666666-6666-4666-8666-666666666666'),
              name: 'SSH key excluded',
              kind: CredentialKind.sshPrivateKey,
              createdAt: date,
              updatedAt: date,
            ),
          ]),
        ),
      ],
      child: MaterialApp(
        theme: AppTheme.dark(platform: TargetPlatform.windows),
        locale: const Locale('en'),
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        supportedLocales: AppLocalizations.supportedLocales,
        home: Scaffold(
          body: SingleChildScrollView(
            child: AutomationVaultSelection(
              scope: scope,
              projectId: project,
              service: service,
              onChanged: (enabled, selection) => changes.add((enabled: enabled, selection: selection)),
            ),
          ),
        ),
      ),
    ),
  );
  await settle(tester);
}

void pick(WidgetTester tester) => tester
    .widget<GlassSelect<String?>>(find.byKey(const ValueKey('automation-vault-credential')))
    .onChanged!(credential);

void main() {
  testWidgets('existing ref is not consent; explicit selection writes only metadata then enables request', (
    tester,
  ) async {
    final service = VaultService(), changes = <({bool enabled, AutomationJson? selection})>[];
    await mount(tester, service, changes);
    expect(service.requests, isEmpty);
    await tapKey(tester, 'automation-vault-enable');
    expect(changes.last.selection, isNull);
    expect(isEnabled(tester, 'automation-vault-save'), isFalse);
    final picker = tester.widget<GlassSelect<String?>>(find.byKey(const ValueKey('automation-vault-credential')));
    expect(picker.items.length, 2);
    pick(tester);
    await tester.pump();
    await tapKey(tester, 'automation-vault-save');
    expect(service.requests.single.keys.toSet(), {'frame', 'credentialId'});
    expect(service.requests.single['credentialId'], credential);
    expect(changes.last.selection!['vault_credential_id'], credential);
    expect(changes.last.selection!['vault_selection'], contextValue()['frame']);
    expect(find.text('Vault password selected; validate the project before running.'), findsOneWidget);
    await tapKey(tester, 'automation-vault-enable');
    expect(changes.last, (enabled: false, selection: null));
    expect(service.requests.length, 1);
  });

  testWidgets('late saved selection after disabling cannot reactivate Vault', (tester) async {
    final service = VaultService(), changes = <({bool enabled, AutomationJson? selection})>[];
    await mount(tester, service, changes);
    await tapKey(tester, 'automation-vault-enable');
    pick(tester);
    await tester.pump();
    service.pending = Completer();
    await tapKey(tester, 'automation-vault-save');
    await tapKey(tester, 'automation-vault-enable');
    service.pending!.complete(contextValue());
    await settle(tester);
    expect(changes.last, (enabled: false, selection: null));
    expect(changes.where((value) => value.selection != null), isEmpty);
  });

  testWidgets('foreign original profile frame is rejected before write', (tester) async {
    final service = VaultService(), changes = <({bool enabled, AutomationJson? selection})>[];
    (service.response['frame'] as AutomationJson)['profileId'] = project;
    await mount(tester, service, changes);
    await tapKey(tester, 'automation-vault-enable');
    pick(tester);
    await tester.pump();
    expect(isEnabled(tester, 'automation-vault-save'), isFalse);
    expect(service.requests, isEmpty);
    expect(changes.where((value) => value.selection != null), isEmpty);
  });

  testWidgets('unmounted original owner ignores a late save', (tester) async {
    final service = VaultService(), changes = <({bool enabled, AutomationJson? selection})>[];
    await mount(tester, service, changes);
    await tapKey(tester, 'automation-vault-enable');
    pick(tester);
    await tester.pump();
    service.pending = Completer();
    await tapKey(tester, 'automation-vault-save');
    await tester.pumpWidget(const SizedBox());
    service.pending!.complete(contextValue());
    await settle(tester);
    expect(changes.where((value) => value.selection != null), isEmpty);
    expect(tester.takeException(), isNull);
  });

  testWidgets('same-profile new unlock owner hides old selector and ignores its late save', (tester) async {
    final service = VaultService(), changes = <({bool enabled, AutomationJson? selection})>[];
    await mount(tester, service, changes);
    await tapKey(tester, 'automation-vault-enable');
    pick(tester);
    await tester.pump();
    service.pending = Completer();
    await tapKey(tester, 'automation-vault-save');
    // Distinct unlock identity is the causal input; const would reuse scope.
    // ignore: prefer_const_constructors
    await mount(tester, service, changes, ownerScope: AutomationScope(profile, true));
    expect(find.byKey(const ValueKey('automation-vault-enable')), findsNothing);
    service.pending!.complete(contextValue());
    await settle(tester);
    expect(changes.where((value) => value.selection != null), isEmpty);
    expect(tester.takeException(), isNull);
  });
}
