import 'dart:math' as math;

import 'package:consolecrypt/core/security/secret_text.dart';

class PasswordGeneratorOptions {
  const PasswordGeneratorOptions({
    this.length = 20,
    this.uppercase = true,
    this.lowercase = true,
    this.digits = true,
    this.symbols = true,
    this.excludeSimilar = false,
  });
  final int length;
  final bool uppercase, lowercase, digits, symbols, excludeSimilar;

  List<String> get _groups => [
    if (uppercase) 'ABCDEFGHIJKLMNOPQRSTUVWXYZ',
    if (lowercase) 'abcdefghijklmnopqrstuvwxyz',
    if (digits) '0123456789',
    if (symbols) r'!@#$%^&*()-_=+[]{};:,.?/',
  ].map((group) => excludeSimilar ? group.split('').where((c) => !'0O1Il'.contains(c)).join() : group).toList();

  void validate() {
    if (length < 8 || length > 128) throw ArgumentError('length');
    if (_groups.isEmpty) throw ArgumentError('groups');
  }

  /// Uniform sampling conditioned on containing every enabled character class.
  /// Rejection avoids the distribution bias of "one per class + shuffle".
  SecretText generate() {
    validate();
    final groups = _groups;
    final alphabet = groups.join();
    final random = math.Random.secure();
    while (true) {
      final chars = List.generate(length, (_) => alphabet[random.nextInt(alphabet.length)]);
      if (groups.every((group) => chars.any(group.contains))) return SecretText(chars.join());
    }
  }

  /// Entropy of this generator's uniform output space, including the class
  /// coverage constraint. This does not assess arbitrary user-entered passwords.
  double get entropyBits {
    validate();
    final groups = _groups;
    final size = groups.join().length;
    var probability = 0.0;
    for (var mask = 0; mask < 1 << groups.length; mask++) {
      var removed = 0;
      var count = 0;
      for (var i = 0; i < groups.length; i++) {
        if ((mask & (1 << i)) != 0) {
          removed += groups[i].length;
          count++;
        }
      }
      probability += (count.isEven ? 1 : -1) * math.pow((size - removed) / size, length);
    }
    return length * math.log(size) / math.ln2 + math.log(probability.clamp(1e-12, 1)) / math.ln2;
  }
}
