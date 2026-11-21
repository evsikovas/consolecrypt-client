import 'dart:convert';
import 'dart:math';
import 'dart:typed_data';

import 'package:consolecrypt/core/bridge/mapping.dart' show guard;
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/keepass/external_vault.dart';
import 'package:consolecrypt/src/rust/api/password_vault.dart' as native;
import 'package:crypto/crypto.dart' show sha256;
import 'package:cryptography/cryptography.dart';

export 'external_vault_repository.dart' show externalVaultConnectionsProvider;

typedef ExternalVaultKeyOperation = Future<Uint8List> Function(String action, String id, Uint8List? material);

class ExternalVaultCredentialStore {
  ExternalVaultCredentialStore({ExternalVaultKeyOperation? keyOperation}) : _native = keyOperation ?? _deviceKey;

  final _aead = AesGcm.with256bits();
  final ExternalVaultKeyOperation _native;

  String _id(ExternalVaultConnection config, List<int> nonce) =>
      sha256.convert(utf8.encode('external-vault:${config.binding}:${base64UrlEncode(nonce)}')).toString();

  static Future<Uint8List> _deviceKey(String action, String id, Uint8List? material) => guard(
    () => native.passwordVaultKey(action: action, id: id, method: 'hello', secret: material ?? Uint8List(0), pin: null),
  );

  SecretBox _box(String cipherText) => SecretBox.fromConcatenation(
    base64Decode(cipherText),
    nonceLength: _aead.nonceLength,
    macLength: _aead.macAlgorithm.macLength,
  );

  Future<bool> available(ExternalVaultConnection config) async {
    final value = jsonDecode(utf8.decode(await _native('info', _id(config, const []), null)));
    return value is Map && value['available'] == true && value['external'] != true;
  }

  Future<String> remember(ExternalVaultConnection config, SecretText secret, bool Function() current) async {
    final random = Random.secure();
    final material = Uint8List.fromList(List.generate(32, (_) => random.nextInt(256)));
    final key = SecretKeyData(material);
    final data = secret.exposeBytes();
    try {
      final box = await _aead.encrypt(data, secretKey: key, aad: utf8.encode(config.binding));
      if (!current()) throw const ExternalVaultFailure('closed');
      await _native('enable', _id(config, box.nonce), material);
      return base64Encode(box.concatenation());
    } finally {
      key.destroy();
      material.fillRange(0, material.length, 0);
      data.fillRange(0, data.length, 0);
    }
  }

  Future<SecretText> unlock(ExternalVaultConnection config, bool Function() current) async {
    final cipherText = config.wrapped;
    if (cipherText == null || cipherText.length > 32768) throw const ExternalVaultFailure('authentication');
    final box = _box(cipherText);
    final material = await _native('release', _id(config, box.nonce), null);
    final key = SecretKeyData(material);
    try {
      if (!current()) throw const ExternalVaultFailure('closed');
      final data = await _aead.decrypt(box, secretKey: key, aad: utf8.encode(config.binding));
      try {
        if (!current()) throw const ExternalVaultFailure('closed');
        return SecretText.fromBytes(data is Uint8List ? data : Uint8List.fromList(data));
      } finally {
        data.fillRange(0, data.length, 0);
      }
    } finally {
      key.destroy();
      material.fillRange(0, material.length, 0);
    }
  }

  Future<void> forget(ExternalVaultConnection config, String cipherText) async {
    if (cipherText.length > 32768) throw const ExternalVaultFailure('authentication');
    await _native('forget', _id(config, _box(cipherText).nonce), null);
  }
}
