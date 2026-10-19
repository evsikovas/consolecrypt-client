import 'package:flutter_test/flutter_test.dart';

import 'local_ai_smoke.dart';

void main() {
  test('smoke timeout is explicit bounded and defaults to three minutes', () {
    expect(parseLocalAiSmokeTimeout([]), const Duration(seconds: 180));
    expect(() => parseLocalAiSmokeTimeout(['--request-timeout-seconds']), throwsFormatException);
    for (final seconds in [30, 180, 900]) {
      expect(parseLocalAiSmokeTimeout(['--request-timeout-seconds=$seconds']), Duration(seconds: seconds));
    }
    for (final value in ['', '29', '901', '-1', '1.5', 'abc', '900000000000000000000000']) {
      expect(() => parseLocalAiSmokeTimeout(['--request-timeout-seconds=$value']), throwsFormatException);
    }
    expect(
      () => parseLocalAiSmokeTimeout(['--request-timeout-seconds=30', '--request-timeout-seconds=40']),
      throwsFormatException,
    );
  });
}
