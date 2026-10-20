import 'dart:io';

import 'package:integration_test/integration_test_driver.dart';

Future<void> main() => integrationDriver(
  timeout: const Duration(minutes: 3),
  writeResponseOnFailure: true,
  responseDataCallback: (data) => writeResponseData(
    data,
    testOutputFilename: 'kubernetes_performance',
    destinationDirectory: Platform.environment['CC_PERFORMANCE_OUTPUT_DIR'] ?? 'build/performance',
  ),
);
