# kdbx 2.5.0

Source: https://pub.dev/packages/kdbx/versions/2.5.0
Upstream: https://github.com/authpass/kdbx.dart
License: GPL-3.0, see LICENSE. The original library source is included here.

Local compatibility changes:

- Recognize the official Argon2id UUID and select PointyCastle's Argon2id
  implementation (type 2). Existing Argon2d uses type 0.
- Refuse excessive KDF resource parameters and unsupported Argon2 secret /
  associated-data inputs rather than silently ignoring them.
- New files default to Argon2id, 64 MiB, three iterations and one lane in
  `lib/src/kdbx_header.dart`. Existing files retain their own KDF parameters.
- Support raw 32-byte key files and validate XML v1/v2 key files, including
  the XML v2 hash in release builds (`lib/src/credentials/keyfile.dart`).
- Use the application's XML 7.x dependency; its compatibility APIs remain
  available. No XML serialization behavior was changed.
- Remove an upstream numeric debug print from `ReaderHelperDartWeb`; that
  synchronous-reader option also keeps AES KDF work in the disposable worker.
- Refuse case-colliding entry field / attachment names before the upstream case-insensitive
  map can discard one of them (`lib/src/kdbx_entry.dart`).

The application runs this library in a disposable isolate, without FFI plugins.
KDBX source files, custom fields, binaries and history stay in its object model.
