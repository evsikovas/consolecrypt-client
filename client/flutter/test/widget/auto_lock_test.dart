import 'dart:async';

import 'package:consolecrypt/app/auto_lock.dart';
import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/vault_service.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

class _Vault implements VaultService {
  _Vault(this.delegate);
  final VaultService delegate;
  int locks = 0;
  Future<void> Function()? onLock;

  @override
  VaultStatus get currentStatus => delegate.currentStatus;
  @override
  Stream<VaultStatus> watchStatus() => delegate.watchStatus();
  @override
  Future<void> lock() async {
    locks++;
    if (onLock case final callback?) {
      await callback();
    } else {
      await delegate.lock();
    }
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => throw UnsupportedError('$invocation');
}

class _Harness {
  _Harness(this.tester) : backend = MockBackend(config: const MockConfig.test(seedDemoData: false)) {
    vault = _Vault(backend.vault);
    addTearDown(backend.dispose);
  }
  final WidgetTester tester;
  final MockBackend backend;
  late final _Vault vault;
  Duration wallOffset = Duration.zero;
  late DateTime started;

  Future<void> mount({bool unlocked = true, Widget? child}) async {
    if (unlocked) await backend.debugCreateUnlockedLocalProfile();
    await setMinutes(1);
    started = tester.binding.clock.now();
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          appServicesProvider.overrideWithValue(backend.services),
          vaultServiceProvider.overrideWithValue(vault),
        ],
        child: MaterialApp(
          builder: (context, navigator) => AutoLockScope(
            now: () => tester.binding.clock.now().add(wallOffset),
            elapsed: () => tester.binding.clock.now().difference(started),
            child: navigator!,
          ),
          home: Scaffold(body: child ?? const SizedBox.expand(key: ValueKey('surface'))),
        ),
      ),
    );
    await tester.pump();
  }

  Future<void> setMinutes(int minutes) =>
      backend.settings.updateLocal(backend.settings.currentLocal.copyWith(autoLockMinutes: minutes));

  Future<void> dispose() async {
    await tester.pumpWidget(const SizedBox());
    await tester.pump();
  }
}

void main() {
  testWidgets('starts only on unlock and locks once after inactivity', (tester) async {
    final h = _Harness(tester);
    await h.mount(unlocked: false);
    await tester.pump(const Duration(minutes: 2));
    expect(h.vault.locks, 0);
    await h.backend.debugCreateUnlockedLocalProfile();
    await tester.pump();
    await tester.pump(const Duration(seconds: 59));
    expect(h.vault.locks, 0);
    await tester.pump(const Duration(seconds: 1));
    expect(h.vault.locks, 1);
    expect(h.vault.currentStatus.isUnlocked, isFalse);
    await tester.pump(const Duration(minutes: 2));
    expect(h.vault.locks, 1);
    await h.dispose();
  });
  for (final activity in ['pointer', 'hover', 'scroll', 'key', 'native text']) {
    testWidgets('$activity renews timeout without consuming child input', (tester) async {
      final h = _Harness(tester);
      var childKeys = 0;
      BuildContext? inputContext;
      await h.mount(
        child: Focus(
          autofocus: true,
          onKeyEvent: (_, _) {
            childKeys++;
            return KeyEventResult.handled;
          },
          child: Builder(
            builder: (context) {
              inputContext = context;
              return const ColoredBox(key: ValueKey('surface'), color: Colors.white, child: SizedBox.expand());
            },
          ),
        ),
      );
      await tester.pump(const Duration(seconds: 50));
      switch (activity) {
        case 'pointer':
          await tester.tap(find.byKey(const ValueKey('surface')));
        case 'hover':
          await tester.sendEventToBinding(const PointerHoverEvent(position: Offset(50, 50)));
        case 'scroll':
          await tester.sendEventToBinding(
            const PointerScrollEvent(position: Offset(50, 50), scrollDelta: Offset(0, 5)),
          );
        case 'key':
          await tester.sendKeyDownEvent(LogicalKeyboardKey.keyA);
          await tester.sendKeyUpEvent(LogicalKeyboardKey.keyA);
          expect(childKeys, 2);
        case 'native text':
          AutoLockScope.recordActivity(inputContext!);
      }
      await tester.pump(const Duration(seconds: 50));
      expect(h.vault.locks, 0);
      await tester.pump(const Duration(seconds: 10));
      expect(h.vault.locks, 1);
      await h.dispose();
    });
  }

  testWidgets('software keyboard edits to a focused text field renew timeout', (tester) async {
    final h = _Harness(tester);
    await h.mount(child: const TextField(key: ValueKey('input')));
    await tester.tap(find.byKey(const ValueKey('input')));
    await tester.pump();
    await tester.pump(const Duration(seconds: 50));
    tester.testTextInput.enterText('typed without a hardware key or pointer');
    await tester.pump();
    await tester.pump(const Duration(seconds: 50));
    expect(h.vault.locks, 0);
    await tester.pump(const Duration(seconds: 10));
    expect(h.vault.locks, 1);
    await h.dispose();
  });

  testWidgets('hidden window still expires, foregrounding cannot renew elapsed deadline', (tester) async {
    final h = _Harness(tester);
    await h.mount();
    await tester.pump(const Duration(seconds: 20));
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.hidden);
    await tester.pump(const Duration(seconds: 40));
    expect(h.vault.locks, 1);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
    await tester.pump();
    expect(h.vault.locks, 1);
    await h.dispose();
  });

  testWidgets('resume rechecks wall time when suspend prevented timer delivery', (tester) async {
    final h = _Harness(tester);
    await h.mount();
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.paused);
    h.wallOffset = const Duration(minutes: 3);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
    await tester.pump();
    expect(h.vault.locks, 1);
    await h.dispose();
  });

  testWidgets('backward wall clock change does not extend inactivity', (tester) async {
    final h = _Harness(tester);
    await h.mount();
    h.wallOffset = const Duration(hours: -1);
    await tester.pump(const Duration(minutes: 1));
    expect(h.vault.locks, 1);
    await h.dispose();
  });

  testWidgets('zero disables, re-enabling starts fresh, shorter timeout keeps idle history', (tester) async {
    final h = _Harness(tester);
    await h.mount();
    await tester.pump(const Duration(seconds: 50));
    await h.setMinutes(0);
    await tester.pump();
    await tester.pump(const Duration(minutes: 3));
    expect(h.vault.locks, 0);
    await h.setMinutes(2);
    await tester.pump();
    await tester.pump(const Duration(seconds: 70));
    expect(h.vault.locks, 0);
    await h.setMinutes(1);
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 1));
    expect(h.vault.locks, 1);
    await h.dispose();
  });

  testWidgets('manual lock cancels old deadline and unlock starts a new period', (tester) async {
    final h = _Harness(tester);
    await h.mount();
    await tester.pump(const Duration(seconds: 50));
    await h.backend.vault.lock();
    await tester.pump();
    await tester.pump(const Duration(minutes: 2));
    expect(h.vault.locks, 0);
    await h.backend.vault.unlockWithPassphrase(SecretText('violet-anchor-muffin-glacier-42'));
    await tester.pump();
    await tester.pump(const Duration(seconds: 50));
    expect(h.vault.locks, 0);
    await tester.pump(const Duration(seconds: 10));
    expect(h.vault.locks, 1);
    await h.dispose();
  });

  testWidgets('profile switch cancels the previous deadline', (tester) async {
    final h = _Harness(tester);
    await h.mount();
    await tester.pump(const Duration(seconds: 50));
    await h.backend.debugCreateUnlockedLocalProfile(name: 'Other');
    await tester.pump();
    await tester.pump(const Duration(seconds: 50));
    expect(h.vault.locks, 0);
    await tester.pump(const Duration(seconds: 10));
    expect(h.vault.locks, 1);
    await h.dispose();
  });

  testWidgets('in-flight lock is single flight and its stale failure cannot rearm a new profile', (tester) async {
    final h = _Harness(tester);
    final pending = Completer<void>();
    h.vault.onLock = () => pending.future;
    await h.mount();
    await tester.pump(const Duration(minutes: 1));
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
    await tester.pump();
    expect(h.vault.locks, 1);
    await h.backend.debugCreateUnlockedLocalProfile(name: 'Other');
    await tester.pump();
    pending.completeError(StateError('old lock failed'));
    await tester.pump();
    await tester.pump(const Duration(seconds: 59));
    expect(h.vault.locks, 1);
    h.vault.onLock = null;
    await tester.pump(const Duration(seconds: 1));
    expect(h.vault.locks, 2);
    await h.dispose();
  });

  testWidgets('dispose cancels timer and late lock completion does not rearm it', (tester) async {
    final h = _Harness(tester);
    final pending = Completer<void>();
    h.vault.onLock = () => pending.future;
    await h.mount();
    await tester.pump(const Duration(minutes: 1));
    expect(h.vault.locks, 1);
    await h.dispose();
    pending.completeError(StateError('disposed'));
    await tester.pump(const Duration(minutes: 2));
    expect(h.vault.locks, 1);
    expect(tester.takeException(), isNull);
  });
}
