import 'dart:math';

import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secure_clipboard.dart';
import 'package:consolecrypt/core/util/value_stream.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  String? clipboard;
  TestWidgetsFlutterBinding.ensureInitialized();
  setUp(() {
    clipboard = null;
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(
      SystemChannels.platform,
      (call) async {
        if (call.method == 'Clipboard.setData') clipboard = (call.arguments as Map)['text'] as String;
        if (call.method == 'Clipboard.getData') return {'text': clipboard};
        return null;
      },
    );
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(
      const MethodChannel('consolecrypt/clipboard'),
      (call) async {
        expect(call.method, 'copySecret');
        final arguments = call.arguments as Map;
        clipboard = arguments['text'] as String;
        expect(arguments['clearAfterMilliseconds'], inInclusiveRange(1000, 86400000));
        return null;
      },
    );
  });
  tearDown(() {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(
      SystemChannels.platform,
      null,
    );
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(
      const MethodChannel('consolecrypt/clipboard'),
      null,
    );
  });
  String value() => List.generate(32, (_) => Random.secure().nextInt(256)).join('-');

  testWidgets('a missing mobile handler cannot downgrade a secret clipboard write', (tester) async {
    clipboard = 'Existing public clip';
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(
      const MethodChannel('consolecrypt/clipboard'),
      (_) async => throw MissingPluginException(),
    );
    final owner = SecureClipboard();
    await expectLater(owner.copySecret(value()), throwsA(isA<MissingPluginException>()));
    expect(clipboard, 'Existing public clip');
    owner.dispose();
    await tester.pump();
  }, variant: const TargetPlatformVariant({TargetPlatform.iOS, TargetPlatform.android}));

  testWidgets('dispose clears a copied secret but preserves a newer clipboard value', (tester) async {
    final first = SecureClipboard();
    await first.copySecret(value());
    first.dispose();
    await tester.pump();
    expect(clipboard?.isEmpty, isTrue);

    final second = SecureClipboard();
    await second.copySecret(value());
    clipboard = 'A newer non-secret value';
    second.dispose();
    await tester.pump();
    expect(clipboard, 'A newer non-secret value');
  });

  testWidgets('changing local settings retains the clipboard owner and the pending deadline', (tester) async {
    final settings = ValueStreamController(const LocalSettings(clipboardClearSeconds: 2));
    final container = ProviderContainer(overrides: [localSettingsProvider.overrideWith((ref) => settings.stream)]);
    final subscription = container.listen(secureClipboardProvider, (_, _) {});
    await tester.pump();
    final owner = container.read(secureClipboardProvider);
    await owner.copySecret(value());
    settings.value = const LocalSettings(clipboardClearSeconds: 3, terminalFontSize: 17);
    await tester.pump();
    expect(identical(container.read(secureClipboardProvider), owner), isTrue);
    await tester.pump(const Duration(seconds: 3));
    expect(clipboard?.isEmpty, isTrue);
    subscription.close();
    container.dispose();
    await settings.close();
  });

  testWidgets('the timer clears only the last copied secret', (tester) async {
    final owner = SecureClipboard(clearAfter: const Duration(seconds: 2));
    await owner.copySecret(value());
    await tester.pump(const Duration(seconds: 1));
    await owner.copySecret(value());
    await tester.pump(const Duration(seconds: 1));
    expect(clipboard?.isNotEmpty, isTrue);
    await tester.pump(const Duration(seconds: 1));
    expect(clipboard?.isEmpty, isTrue);
    owner.dispose();
  });
}
