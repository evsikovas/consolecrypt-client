import 'dart:async';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/services/vault_service.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import '../helpers/test_app.dart';

/// The real vault of the mock backend, with security key support (FEAT-16)
/// simulated: the key asks for a PIN in the app and records what reaches
/// the core.
class _Vault implements VaultService {
  _Vault(this.delegate);
  final VaultService delegate;
  bool enabled = false;
  final pins = <String?>[];
  final toggles = <bool>[];

  /// Holds the request like a key waiting for its touch.
  Completer<void>? touch;

  VaultStatus _with(VaultStatus s) => VaultStatus(
    phase: s.phase,
    vaultId: s.vaultId,
    vaultName: s.vaultName,
    recoveryKitConfirmed: s.recoveryKitConfirmed,
    deviceTrusted: s.deviceTrusted,
    deviceUnlock: s.deviceUnlock,
    securityKey: SecurityKeyInfo(
      supported: true,
      enabled: enabled,
      pinPrompt: SecurityKeyPinPrompt.app,
      hasVault: true,
    ),
  );

  @override
  VaultStatus get currentStatus => _with(delegate.currentStatus);
  @override
  Stream<VaultStatus> watchStatus() => delegate.watchStatus().map(_with);
  @override
  Future<void> lock() => delegate.lock();
  @override
  Future<void> refreshDeviceUnlockAvailability() => delegate.refreshDeviceUnlockAvailability();

  @override
  Future<void> setSecurityKeyEnabled(bool enabled, {SecretText? pin}) async {
    pins.add(pin?.expose());
    await touch?.future;
    toggles.add(enabled);
    this.enabled = enabled;
  }

  @override
  Future<void> unlockWithSecurityKey({SecretText? pin}) async {
    pins.add(pin?.expose());
    await touch?.future;
    throw const AppException(
      AppErrorCode.validation,
      'wrong key',
      reason: AppErrorReason.invalidField,
      args: {'field': 'security_key', 'rule': 'wrong_key'},
    );
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => throw UnsupportedError('$invocation');
}

void main() {
  testWidgets('the lock screen unlocks with the security key and its PIN', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    final vault = _Vault(backend.services.vault)..enabled = true;
    await pumpApp(tester, backend, overrides: [vaultServiceProvider.overrideWithValue(vault)]);
    await backend.services.vault.lock();
    await settle(tester);

    await tapKey(tester, 'unlock-security-key');
    expect(find.byKey(const ValueKey('security-key-pin')), findsOneWidget);
    await enterKey(tester, 'security-key-pin', '123456');
    vault.touch = Completer<void>();
    await tapKey(tester, 'security-key-pin-ok');
    expect(vault.pins, ['123456']);
    // After the PIN the key only blinks: the app says to touch it.
    expect(find.byKey(const ValueKey('security-key-touch-dialog')), findsOneWidget);
    expect(find.byKey(const ValueKey('security-key-touch-twice')), findsNothing);
    vault.touch!.complete();
    await settle(tester);
    expect(find.byKey(const ValueKey('security-key-touch-dialog')), findsNothing);
    expect(find.text('This is not the security key set up for this vault.'), findsOneWidget);
  });

  testWidgets('cancelling the PIN never reaches the key', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    final vault = _Vault(backend.services.vault)..enabled = true;
    await pumpApp(tester, backend, overrides: [vaultServiceProvider.overrideWithValue(vault)]);
    await backend.services.vault.lock();
    await settle(tester);

    await tapKey(tester, 'unlock-security-key');
    await tapKey(tester, 'security-key-pin-cancel');
    expect(vault.pins, isEmpty);
  });

  testWidgets('without the feature switched on the lock screen offers no security key', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend, overrides: [vaultServiceProvider.overrideWithValue(_Vault(backend.services.vault))]);
    await backend.services.vault.lock();
    await settle(tester);
    expect(find.byKey(const ValueKey('unlock-security-key')), findsNothing);
  });

  testWidgets('settings switch the security key on with its PIN and off again', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    final vault = _Vault(backend.services.vault);
    await pumpApp(tester, backend, overrides: [vaultServiceProvider.overrideWithValue(vault)]);
    final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    container.read(routerProvider).go(AppRoutes.settings);
    await settle(tester);

    await tapKey(tester, 'security-key-switch');
    await enterKey(tester, 'security-key-pin', '654321');
    vault.touch = Completer<void>();
    await tapKey(tester, 'security-key-pin-ok');
    expect(find.byKey(const ValueKey('security-key-touch-twice')), findsOneWidget);
    vault.touch!.complete();
    await settle(tester);
    expect(find.byKey(const ValueKey('security-key-touch-dialog')), findsNothing);
    expect(vault.toggles, [true]);
    expect(vault.pins, ['654321']);
    expect(find.text('Security key unlock is on'), findsOneWidget);
  });
}
