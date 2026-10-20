import 'package:consolecrypt/kubernetes/kube_log_filter.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('log search supports substring, severity, and regex without modifying source', () async {
    final filter = KubeLogFilter();
    addTearDown(filter.cancel);
    const lines = ['INFO ready', 'ERROR failed request 12', 'WARN retry request 13'];
    expect(await filter.apply(lines, 'REQUEST', false, 'error'), ['ERROR failed request 12']);
    expect(await filter.apply(lines, r'request 1[23]$', true, 'all'), lines.skip(1));
    await expectLater(filter.apply(lines, '[', true, 'all'), throwsA(isA<FormatException>()));
  });
  test('catastrophic user regex is terminated and a later filter still works', () async {
    final filter = KubeLogFilter();
    addTearDown(filter.cancel);
    await expectLater(filter.apply(['${'a' * 100000}!'], r'(a+)+$', true, 'all'), throwsA(isA<FormatException>()));
    expect(await filter.apply(['INFO ready'], '', false, 'all'), ['INFO ready']);
  }, timeout: const Timeout(Duration(seconds: 5)));
  test('cancelling filtering drops pending source and does not publish stale lines', () async {
    final filter = KubeLogFilter();
    final pending = filter.apply(['${'a' * 100000}!'], r'(a+)+$', true, 'all');
    filter.cancel();
    expect(await pending, isEmpty);
  });
}
