import 'package:consolecrypt/app/app.dart';
import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/security/vault_key_alarm.dart';
import 'package:consolecrypt/core/services/devices_service.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/vault/approval_wait_screen.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

/// Devices of the mock account plus the protocol 1.11 parts under test.
final class _Devices implements DevicesService {
  _Devices(this._inner);
  final DevicesService _inner;
  VaultKeyStatus status = const VaultKeyStatus(pinned: true, unconfirmed: true, fingerprint: '0011223344556677');
  ApprovalProgress? progress;
  VerificationCode? confirmedShown;
  int confirmations = 0;

  @override
  Future<VaultKeyStatus> vaultKeyStatus() async => status;
  @override
  Future<bool> confirmVaultKey(SecretText passphrase) async {
    final ok = passphrase.expose() == 'right passphrase';
    passphrase.wipe();
    if (ok) {
      confirmations++;
      status = const VaultKeyStatus(pinned: true, fingerprint: '0011223344556677');
    }
    return ok;
  }

  @override
  Future<ApprovalProgress?> approvalProgress() async => progress;
  @override
  Future<void> confirmApprover(ApprovalProgress p, VerificationCode shown) async => confirmedShown = shown;

  @override
  dynamic noSuchMethod(Invocation invocation) =>
      Function.apply(_delegate(invocation.memberName), invocation.positionalArguments, invocation.namedArguments);

  Function _delegate(Symbol name) => switch (name) {
    #watchDevices => _inner.watchDevices,
    #refresh => _inner.refresh,
    #verificationCodeFor => _inner.verificationCodeFor,
    #currentDeviceVerificationCode => _inner.currentDeviceVerificationCode,
    #approve => _inner.approve,
    #reject => _inner.reject,
    #revoke => _inner.revoke,
    #rename => _inner.rename,
    #watchNewDevices => _inner.watchNewDevices,
    #dismissNewDevice => _inner.dismissNewDevice,
    _ => throw UnimplementedError('$name'),
  };
}

Future<_Devices> _pump(WidgetTester tester, MockBackend backend, {Widget? home}) async {
  final devices = _Devices(backend.services.devices);
  setTestLocale(backend, AppLocale.en);
  tester.view.physicalSize = const Size(1600, 1000);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        appServicesProvider.overrideWithValue(backend.services),
        devicesServiceProvider.overrideWithValue(devices),
      ],
      retry: (_, _) => null,
      child: home ?? const ConsoleCryptApp(),
    ),
  );
  await settle(tester);
  return devices;
}

void main() {
  testWidgets('a vault key mismatch raises the blocking screen with diagnostics and lock', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);
    vaultKeyAlarm.value = AppException(
      AppErrorCode.vaultKeyMismatch,
      'mismatch',
      args: {'vault_id': 'vault-123', 'pinned': 'aa' * 32, 'received': 'bb' * 32, 'reason': 'commitment'},
    );
    await settle(tester);
    expect(find.byKey(const ValueKey('vault-key-mismatch')), findsOneWidget);
    expect(find.textContaining('The server sent a different vault key'), findsWidgets);
    // No way to accept the other key: only Lock, Copy diagnostics, More.
    expect(find.textContaining('accept', findRichText: true), findsNothing);
    await tapKey(tester, 'vault-key-mismatch-more');
    final diagnostics = tester.widget<SelectableText>(find.byKey(const ValueKey('vault-key-mismatch-diagnostics')));
    expect(diagnostics.data, contains('vault-123'));
    expect(diagnostics.data, contains('aa' * 32));
    expect(diagnostics.data, contains('bb' * 32));
    await tapKey(tester, 'vault-key-mismatch-lock');
    expect(find.byKey(const ValueKey('vault-key-mismatch')), findsNothing);
    expect(backend.debugVaultStatus.phase, isNot(VaultPhase.unlocked));
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('an unconfirmed vault key is shown on the devices screen and confirmed with the passphrase', (
    tester,
  ) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    final devices = await _pump(tester, backend);
    await tapKey(tester, 'nav-devices');
    expect(find.byKey(const ValueKey('vault-key-unconfirmed')), findsOneWidget);
    await tapKey(tester, 'vault-key-confirm');
    await enterKey(tester, 'vault-key-confirm-passphrase', 'wrong passphrase');
    await tapKey(tester, 'vault-key-confirm-submit');
    expect(devices.confirmations, 0);
    expect(find.byKey(const ValueKey('vault-key-confirm-dialog')), findsOneWidget);
    await enterKey(tester, 'vault-key-confirm-passphrase', 'right passphrase');
    await tapKey(tester, 'vault-key-confirm-submit');
    expect(devices.confirmations, 1);
    expect(find.byKey(const ValueKey('vault-key-confirm-dialog')), findsNothing);
    expect(find.byKey(const ValueKey('vault-key-unconfirmed')), findsNothing);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('the new device confirms the pairing code only with the mandatory checkbox', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    final pairing = VerificationCode.tryParse('12345 67890 11111 22222 33333 44444')!;
    final devices = _Devices(backend.services.devices)
      ..progress = ApprovalProgress(
        requestId: const DeviceRequestId('request-1'),
        ownCode: VerificationCode.tryParse('99999 88888 77777 66666 55555 44444')!,
        vrkCommitment: true,
        approverDeviceId: const DeviceId('approver-1'),
        approverName: 'Office PC',
        pairingCode: pairing,
      );
    setTestLocale(backend, AppLocale.en);
    tester.view.physicalSize = const Size(1200, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          appServicesProvider.overrideWithValue(backend.services),
          devicesServiceProvider.overrideWithValue(devices),
        ],
        retry: (_, _) => null,
        child: const MaterialApp(
          localizationsDelegates: AppLocalizations.localizationsDelegates,
          supportedLocales: AppLocalizations.supportedLocales,
          home: ApprovalWaitScreen(),
        ),
      ),
    );
    await settle(tester);
    expect(find.byKey(const ValueKey('approval-pairing-title')), findsOneWidget);
    expect(find.textContaining('Office PC'), findsWidgets);
    expect(isEnabled(tester, 'approval-pairing-confirm'), isFalse, reason: 'the checkbox is mandatory');
    await tapKey(tester, 'approval-pairing-matches');
    await tapKey(tester, 'approval-pairing-confirm');
    expect(devices.confirmedShown, pairing, reason: 'the code that was on the screen is confirmed');
    expect(find.byKey(const ValueKey('approval-pairing-confirmed')), findsOneWidget);
    expect(tester.takeException(), isNull);
  });
}
