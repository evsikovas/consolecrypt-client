import 'package:consolecrypt/wslc/shared/shell_words.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('command lines split like a POSIX shell without expansion', () {
    expect(shellWords(''), isEmpty);
    expect(shellWords('  nginx  -g   daemon '), ['nginx', '-g', 'daemon']);
    expect(shellWords("sh -c 'echo \"hi there\"'"), ['sh', '-c', 'echo "hi there"']);
    expect(shellWords(r'echo "a \"b\" \c"'), ['echo', r'a "b" \c']);
    expect(shellWords("x '' y"), ['x', '', 'y']);
    expect(() => shellWords("echo 'open"), throwsFormatException);
  });
}
