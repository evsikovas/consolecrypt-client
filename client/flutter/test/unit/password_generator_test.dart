import 'dart:math' as math;

import 'package:consolecrypt/keepass/password_generator.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('every enabled class is present and disabled/similar characters are absent', () {
    const groups = [
      'ABCDEFGHIJKLMNOPQRSTUVWXYZ',
      'abcdefghijklmnopqrstuvwxyz',
      '0123456789',
      r'!@#$%^&*()-_=+[]{};:,.?/',
    ];
    for (var mask = 1; mask < 16; mask++) {
      for (final length in [8, 20, 128]) {
        for (final similar in [false, true]) {
          final options = PasswordGeneratorOptions(
            length: length,
            uppercase: mask & 1 != 0,
            lowercase: mask & 2 != 0,
            digits: mask & 4 != 0,
            symbols: mask & 8 != 0,
            excludeSimilar: similar,
          );
          final generated = options.generate();
          try {
            final text = generated.expose();
            expect(text.length, length);
            var alphabet = '';
            for (var i = 0; i < 4; i++) {
              if ((mask & (1 << i)) != 0) {
                alphabet += groups[i];
                expect(text.split('').any(groups[i].contains), isTrue);
              }
            }
            expect(text.split('').every(alphabet.contains), isTrue);
            if (similar) expect(text.split('').any('0O1Il'.contains), isFalse);
          } finally {
            generated.wipe();
          }
        }
      }
    }
  });

  test('invalid options never silently fall back to another policy', () {
    for (final length in [-1, 0, 7, 129, 100000]) {
      expect(() => PasswordGeneratorOptions(length: length).generate(), throwsArgumentError);
    }
    expect(
      () =>
          const PasswordGeneratorOptions(uppercase: false, lowercase: false, digits: false, symbols: false).generate(),
      throwsArgumentError,
    );
  });

  test('entropy describes the uniform generator with mandatory class coverage', () {
    expect(
      const PasswordGeneratorOptions(uppercase: false, lowercase: false, symbols: false).entropyBits,
      closeTo(20 * math.log(10) / math.ln2, 1e-9),
    );
    final bits = const PasswordGeneratorOptions().entropyBits;
    expect(bits, greaterThan(100));
    expect(const PasswordGeneratorOptions(excludeSimilar: true).entropyBits, lessThan(bits));
    expect(const PasswordGeneratorOptions(length: 128).entropyBits.isFinite, isTrue);
  });
}
