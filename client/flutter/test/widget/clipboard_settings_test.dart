import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/secure_clipboard.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

final _toggle = find.byKey(const ValueKey('clipboard-background-clear'));

Future<void> _settings(WidgetTester tester, {Size size = const Size(360, 800)}) async {
  final backend = testBackend();
  addTearDown(backend.dispose);
  await backend.debugSignInDemoAndUnlock();
  final settings = backend.services.settings;
  await settings.updateLocal(settings.currentLocal.copyWith(uiFontScale: 1.4));
  await pumpApp(tester, backend, locale: AppLocale.ru, size: size);
  final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
  container.read(routerProvider).go(AppRoutes.settings);
  await settle(tester);
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  final messenger = TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
  tearDown(() => messenger.setMockMethodCallHandler(SecureClipboard.channel, null));

  testWidgets('Android: background clearing is on by default and saved natively', (tester) async {
    bool? stored;
    final writes = <bool>[];
    messenger.setMockMethodCallHandler(SecureClipboard.channel, (call) async {
      if (call.method == 'setBackgroundClear') {
        stored = (call.arguments as Map<Object?, Object?>)['enabled']! as bool;
        writes.add(stored!);
      }
      return stored ?? true;
    });
    await _settings(tester);
    await tester.scrollUntilVisible(_toggle, 200, scrollable: find.byType(Scrollable).last);
    expect(tester.widget<SwitchListTile>(_toggle).value, isTrue);
    expect(find.text('Очищать буфер в фоне, даже если нельзя проверить, что там наш секрет'), findsOneWidget);
    await tapKey(tester, 'clipboard-background-clear');
    expect(writes, [false]);
    expect(tester.widget<SwitchListTile>(_toggle).value, isFalse);
    expect(tester.takeException(), isNull, reason: '360px Russian 140% layout');
  }, variant: TargetPlatformVariant.only(TargetPlatform.android));

  testWidgets('the background clearing switch is Android only', (tester) async {
    await _settings(tester, size: const Size(1280, 900));
    expect(_toggle, findsNothing);
  }, variant: const TargetPlatformVariant({TargetPlatform.iOS, TargetPlatform.macOS, TargetPlatform.windows}));
}
