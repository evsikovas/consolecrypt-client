import 'dart:async';
import 'dart:math';

import 'package:consolecrypt/core/security/secure_clipboard.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter_test/flutter_test.dart';

import '../helpers/fake_native_clipboard.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  String generated() => List.generate(32, (_) => Random.secure().nextInt(256)).join('-');
  const platforms = TargetPlatformVariant({
    TargetPlatform.windows,
    TargetPlatform.macOS,
    TargetPlatform.linux,
    TargetPlatform.android,
    TargetPlatform.iOS,
  });

  testWidgets('revoked FIFO entry never writes after an earlier blocked OS copy', (tester) async {
    final native = FakeNativeClipboard()..install();
    native.gate = Completer<void>();
    final owner = SecureClipboard();
    final first = owner.copySecret(generated());
    await tester.pump();
    expect(native.calls, ['copySecret']);
    var current = true;
    final queued = owner.copySecret(generated(), isCurrent: () => current);
    current = false;
    native.gate!.complete();
    await first;
    await queued;
    expect(native.calls, ['copySecret']);
    expect(native.text, isNotNull);
    owner.dispose();
    await tester.pump();
  }, variant: platforms);

  for (final anotherOwner in [false, true]) {
    testWidgets('revocation during OS copy clears own clip only, external change=$anotherOwner', (tester) async {
      final native = FakeNativeClipboard()..install();
      native.gate = Completer<void>();
      final owner = SecureClipboard();
      var current = true;
      final operation = owner.copySecret(generated(), isCurrent: () => current);
      await tester.pump();
      current = false;
      // Another app may write right after our native write completes.
      if (anotherOwner) native.afterCopy = () => native.userCopies('New public clip');
      native.gate!.complete();
      await operation;
      expect(anotherOwner ? native.text == 'New public clip' : native.text == null, isTrue);
      expect(native.calls, ['copySecret', 'clearIfOwned']);
      expect(native.dartReads, 0);
      owner.dispose();
      await tester.pump();
    }, variant: platforms);
  }
}
