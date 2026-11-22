import 'package:consolecrypt/core/l10n/l10n.dart';

/// Splits a command line into arguments: whitespace separates words, '…' is literal,
/// "…" allows \" and \\ escapes. Throws [FormatException] on an unclosed quote.
List<String> shellWords(String input, {required AppLocalizations l10n}) {
  final words = <String>[];
  final word = StringBuffer();
  String? quote;
  var started = false;
  for (var i = 0; i < input.length; i++) {
    final c = input[i];
    if (quote != null) {
      if (c == quote) {
        quote = null;
      } else if (quote == '"' && c == r'\' && i + 1 < input.length && (input[i + 1] == '"' || input[i + 1] == r'\')) {
        word.write(input[++i]);
      } else {
        word.write(c);
      }
      continue;
    }
    if (c == '"' || c == "'") {
      quote = c;
      started = true;
    } else if (c.trim().isEmpty) {
      if (started) {
        words.add(word.toString());
        word.clear();
        started = false;
      }
    } else {
      word.write(c);
      started = true;
    }
  }
  if (quote != null) throw FormatException(l10n.wslcUnclosedQuote);
  if (started) words.add(word.toString());
  return words;
}
