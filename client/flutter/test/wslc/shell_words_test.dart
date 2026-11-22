import 'package:consolecrypt/l10n/app_localizations_ru.dart';
import 'package:consolecrypt/wslc/shared/shell_words.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  List<String> parse(String command) => shellWords(command, l10n: AppLocalizationsRu());
  test('command lines split like a POSIX shell without expansion', () {
    expect(parse(''), isEmpty);
    expect(parse('  nginx  -g   daemon '), ['nginx', '-g', 'daemon']);
    expect(parse("sh -c 'echo \"hi there\"'"), ['sh', '-c', 'echo "hi there"']);
    expect(parse(r'echo "a \"b\" \c"'), ['echo', r'a "b" \c']);
    expect(parse("x '' y"), ['x', '', 'y']);
    expect(() => parse("echo 'open"), throwsFormatException);
  });
}
