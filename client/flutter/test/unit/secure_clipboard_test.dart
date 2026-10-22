import 'dart:convert';
import 'dart:math';

import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secure_clipboard.dart';
import 'package:consolecrypt/core/util/value_stream.dart';
import 'package:crypto/crypto.dart';
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
    final settings = ValueStreamController(const LocalSettings(clipboardClearSeconds: 30));
    final container = ProviderContainer(overrides: [localSettingsProvider.overrideWith((ref) => settings.stream)]);
    final subscription = container.listen(secureClipboardProvider, (_, _) {});
    await tester.pump();
    final owner = container.read(secureClipboardProvider);
    await owner.copySecret(value());
    settings.value = const LocalSettings(clipboardClearSeconds: 60, terminalFontSize: 17);
    await tester.pump();
    expect(identical(container.read(secureClipboardProvider), owner), isTrue);
    await tester.pump(const Duration(seconds: 29));
    expect(clipboard?.isEmpty, isFalse);
    await tester.pump(const Duration(seconds: 1));
    expect(clipboard?.isEmpty, isTrue);
    expect(owner.clearAfter, const Duration(minutes: 1));
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

  test('stored auto-clear durations map to the nearest choice not shorter', () {
    expect(clipboardClearChoices, [30, 60, 120, 300, 900, 3600, 28800, 86400]);
    expect(clipboardClearSeconds(10), 30);
    expect(clipboardClearSeconds(15), 30);
    expect(clipboardClearSeconds(30), 30);
    expect(clipboardClearSeconds(45), 60);
    expect(clipboardClearSeconds(120), 120);
    expect(clipboardClearSeconds(121), 300);
    expect(clipboardClearSeconds(7200), 28800);
    expect(clipboardClearSeconds(999999), 86400);
    expect(clipboardClearSeconds(-1), 30);
    // Settings saved by older versions (15/30/60/120) load as choices.
    for (final (old, mapped) in [(10, 30), (15, 30), (30, 30), (60, 60), (120, 120), (90000, 86400)]) {
      expect(localSettingsFromJson({'clipboard_clear_seconds': old}).clipboardClearSeconds, mapped);
    }
    expect(localSettingsFromJson({}).clipboardClearSeconds, 30);
  });

  testWidgets('a pending clear survives a restart and never clears foreign content', (tester) async {
    final store = _MemoryClearStore();
    var now = DateTime.utc(2026, 10, 22, 12);
    final secret = value();
    final first = SecureClipboard(clearAfter: const Duration(hours: 1), store: store, now: () => now);
    await first.copySecret(secret);
    expect(store.record, isNotNull);
    expect(store.record, isNot(contains(secret)));
    // The app is killed: no dispose, no timer.

    // Restart before the deadline: nothing is cleared yet.
    now = now.add(const Duration(minutes: 30));
    final second = SecureClipboard(store: store, now: () => now);
    await second.clearExpired();
    expect(clipboard, secret);
    expect(store.record, isNotNull);
    second.dispose();
    await tester.pump();

    // Restart after the deadline with our value still there: cleared.
    expect(clipboard?.isEmpty, isTrue, reason: 'dispose clears what it re-armed');
    clipboard = secret;
    store.record = '{"deadline_ms":${now.millisecondsSinceEpoch - 1},"sha256":"${_sha(secret)}"}';
    final third = SecureClipboard(store: store, now: () => now);
    await third.clearExpired();
    expect(clipboard?.isEmpty, isTrue);
    expect(store.record, isNull);

    // Foreign content after the deadline stays; the record is dropped.
    clipboard = 'copied by the user later';
    store.record = '{"deadline_ms":${now.millisecondsSinceEpoch - 1},"sha256":"${_sha(secret)}"}';
    await third.clearExpired();
    expect(clipboard, 'copied by the user later');
    expect(store.record, isNull);

    // Malformed records are dropped without touching the clipboard.
    store.record = 'not json';
    await third.clearExpired();
    expect(clipboard, 'copied by the user later');
    expect(store.record, isNull);
    third.dispose();
    first.dispose();
    await tester.pump();
  });

  testWidgets('copying plain text or clearing on time drops the pending record', (tester) async {
    final store = _MemoryClearStore();
    final owner = SecureClipboard(clearAfter: const Duration(seconds: 2), store: store);
    await owner.copySecret(value());
    expect(store.record, isNotNull);
    await tester.pump(const Duration(seconds: 2));
    expect(clipboard?.isEmpty, isTrue);
    expect(store.record, isNull);
    await owner.copySecret(value());
    await owner.copyPlain('ssh-ed25519 AAAA public');
    expect(store.record, isNull);
    owner.dispose();
  });
}

String _sha(String text) => sha256.convert(utf8.encode(text)).toString();

final class _MemoryClearStore implements ClipboardClearStore {
  String? record;
  @override
  Future<String?> read() async => record;
  @override
  Future<void> write(String? record) async => this.record = record;
}
