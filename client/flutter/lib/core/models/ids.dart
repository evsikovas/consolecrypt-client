import 'dart:math';

final Random _rng = Random.secure();

/// RFC 4122 version-4 UUID from the platform CSPRNG (lowercase, hyphenated).
///
/// Only mocks and optimistic UI drafts use this; real ids are minted by
/// app-core (`cc_protocol::ObjectId::new`).
String generateUuidV4() {
  final bytes = List<int>.generate(16, (_) => _rng.nextInt(256));
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  final hex = bytes.map((b) => b.toRadixString(16).padLeft(2, '0')).join();
  return '${hex.substring(0, 8)}-${hex.substring(8, 12)}-'
      '${hex.substring(12, 16)}-${hex.substring(16, 20)}-${hex.substring(20)}';
}

/// `cc_protocol::ObjectId` — id of any vault object.
extension type const ObjectId(String value) implements Object {
  factory ObjectId.generate() => ObjectId(generateUuidV4());
}

/// `cc_protocol::VaultId`.
extension type const VaultId(String value) implements Object {
  factory VaultId.generate() => VaultId(generateUuidV4());
}

/// `cc_protocol::DeviceId`.
extension type const DeviceId(String value) implements Object {
  factory DeviceId.generate() => DeviceId(generateUuidV4());
}

/// `cc_protocol::DeviceRequestId`.
extension type const DeviceRequestId(String value) implements Object {
  factory DeviceRequestId.generate() => DeviceRequestId(generateUuidV4());
}

/// Local handle of a live terminal session (app-core runtime id, not synced).
extension type const TerminalSessionId(String value) implements Object {
  factory TerminalSessionId.generate() => TerminalSessionId(generateUuidV4());
}

/// Local handle of a live SFTP session.
extension type const SftpSessionId(String value) implements Object {
  factory SftpSessionId.generate() => SftpSessionId(generateUuidV4());
}

/// Local handle of a file transfer.
extension type const TransferId(String value) implements Object {
  factory TransferId.generate() => TransferId(generateUuidV4());
}
