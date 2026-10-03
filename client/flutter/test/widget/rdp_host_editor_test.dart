import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/credentials/credential_dialogs.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

void main() {
  testWidgets('RDP is selected first and offers Windows fields without SSH controls', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);
    await tapKey(tester, 'add-host');
    final selector = find.byKey(const ValueKey('host-protocol'));
    await tester.tap(find.descendant(of: selector, matching: find.text('RDP')));
    await settle(tester);
    expect(find.byKey(const ValueKey('host-rdp-domain')), findsOneWidget);
    expect(find.byKey(const ValueKey('jump-mode')), findsNothing);
    expect(find.byKey(const ValueKey('host-key-policy')), findsNothing);
    expect(find.byKey(const ValueKey('auth-mode')), findsNothing);
    expect(tester.widget<TextFormField>(find.byKey(const ValueKey('host-port'))).controller!.text, '3389');
    await enterKey(tester, 'host-name', 'Windows test');
    await enterKey(tester, 'host-address', 'windows.example.test');
    await enterKey(tester, 'host-username', 'operator');
    await enterKey(tester, 'host-rdp-domain', 'LAB');
    await tapKey(tester, 'save-host');
    final host = backend.inventory.currentHosts.firstWhere((h) => h.name == 'Windows test');
    expect(host.protocol, HostProtocol.rdp);
    expect(host.rdpDomain, 'LAB');
    expect(host.port, 3389);
    expect(host.promptsForPassword, isTrue);
    await enterKey(tester, 'hosts-search', 'Windows test');
    await tapKey(tester, 'host-menu-Windows test');
    expect(find.text('Open SFTP'), findsNothing);
    await tapKey(tester, 'host-edit-menu-Windows test');
    final control = tester.widget<GlassSegmented<HostProtocol>>(selector);
    expect(control.selected, HostProtocol.rdp);
    expect(control.onChanged, isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
  testWidgets('RDP credential creation opens only the password editor', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);
    await tapKey(tester, 'add-host');
    await tester.tap(find.descendant(of: find.byKey(const ValueKey('host-protocol')), matching: find.text('RDP')));
    await settle(tester);
    await tapKey(tester, 'use-existing-credential');
    await tapKey(tester, 'picker-new-credential');
    expect(find.byType(PasswordCredentialDialog), findsOneWidget);
    expect(find.byKey(const ValueKey('new-credential-chooser')), findsNothing);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
}
