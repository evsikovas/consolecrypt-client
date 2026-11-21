import 'package:consolecrypt/wslc/features/logs/log_format.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('levels come from JSON fields or the first level word', () {
    expect(detectLevel('2026-10-29 ERROR db timeout'), LogLevel.error);
    expect(detectLevel('[warn] disk 91%'), LogLevel.warn);
    expect(detectLevel('INFO started'), LogLevel.info);
    expect(detectLevel('level=debug msg=x'), LogLevel.debug);
    expect(detectLevel('{"level":"error","msg":"info about it"}'), LogLevel.error);
    expect(detectLevel('nothing to see'), isNull);
    expect(detectLevel('terrorist'), isNull, reason: 'whole words only');
  });

  test('JSON objects are pretty-printed, other lines are left alone', () {
    expect(prettyJson('{"a":1,"b":{"c":true}}'), '{\n  "a": 1,\n  "b": {\n    "c": true\n  }\n}');
    expect(prettyJson('[1,2]'), isNull);
    expect(prettyJson('{broken'), isNull);
    expect(prettyJson('plain text'), isNull);
  });

  test('runtime timestamps split into local time and message', () {
    final split = splitTimestamp('2026-10-29T07:56:29.900943874Z worker heartbeat 17');
    expect(split, isNotNull);
    expect(split!.$2, 'worker heartbeat 17');
    expect(split.$1, matches(RegExp(r'^\d\d:\d\d:29\.900$')));
    expect(splitTimestamp('no stamp here'), isNull);
  });
}
