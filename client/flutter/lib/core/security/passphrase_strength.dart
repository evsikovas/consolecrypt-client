import 'dart:math';

/// Which suggestion to show under the strength meter (localized in
/// `lib/core/l10n/labels.dart`).
enum PassphraseHint { empty, sequence, common, tooShort, good, addMore }

/// Heuristic strength estimate for the Vault passphrase (onboarding and
/// recovery). It only guides the user: the real protection is Argon2id
/// (ADR-0002). Deliberately dependency-free; no network lookups.
final class PassphraseStrength {
  const PassphraseStrength._(this.score, this.bits, this.hint);

  /// 0 very weak … 4 very strong.
  final int score;

  /// Rough entropy estimate in bits.
  final double bits;

  /// One actionable suggestion (or a confirmation).
  final PassphraseHint hint;

  /// Number of strength levels (score 0…4).
  static const levels = 5;

  /// Minimum length for a Vault passphrase.
  static const minLength = 12;

  double get fraction => (score + 1) / levels;

  /// Whether onboarding/recovery accept this passphrase.
  bool get acceptable => score >= 3;

  static const _common = [
    'password',
    'passw0rd',
    'qwerty',
    'letmein',
    'welcome',
    'admin',
    'iloveyou',
    'monkey',
    'dragon',
    'sunshine',
    'princess',
    'football',
    'baseball',
    'master',
    'consolecrypt',
    'termius',
    'correct horse battery staple', // l10n-ignore: common-password list
    '123456',
    'abc123',
    'secret',
    'changeme',
    'trustno1',
  ];

  static const _sequences = ['abcdefghijklmnopqrstuvwxyz', '0123456789', 'qwertyuiop', 'asdfghjkl', 'zxcvbnm'];

  static PassphraseStrength estimate(String passphrase) {
    if (passphrase.isEmpty) {
      return const PassphraseStrength._(0, 0, PassphraseHint.empty);
    }
    final lower = passphrase.toLowerCase();
    var pool = 0;
    if (RegExp('[a-z]').hasMatch(passphrase)) pool += 26;
    if (RegExp('[A-Z]').hasMatch(passphrase)) pool += 26;
    if (RegExp('[0-9]').hasMatch(passphrase)) pool += 10;
    if (RegExp(r'[^A-Za-z0-9\s]').hasMatch(passphrase)) pool += 33;
    if (RegExp(r'\s').hasMatch(passphrase)) pool += 1;
    if (passphrase.runes.any((r) => r > 127)) pool += 64;

    // Characters beyond a repeat of the previous two add little entropy.
    var effectiveLength = 0.0;
    final chars = passphrase.split('');
    for (var i = 0; i < chars.length; i++) {
      final repeat = i >= 2 && chars[i] == chars[i - 1] && chars[i] == chars[i - 2];
      effectiveLength += repeat ? 0.25 : 1;
    }
    var bits = effectiveLength * (log(max(pool, 2)) / ln2);
    PassphraseHint? hint;

    for (final seq in _sequences) {
      for (var i = 0; i + 4 <= seq.length; i++) {
        if (lower.contains(seq.substring(i, i + 4))) {
          bits -= 12;
          hint = PassphraseHint.sequence;
          break;
        }
      }
    }
    for (final word in _common) {
      if (lower.contains(word)) {
        bits = min(bits, 24);
        hint = PassphraseHint.common;
        break;
      }
    }
    if (passphrase.length < minLength) {
      bits = min(bits, 35);
      hint = PassphraseHint.tooShort;
    }
    bits = max(bits, 0);

    final score = switch (bits) {
      < 28 => 0,
      < 40 => 1,
      < 60 => 2,
      < 80 => 3,
      _ => 4,
    };
    hint ??= score >= 3 ? PassphraseHint.good : PassphraseHint.addMore;
    return PassphraseStrength._(score, bits, hint);
  }
}
