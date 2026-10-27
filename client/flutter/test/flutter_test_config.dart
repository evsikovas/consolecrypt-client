import 'dart:async';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

/// Fail hung widget tests fast (default would be 10 minutes). A slow CI host
/// (the Windows runner, a desktop in use) sets CC_WIDGET_TEST_TIMEOUT_SECONDS.
Future<void> testExecutable(FutureOr<void> Function() testMain) async {
  final binding = TestWidgetsFlutterBinding.ensureInitialized();
  if (binding is AutomatedTestWidgetsFlutterBinding) {
    final seconds = int.tryParse(Platform.environment['CC_WIDGET_TEST_TIMEOUT_SECONDS'] ?? '') ?? 60;
    binding.defaultTestTimeout = Timeout(Duration(seconds: seconds));
  }
  await testMain();
}
