import 'package:consolecrypt/core/models/ids.dart';

/// Mirrors `cc_models::known_host::KnownHostSource`.
enum KnownHostSource {
  tofu('tofu'),
  manual('manual'),
  imported('imported'),
  certAuthority('cert_authority');

  const KnownHostSource(this.wireName);

  final String wireName;
}

/// Mirrors `cc_models::known_host::KnownHost` (synced E2EE).
final class KnownHost {
  const KnownHost({
    required this.id,
    required this.hostPattern,
    required this.keyType,
    required this.publicKey,
    required this.fingerprintSha256,
    required this.source,
    required this.addedAt,
    required this.updatedAt,
    this.revoked = false,
  });

  final ObjectId id;

  /// `host` for port 22, otherwise `[host]:port`.
  final String hostPattern;
  final String keyType;
  final String publicKey;
  final String fingerprintSha256;
  final KnownHostSource source;
  final bool revoked;
  final DateTime addedAt;
  final DateTime updatedAt;
}

/// Mirrors `cc_models::known_host::host_pattern`.
String hostPattern(String host, int port) => port == 22 ? host.toLowerCase() : '[${host.toLowerCase()}]:$port';
