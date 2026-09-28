import 'package:consolecrypt/core/models/ids.dart';
import 'package:consolecrypt/core/security/secret_text.dart';

/// Mirrors `cc_models::secret::SecretKind`.
enum SecretKind {
  password('password'),
  sshPrivateKey('ssh_private_key'),
  sshKeyPassphrase('ssh_key_passphrase'),
  apiKey('api_key'),
  token('token'),
  other('other');

  const SecretKind(this.wireName);

  final String wireName;
}

/// Mirrors `cc_models::secret::Secret`. The UI normally never holds these:
/// secret material stays in app-core. This type exists for the explicit
/// "reveal" path and for tests; its `toString()` is redacted.
final class Secret {
  const Secret({
    required this.id,
    required this.kind,
    required this.value,
    required this.createdAt,
    required this.updatedAt,
  });

  final ObjectId id;
  final SecretKind kind;
  final SecretText value;
  final DateTime createdAt;
  final DateTime updatedAt;

  @override
  String toString() => 'Secret(id: ${id.value}, kind: ${kind.wireName}, value: <redacted>)';
}
