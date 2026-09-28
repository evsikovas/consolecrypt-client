import 'dart:async';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

const _channel = MethodChannel('consolecrypt/screen_capture');
final _toggle = find.byKey(const ValueKey('screen-capture-switch'));

Future<ProviderContainer> _settings(WidgetTester tester) async {
  final backend = testBackend();
  addTearDown(backend.dispose);
  await backend.debugSignInDemoAndUnlock();
  final settings = backend.services.settings;
  await settings.updateLocal(settings.currentLocal.copyWith(uiFontScale: 1.4));
  await pumpApp(tester, backend, locale: AppLocale.ru, size: const Size(360, 800));
  final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
  container.read(routerProvider).go(AppRoutes.settings);
  await settle(tester);
  return container;
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  final messenger = TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
  tearDown(() => messenger.setMockMethodCallHandler(_channel, null));

  testWidgets('Android setting reads native state, changes immediately and survives reopening', (tester) async {
    var allowed = false;
    final writes = <bool>[];
    messenger.setMockMethodCallHandler(_channel, (call) async {
      if (call.method == 'setAllowed') {
        allowed = (call.arguments as Map<Object?, Object?>)['allowed']! as bool;
        writes.add(allowed);
      }
      return allowed;
    });
    final container = await _settings(tester);
    expect(tester.widget<SwitchListTile>(_toggle).value, isFalse);
    expect(writes, isEmpty);
    await tapKey(tester, 'screen-capture-switch');
    expect(writes, [true]);
    expect(tester.widget<SwitchListTile>(_toggle).value, isTrue);
    container.read(routerProvider).go(AppRoutes.hosts);
    await settle(tester);
    container.read(routerProvider).go(AppRoutes.settings);
    await settle(tester);
    expect(tester.widget<SwitchListTile>(_toggle).value, isTrue);
    expect(writes, [true], reason: 'navigation never resets the native preference');
    await tapKey(tester, 'screen-capture-switch');
    expect(writes, [true, false]);
    expect(tester.widget<SwitchListTile>(_toggle).value, isFalse);
    expect(tester.takeException(), isNull, reason: '360px Russian 140% layout');
  }, variant: TargetPlatformVariant.only(TargetPlatform.android));

  testWidgets('read/write errors show retry and never claim the setting was applied', (tester) async {
    var failRead = true;
    messenger.setMockMethodCallHandler(_channel, (call) async {
      if (call.method == 'setAllowed' || failRead) throw PlatformException(code: 'storage');
      return false;
    });
    await _settings(tester);
    expect(tester.widget<SwitchListTile>(_toggle).onChanged, isNull);
    expect(find.byKey(const ValueKey('screen-capture-error')), findsOneWidget);
    failRead = false;
    await tapKey(tester, 'screen-capture-retry');
    expect(tester.widget<SwitchListTile>(_toggle).onChanged, isNotNull);
    await tapKey(tester, 'screen-capture-switch');
    expect(tester.widget<SwitchListTile>(_toggle).value, isFalse);
    expect(tester.widget<SwitchListTile>(_toggle).onChanged, isNull);
    expect(find.byKey(const ValueKey('screen-capture-error')), findsOneWidget);
    await tapKey(tester, 'screen-capture-retry');
    expect(find.byKey(const ValueKey('screen-capture-error')), findsNothing);
  }, variant: TargetPlatformVariant.only(TargetPlatform.android));

  testWidgets('pending native changes disable the switch and can finish after leaving settings', (tester) async {
    final pending = Completer<bool>();
    var writes = 0;
    messenger.setMockMethodCallHandler(_channel, (call) async {
      if (call.method == 'getAllowed') return false;
      writes++;
      return pending.future;
    });
    final container = await _settings(tester);
    await tapKey(tester, 'screen-capture-switch');
    expect(tester.widget<SwitchListTile>(_toggle).onChanged, isNull);
    expect(tester.widget<SwitchListTile>(_toggle).value, isFalse);
    expect(writes, 1);
    container.read(routerProvider).go(AppRoutes.hosts);
    await settle(tester);
    pending.complete(true);
    await settle(tester);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.android));

  testWidgets('resuming Android refreshes the actual window state without writing preferences', (tester) async {
    var allowed = false;
    messenger.setMockMethodCallHandler(_channel, (call) async {
      expect(call.method, 'getAllowed');
      return allowed;
    });
    await _settings(tester);
    allowed = true;
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.inactive);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
    await settle(tester);
    expect(tester.widget<SwitchListTile>(_toggle).value, isTrue);
  }, variant: TargetPlatformVariant.only(TargetPlatform.android));

  testWidgets('macOS explains capture limits without a pretend blocking switch', (tester) async {
    messenger.setMockMethodCallHandler(_channel, (call) async {
      fail('macOS must not attempt unsupported screenshot protection');
    });
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);
    await tapKey(tester, 'nav-settings');
    expect(find.byKey(const ValueKey('screen-capture-macos-info')), findsOneWidget);
    expect(_toggle, findsNothing);
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));
}
