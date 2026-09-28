import 'dart:async';

import 'package:flutter_test/flutter_test.dart';

/// Fail hung widget tests fast (default would be 10 minutes).
Future<void> testExecutable(FutureOr<void> Function() testMain) async {
  final binding = TestWidgetsFlutterBinding.ensureInitialized();
  if (binding is AutomatedTestWidgetsFlutterBinding) {
    binding.defaultTestTimeout = const Timeout(Duration(seconds: 60));
  }
  await testMain();
}
