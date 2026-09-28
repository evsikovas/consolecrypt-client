import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/passphrase_strength.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  const marker = 'hunter2-SECRET-marker';

  group('no secrets in toString()', () {
    test('SecretText is redacted and can be wiped', () {
      final s = SecretText(marker);
      expect(s.toString(), isNot(contains(marker)));
      expect('$s', contains('redacted'));
      expect(s.expose(), marker);
      s.wipe();
      expect(s.isWiped, isTrue);
      expect(s.expose, throwsStateError);
    });

    test('Secret model is redacted', () {
      final secret = Secret(
        id: ObjectId.generate(),
        kind: SecretKind.password,
        value: SecretText(marker),
        createdAt: DateTime.now(),
        updatedAt: DateTime.now(),
      );
      expect(secret.toString(), isNot(contains(marker)));
    });

    test('RecoveryKit is redacted and forgettable', () {
      final kit = RecoveryKit(
        vaultId: VaultId.generate(),
        words: List.filled(24, 'marker'),
        qrPayload: 'consolecrypt-recovery:v1:$marker',
        createdAt: DateTime.now(),
      );
      expect(kit.toString(), isNot(contains('marker')));
      kit.forget();
      expect(kit.exposeWords(), isEmpty);
      expect(kit.exposeQrPayload(), isEmpty);
    });

    test('backup unlock requests are redacted', () {
      expect(BackupUnlockWithPassphrase(SecretText(marker)).toString(), isNot(contains(marker)));
      expect(
        BackupUnlockWithRecoveryKey(recoveryInput: SecretText(marker), newPassphrase: SecretText(marker)).toString(),
        isNot(contains(marker)),
      );
    });

    test('constant-time comparison', () {
      expect(SecretText('abc').constantTimeEquals(SecretText('abc')), isTrue);
      expect(SecretText('abc').constantTimeEquals(SecretText('abd')), isFalse);
      expect(SecretText('abc').constantTimeEquals(SecretText('abcd')), isFalse);
    });
  });

  group('passphrase strength', () {
    test('weak inputs are rejected', () {
      for (final p in [
        '',
        'short',
        'password1234',
        'qwertyuiop12',
        'correct horse battery staple',
        'aaaaaaaaaaaaaaaa',
      ]) {
        expect(PassphraseStrength.estimate(p).acceptable, isFalse, reason: p);
      }
    });

    test('long random-ish passphrases are accepted', () {
      for (final p in ['violet-anchor-muffin-glacier-42', 'Tr4in!Moss_Harbor_Lamp', 'seven brisk otters juggle neon']) {
        expect(PassphraseStrength.estimate(p).acceptable, isTrue, reason: p);
      }
    });

    test('score is monotonic-ish with length', () {
      expect(
        PassphraseStrength.estimate('Tr4in!Moss_Harbor_Lamp_Quiet').score,
        greaterThanOrEqualTo(PassphraseStrength.estimate('Tr4in!Moss').score),
      );
    });
  });
}
