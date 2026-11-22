import 'package:consolecrypt/l10n/app_localizations_en.dart';
import 'package:consolecrypt/l10n/app_localizations_zh.dart';
import 'package:consolecrypt/wslc/features/logs/log_format.dart';
import 'package:consolecrypt/wslc/shared/widgets.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('line count follows the current language', () {
    String lines(int n, String locale) => plural(n, 'one', 'few', 'many', locale: locale);
    expect(lines(21, 'en'), '21 many');
    expect(lines(21, 'es'), '21 many');
    expect(lines(21, 'ru'), '21 one');
    expect(lines(22, 'ru'), '22 few');
    expect(lines(11, 'ru'), '11 many');
    expect(lines(1, 'zh'), '1 many');
  });
  test('native reader statuses are localized without changing their polling token', () {
    final en = AppLocalizationsEn();
    final zh = AppLocalizationsZh();
    expect(readingLogStatus, 'Чтение');
    expect(logStatusLabel(readingLogStatus, en), en.wslcLogStreamReading);
    expect(logStatusLabel(readingLogStatus, zh), zh.wslcLogStreamReading);
    expect(logStatusLabel('Ошибка CLI (код 17)', en), en.wslcErrorExitCode('17'));
    expect(logStatusLabel('unrecognized runtime detail', en), 'unrecognized runtime detail');
  });
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
