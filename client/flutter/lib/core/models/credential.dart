import 'package:consolecrypt/core/models/ids.dart';

/// Mirrors `cc_models::credential::CredentialKind`.
enum CredentialKind {
  password('password'),
  sshPrivateKey('ssh_private_key'),

  /// Private key + OpenSSH certificate.
  sshCertificate('ssh_certificate'),

  /// Keys from the OS SSH agent.
  osSshAgent('os_ssh_agent'),

  /// FIDO2 security key. Post-MVP.
  fido2('fido2'),

  /// Third-party agent socket (1Password, Secretive, …).
  externalAgent('external_agent');

  const CredentialKind(this.wireName);

  final String wireName;
}

/// Mirrors `cc_models::credential::KeyAlgorithm`.
enum KeyAlgorithm {
  ed25519('ed25519', 'Ed25519'),
  rsa2048('rsa2048', 'RSA 2048'),
  rsa3072('rsa3072', 'RSA 3072'),
  rsa4096('rsa4096', 'RSA 4096'),
  ecdsaP256('ecdsa_p256', 'ECDSA P-256'),
  ecdsaP384('ecdsa_p384', 'ECDSA P-384'),
  ecdsaP521('ecdsa_p521', 'ECDSA P-521'),
  skEd25519('sk_ed25519', 'Ed25519-SK'),
  skEcdsaP256('sk_ecdsa_p256', 'ECDSA-SK P-256');

  const KeyAlgorithm(this.wireName, this.label);

  final String wireName;
  final String label;

  /// Algorithms offered by "Generate key" (CLIENT_SPEC §7.1).
  static const generatable = [ed25519, rsa3072, rsa4096];
}

/// Mirrors `cc_models::credential::Credential` — *how* to authenticate.
/// Holds no secret material: passwords / private keys / passphrases live in
/// separate `Secret` objects referenced by id and never enter the UI unless
/// the user explicitly reveals them.
final class Credential {
  const Credential({
    required this.id,
    required this.name,
    required this.kind,
    required this.createdAt,
    required this.updatedAt,
    this.username,
    this.secretId,
    this.passphraseSecretId,
    this.keyEncrypted = false,
    this.keyAlgorithm,
    this.publicKey,
    this.certificate,
    this.fingerprint,
    this.agentPath,
  });

  final ObjectId id;
  final String name;
  final CredentialKind kind;

  /// Optional username override.
  final String? username;

  /// `Secret` holding the password or the OpenSSH private key.
  final ObjectId? secretId;

  /// `Secret` holding the key passphrase ("Remember SSH key passphrase").
  final ObjectId? passphraseSecretId;

  /// Whether the stored private key is itself passphrase-protected.
  final bool keyEncrypted;
  final KeyAlgorithm? keyAlgorithm;

  /// OpenSSH public key line. Not secret.
  final String? publicKey;

  /// OpenSSH certificate line. Not secret.
  final String? certificate;

  /// `SHA256:…` fingerprint for display.
  final String? fingerprint;

  /// For `externalAgent`: socket path / pipe name.
  final String? agentPath;
  final DateTime createdAt;
  final DateTime updatedAt;

  bool get remembersPassphrase => passphraseSecretId != null;

  Credential copyWith({String? name, String? username, bool clearUsername = false}) => Credential(
    id: id,
    name: name ?? this.name,
    kind: kind,
    username: clearUsername ? null : (username ?? this.username),
    secretId: secretId,
    passphraseSecretId: passphraseSecretId,
    keyEncrypted: keyEncrypted,
    keyAlgorithm: keyAlgorithm,
    publicKey: publicKey,
    certificate: certificate,
    fingerprint: fingerprint,
    agentPath: agentPath,
    createdAt: createdAt,
    updatedAt: DateTime.now().toUtc(),
  );
}

/// Result of parsing a pasted private key before import (ssh-core parses;
/// the key's own passphrase protection is never removed).
final class KeyInspection {
  const KeyInspection({
    required this.valid,
    this.algorithm,
    this.encrypted = false,
    this.fingerprint,
    this.publicKey,
    this.error,
  });

  final bool valid;
  final KeyAlgorithm? algorithm;

  /// The key is passphrase-protected (OpenSSH `bcrypt` KDF / PEM encryption).
  final bool encrypted;
  final String? fingerprint;
  final String? publicKey;
  final String? error;
}
