import 'dart:async';

import 'package:consolecrypt/rdp/rdp_window.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  testWidgets('native entry waits for fullscreen acknowledgement', (tester) async {
    var acknowledged = false, completed = false;
    final messenger = TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
    messenger.setMockMethodCallHandler(
      RdpWindow.channel,
      (call) async => switch (call.method) {
        'beginRdpFullscreen' => true,
        'isFullscreen' => acknowledged,
        'isRdpFullscreenRestored' => true,
        _ => null,
      },
    );
    addTearDown(() => messenger.setMockMethodCallHandler(RdpWindow.channel, null));
    final window = RdpWindow();
    final lease = window.acquire()!;
    final pending = lease.enter().then((_) => completed = true);
    await tester.pump(const Duration(seconds: 1));
    expect(completed, isFalse);
    expect(window.acquire(), isNull);
    acknowledged = true;
    await tester.pump(const Duration(milliseconds: 100));
    await pending;
    expect(completed, isTrue);
    await lease.restore();
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('lease remains exclusive through native restore acknowledgement and minimize', (tester) async {
    var restored = false, completed = false;
    final minimizing = Completer<void>();
    final calls = <String>[];
    final messenger = TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
    messenger.setMockMethodCallHandler(RdpWindow.channel, (call) async {
      calls.add(call.method);
      switch (call.method) {
        case 'beginRdpFullscreen':
          return true;
        case 'isFullscreen':
          return true;
        case 'isRdpFullscreenRestored':
          return restored;
        case 'minimize':
          await minimizing.future;
          return null;
        case 'isRdpMinimized':
          return true;
      }
      return null;
    });
    addTearDown(() => messenger.setMockMethodCallHandler(RdpWindow.channel, null));
    final window = RdpWindow();
    final lease = window.acquire()!;
    await lease.enter();
    final pending = lease.restore(minimize: true).then((_) => completed = true);
    await tester.pump(const Duration(seconds: 1));
    expect(calls, isNot(contains('minimize')));
    expect(window.acquire(), isNull);
    restored = true;
    await tester.pump(const Duration(milliseconds: 100));
    expect(calls, contains('minimize'));
    expect(window.acquire(), isNull);
    expect(completed, isFalse);
    minimizing.complete();
    await pending;
    final next = window.acquire();
    expect(next, isNotNull);
    final count = calls.length;
    await lease.restore();
    expect(calls.length, count); // Old owner cannot restore a newer lease.
    await next!.restore();
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('failed restore does not release native window to another owner', (tester) async {
    final messenger = TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
    messenger.setMockMethodCallHandler(RdpWindow.channel, (call) async {
      if (call.method == 'endRdpFullscreen') throw PlatformException(code: 'restore_failed');
      return true;
    });
    addTearDown(() => messenger.setMockMethodCallHandler(RdpWindow.channel, null));
    final window = RdpWindow();
    final lease = window.acquire()!;
    await lease.enter();
    await expectLater(lease.restore(), throwsA(isA<PlatformException>()));
    expect(window.acquire(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('accepted minimize waits for native state before another lease', (tester) async {
    var minimized = false, completed = false;
    final calls = <String>[];
    final messenger = TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
    messenger.setMockMethodCallHandler(RdpWindow.channel, (call) async {
      calls.add(call.method);
      return switch (call.method) {
        'beginRdpFullscreen' || 'isFullscreen' || 'isRdpFullscreenRestored' => true,
        'isRdpMinimized' => minimized,
        _ => null, // Native minimize accepts the request before animation ends.
      };
    });
    addTearDown(() => messenger.setMockMethodCallHandler(RdpWindow.channel, null));
    final window = RdpWindow();
    final lease = window.acquire()!;
    await lease.enter();
    final pending = lease.restore(minimize: true).then((_) => completed = true);
    await tester.pump(const Duration(seconds: 1));
    expect(calls, contains('minimize'));
    expect(completed, isFalse);
    expect(window.acquire(), isNull);
    minimized = true;
    await tester.pump(const Duration(milliseconds: 100));
    await pending;
    expect(completed, isTrue);
    final next = window.acquire();
    expect(next, isNotNull);
    final count = calls.length;
    await lease.restore();
    expect(calls.length, count);
    await next!.restore();
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));

  testWidgets('minimize timeout preserves the exclusive native lease', (tester) async {
    final messenger = TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
    messenger.setMockMethodCallHandler(
      RdpWindow.channel,
      (call) async => switch (call.method) {
        'beginRdpFullscreen' || 'isFullscreen' || 'isRdpFullscreenRestored' => true,
        'isRdpMinimized' => false,
        _ => null,
      },
    );
    addTearDown(() => messenger.setMockMethodCallHandler(RdpWindow.channel, null));
    final window = RdpWindow();
    final lease = window.acquire()!;
    await lease.enter();
    final assertion = expectLater(
      lease.restore(minimize: true),
      throwsA(isA<PlatformException>().having((e) => e.code, 'code', 'fullscreen_minimize_timeout')),
    );
    for (var tick = 0; tick < 160; tick++) {
      await tester.pump(const Duration(milliseconds: 100));
    }
    await assertion;
    expect(window.acquire(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.linux));

  testWidgets('native minimize error preserves the exclusive lease', (tester) async {
    final messenger = TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
    messenger.setMockMethodCallHandler(RdpWindow.channel, (call) async {
      if (call.method == 'minimize') throw PlatformException(code: 'minimize_failed');
      return true;
    });
    addTearDown(() => messenger.setMockMethodCallHandler(RdpWindow.channel, null));
    final window = RdpWindow();
    final lease = window.acquire()!;
    await lease.enter();
    await expectLater(lease.restore(minimize: true), throwsA(isA<PlatformException>()));
    expect(window.acquire(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
}
