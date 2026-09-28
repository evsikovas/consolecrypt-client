import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

Host hostNamed(MockBackend b, String name) => b.inventory.currentHosts.firstWhere((h) => h.name == name);

List<Credential> credentialsOf(MockBackend b) => b.cloud.activeData!.credentials;

String? storedSecret(MockBackend b, ObjectId id) => b.cloud.activeData!.secrets[id]?.expose();

Future<void> fillNewHost(WidgetTester tester, String name) async {
  await tapKey(tester, 'add-host');
  await enterKey(tester, 'host-name', name);
  await enterKey(tester, 'host-address', '$name.example.net');
  await enterKey(tester, 'host-username', 'ops');
  await settle(tester);
}

Future<void> openHost(WidgetTester tester, String name) async {
  await enterKey(tester, 'hosts-search', name);
  await tapKey(tester, 'host-menu-$name');
  await tapKey(tester, 'host-edit-menu-$name');
}

Future<void> chooseMode(WidgetTester tester, String label) async {
  await tester.tap(find.descendant(of: find.byKey(const ValueKey('auth-mode')), matching: find.text(label)));
  await settle(tester);
}

void main() {
  testWidgets('inline password: a Password credential is created, linked and never shown again', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);
    final before = credentialsOf(backend).length;

    await fillNewHost(tester, 'grafana');
    // Password is the default mode; "Save password in vault" is on by default.
    expect(find.byKey(const ValueKey('auth-section')), findsOneWidget);
    await enterKey(tester, 'host-password', 'hunter2-grafana');
    await settle(tester);
    expect(find.text('Password · saved in vault'), findsOneWidget, reason: 'effective preview');
    await tapKey(tester, 'save-host');

    final host = hostNamed(backend, 'grafana');
    expect(credentialsOf(backend), hasLength(before + 1));
    final cred = credentialsOf(backend).firstWhere((c) => c.id == host.credentialId);
    expect(cred.kind, CredentialKind.password);
    expect(cred.username, 'ops');
    expect(host.inlineCredentialId, cred.id);
    expect(host.promptsForPassword, isFalse);
    expect(storedSecret(backend, cred.id), 'hunter2-grafana');

    // Editing shows that a password is saved — never its value.
    await openHost(tester, 'grafana');
    expect(find.byKey(const ValueKey('password-saved')), findsOneWidget);
    expect(find.text('••••••'), findsOneWidget);
    expect(find.text('hunter2-grafana'), findsNothing);

    // Change: same credential, new secret.
    await tapKey(tester, 'password-change');
    await enterKey(tester, 'host-password', 'new-secret-value');
    await tapKey(tester, 'save-host');
    final changed = hostNamed(backend, 'grafana');
    expect(changed.credentialId, cred.id);
    expect(storedSecret(backend, cred.id), 'new-secret-value');

    // Remove: the inline credential and its secret are deleted.
    await openHost(tester, 'grafana');
    await tapKey(tester, 'password-remove');
    await tapKey(tester, 'save-host');
    final removed = hostNamed(backend, 'grafana');
    expect(removed.credentialId, isNull);
    expect(removed.promptsForPassword, isTrue);
    expect(credentialsOf(backend), hasLength(before));
    expect(storedSecret(backend, cred.id), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('unchecked "Save password": nothing is stored and connecting asks for the password', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);
    final before = credentialsOf(backend).length;

    await fillNewHost(tester, 'nopass');
    await tapKey(tester, 'save-password');
    expect(find.byKey(const ValueKey('host-password')), findsNothing, reason: 'nothing to type when not saving');
    expect(find.text('Password · asked when connecting'), findsOneWidget);
    await tapKey(tester, 'save-host');

    final host = hostNamed(backend, 'nopass');
    expect(credentialsOf(backend), hasLength(before), reason: 'no credential created');
    expect(host.credentialId, isNull);
    expect(host.promptsForPassword, isTrue);

    // Connect: host key first (unknown host), then the password prompt.
    await enterKey(tester, 'hosts-search', 'nopass');
    await tapKey(tester, 'connect-nopass');
    await tapKey(tester, 'accept-host-key');
    expect(find.byKey(const ValueKey('terminal-password-prompt')), findsOneWidget);
    await enterKey(tester, 'terminal-password', 'typed-at-connect');
    await tapKey(tester, 'terminal-password-submit');
    expect(find.byKey(const ValueKey('terminal-password-prompt')), findsNothing);
    expect(find.byKey(const ValueKey('reconnect-banner')), findsNothing, reason: 'connected');
    expect(credentialsOf(backend), hasLength(before), reason: 'the typed password is not persisted');
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('SSH key mode: "Generate Ed25519…" inline creates and links a key', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);

    await fillNewHost(tester, 'keyed');
    await chooseMode(tester, 'SSH key');
    await tapKey(tester, 'inline-generate-key');
    await tester.enterText(find.descendant(of: findDialog(), matching: find.byType(TextField)).first, 'keyed-ed25519');
    await tapKey(tester, 'credential-save');
    await tapKey(tester, 'generated-done');
    expect(find.text('SSH key · keyed-ed25519'), findsOneWidget, reason: 'new key is selected');
    await tapKey(tester, 'save-host');

    final host = hostNamed(backend, 'keyed');
    final key = credentialsOf(backend).firstWhere((c) => c.id == host.credentialId);
    expect(key.name, 'keyed-ed25519');
    expect(key.kind, CredentialKind.sshPrivateKey);
    expect(key.keyAlgorithm, KeyAlgorithm.ed25519);
    expect(host.inlineCredentialId, isNull, reason: 'keys are shared credentials');
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('"Use existing credential…" links a shared credential without creating one', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);
    final before = credentialsOf(backend).length;
    final shared = credentialsOf(backend).firstWhere((c) => c.name == 'home-lab password');

    await fillNewHost(tester, 'nas');
    await tapKey(tester, 'use-existing-credential');
    await tapKey(tester, 'pick-credential-home-lab password');
    expect(find.byKey(const ValueKey('linked-credential')), findsOneWidget);
    await tapKey(tester, 'save-host');

    final host = hostNamed(backend, 'nas');
    expect(host.credentialId, shared.id);
    expect(host.inlineCredentialId, isNull);
    expect(credentialsOf(backend), hasLength(before));
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('agent mode links the OS agent credential', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);

    await fillNewHost(tester, 'agented');
    await chooseMode(tester, 'Agent');
    await tapKey(tester, 'save-host');
    final host = hostNamed(backend, 'agented');
    final cred = credentialsOf(backend).firstWhere((c) => c.id == host.credentialId);
    expect(cred.kind, CredentialKind.osSshAgent);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
}
