// Run on an isolated Android emulator/device; no vault or network is used.
// Run a second time with CC_CAPTURE_EXPECT_ALLOWED=true to verify that the
// native window restores the stored flag before Dart starts. Finally use
// CC_CAPTURE_LEAVE_ALLOWED=false to restore protection on the test device.
import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();
  const channel = MethodChannel('consolecrypt/screen_capture');
  testWidgets('Android native window capture flag toggles and persists', (tester) async {
    const expected = bool.fromEnvironment('CC_CAPTURE_EXPECT_ALLOWED');
    const leaveAllowed = bool.fromEnvironment('CC_CAPTURE_LEAVE_ALLOWED', defaultValue: true);
    expect(await channel.invokeMethod<bool>('getAllowed'), expected);
    for (final allowed in [false, true, false, leaveAllowed]) {
      expect(await channel.invokeMethod<bool>('setAllowed', {'allowed': allowed}), allowed);
      expect(await channel.invokeMethod<bool>('getAllowed'), allowed);
    }
    await expectLater(
      channel.invokeMethod<bool>('setAllowed', {'allowed': 'invalid'}),
      throwsA(isA<PlatformException>().having((e) => e.code, 'code', 'invalid_argument')),
    );
    expect(await channel.invokeMethod<bool>('getAllowed'), leaveAllowed);
  }, skip: !Platform.isAndroid);
}
