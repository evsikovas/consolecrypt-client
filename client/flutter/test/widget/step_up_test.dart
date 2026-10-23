import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/vault/user_presence.dart';
import 'package:consolecrypt/vault/vault_passphrase_confirmation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/fake_presence.dart';
import '../helpers/test_app.dart';

final _scope = Provider<int>((ref) => 1);

Future<void> _host(WidgetTester tester, FakePresence presence) async {
  tester.view.physicalSize = const Size(1200, 900);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    ProviderScope(
      overrides: [userPresenceProvider.overrideWithValue(presence)],
      child: const MaterialApp(
        locale: Locale('en'),
        localizationsDelegates: [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        supportedLocales: AppLocalizations.supportedLocales,
        home: Scaffold(body: SizedBox.expand()),
      ),
    ),
  );
}

/// Opens the confirmation; the caller pumps and awaits the result.
Future<bool> _confirm(WidgetTester tester, {StepUpAction action = StepUpAction.confirm, String object = ''}) =>
    showVaultPassphraseConfirmation<int>(
      tester.element(find.byType(Scaffold)),
      scope: _scope,
      title: 'Allow changes',
      message: 'Writes to the cluster are allowed for 10 minutes.',
      confirmLabel: 'Allow changes',
      action: action,
      object: object,
    );

Finder _primary() => find.byKey(const ValueKey('vault-passphrase-confirm-action'));

String _primaryLabel(WidgetTester tester) => (tester.widget<GlassButton>(_primary())).label;

void main() {
  testWidgets('Touch ID is the primary confirmation; the token is checked for the action', (tester) async {
    final presence = FakePresence();
    await _host(tester, presence);
    final result = _confirm(tester, object: 'kube/s1');
    await settle(tester);
    expect(_primaryLabel(tester), 'Confirm with Touch ID');
    expect(find.byKey(const ValueKey('vault-passphrase-confirm')), findsNothing, reason: 'no field until asked');
    expect(find.byKey(const ValueKey('step-up-use-passphrase')), findsOneWidget);
    await tester.tap(_primary());
    await settle(tester);
    expect(await result, isTrue);
    final (action, object, reason) = presence.verified.single;
    expect((action, object), (StepUpAction.confirm, 'kube/s1'));
    expect(reason, 'confirm “Allow changes”');
    expect(presence.confirmed.single, isA<PresenceProof>());
  });

  testWidgets('cancelled or locked-out biometry falls back to the passphrase', (tester) async {
    final presence = FakePresence()..failure = AppErrorCode.osAuthFailed;
    await _host(tester, presence);
    final result = _confirm(tester);
    await settle(tester);
    await tester.tap(_primary());
    await settle(tester);
    expect(find.byKey(const ValueKey('step-up-presence-failed')), findsOneWidget);
    expect(find.text('Not confirmed. Enter the vault passphrase instead.'), findsOneWidget);
    expect(_primaryLabel(tester), 'Allow changes');
    expect(presence.confirmed, isEmpty, reason: 'nothing to check after a failed prompt');
    // A wrong passphrase stays under the field.
    await tester.enterText(find.byKey(const ValueKey('vault-passphrase-confirm')), 'nope');
    await tester.pump();
    await tester.tap(_primary());
    await settle(tester);
    expect(find.text('Wrong vault passphrase.'), findsOneWidget);
    await tester.enterText(find.byKey(const ValueKey('vault-passphrase-confirm')), 'vault-pass');
    await tester.pump();
    await tester.tap(_primary());
    await settle(tester);
    expect(await result, isTrue);
    expect(presence.confirmed.last, isA<PassphraseProof>());
  });

  testWidgets('the passphrase can be chosen instead, and biometry chosen back', (tester) async {
    final presence = FakePresence(kind: DeviceAuthKind.windowsHello);
    await _host(tester, presence);
    final result = _confirm(tester);
    await settle(tester);
    expect(_primaryLabel(tester), 'Confirm with Windows Hello');
    await tester.tap(find.byKey(const ValueKey('step-up-use-passphrase')));
    await settle(tester);
    expect(find.byKey(const ValueKey('vault-passphrase-confirm')), findsOneWidget);
    expect(_primaryLabel(tester), 'Allow changes');
    expect(tester.widget<GlassButton>(_primary()).onPressed, isNull, reason: 'no passphrase typed yet');
    await tester.tap(find.byKey(const ValueKey('step-up-use-presence')));
    await settle(tester);
    expect(_primaryLabel(tester), 'Confirm with Windows Hello');
    await tester.tap(find.byKey(const ValueKey('vault-passphrase-cancel')));
    await settle(tester);
    expect(await result, isFalse);
    expect(presence.verified, isEmpty, reason: 'cancel never prompts');
  });

  testWidgets('passphrase-only actions and devices without the OS check show only the field', (tester) async {
    final presence = FakePresence(kind: DeviceAuthKind.fingerprint);
    await _host(tester, presence);
    var result = _confirm(tester, action: StepUpAction.changePassphrase);
    await settle(tester);
    expect(find.byKey(const ValueKey('vault-passphrase-confirm')), findsOneWidget);
    expect(find.byKey(const ValueKey('step-up-use-presence')), findsNothing);
    expect(_primaryLabel(tester), 'Allow changes');
    await tester.tap(find.byKey(const ValueKey('vault-passphrase-cancel')));
    await settle(tester);
    expect(await result, isFalse);

    presence.kind = null;
    result = _confirm(tester);
    await settle(tester);
    expect(find.byKey(const ValueKey('vault-passphrase-confirm')), findsOneWidget);
    expect(find.byKey(const ValueKey('step-up-use-presence')), findsNothing);
    await tester.tap(find.byKey(const ValueKey('vault-passphrase-cancel')));
    await settle(tester);
    expect(await result, isFalse);
    expect(presence.verified, isEmpty);
  });

  test('passphrase-only actions mirror the core list', () {
    expect(passphraseOnlyStepUps, {
      StepUpAction.changePassphrase,
      StepUpAction.recoveryKit,
      StepUpAction.disableDeviceUnlock,
      StepUpAction.backupExport,
      StepUpAction.backupRestore,
      StepUpAction.approveDevice,
      StepUpAction.accountSecurity,
      StepUpAction.enterpriseAccount,
    });
    for (final action in [
      StepUpAction.confirm,
      StepUpAction.dbProductionWrite,
      StepUpAction.dockerReveal,
      StepUpAction.dockerCompose,
    ]) {
      expect(action.passphraseOnly, isFalse);
    }
    expect(const PresenceProof('secret-token').toString(), isNot(contains('secret-token')));
  });
}
