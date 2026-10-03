import 'dart:async';
import 'dart:math';

import 'package:consolecrypt/core/security/secure_clipboard.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  String generated() => List.generate(32, (_) => Random.secure().nextInt(256)).join('-');
  const mobile = MethodChannel('consolecrypt/clipboard');
  void install(Future<Object?> Function(MethodCall) handler) {
    final messenger = TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
    messenger.setMockMethodCallHandler(SystemChannels.platform, handler);
    messenger.setMockMethodCallHandler(mobile, handler);
  }

  tearDown(() {
    final messenger = TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
    messenger.setMockMethodCallHandler(SystemChannels.platform, null);
    messenger.setMockMethodCallHandler(mobile, null);
  });
  const platforms = TargetPlatformVariant({
    TargetPlatform.windows,
    TargetPlatform.macOS,
    TargetPlatform.android,
    TargetPlatform.iOS,
  });

  testWidgets('revoked FIFO entry never writes after an earlier blocked OS copy', (tester) async {
    final gate = Completer<void>();
    var writes = 0;
    String? clipboard;
    install((call) async {
      if (call.method == 'Clipboard.setData' || call.method == 'copySecret') {
        writes++;
        clipboard = (call.arguments as Map)['text'] as String;
        if (writes == 1) await gate.future;
      }
      if (call.method == 'Clipboard.getData') return {'text': clipboard};
      return null;
    });
    final owner = SecureClipboard();
    final first = owner.copySecret(generated());
    await tester.pump();
    expect(writes, 1);
    var current = true;
    final queued = owner.copySecret(generated(), isCurrent: () => current);
    current = false;
    gate.complete();
    await first;
    await queued;
    expect(writes, 1);
    expect(clipboard?.isNotEmpty, isTrue);
    owner.dispose();
    await tester.pump();
  }, variant: platforms);

  for (final anotherOwner in [false, true]) {
    testWidgets('revocation during OS copy clears own value only, external change=$anotherOwner', (tester) async {
      final gate = Completer<void>();
      var writes = 0;
      String? clipboard;
      install((call) async {
        if (call.method == 'Clipboard.setData' || call.method == 'copySecret') {
          writes++;
          clipboard = (call.arguments as Map)['text'] as String;
          if (writes == 1) await gate.future;
        }
        if (call.method == 'Clipboard.getData') return {'text': clipboard};
        return null;
      });
      final owner = SecureClipboard();
      var current = true;
      final operation = owner.copySecret(generated(), isCurrent: () => current);
      await tester.pump();
      expect(writes, 1);
      current = false;
      if (anotherOwner) clipboard = 'New public clip';
      gate.complete();
      await operation;
      expect(anotherOwner ? clipboard == 'New public clip' : clipboard?.isEmpty, isTrue);
      expect(writes, anotherOwner ? 1 : 2);
      owner.dispose();
      await tester.pump();
    }, variant: platforms);
  }
}
