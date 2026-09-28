import 'dart:convert';
import 'dart:typed_data';

/// A secret typed by the user (password, passphrase, private key, API key,
/// Recovery Key) on its way to the core.
///
/// * `toString()` is redacted, so a secret never leaks through string
///   interpolation, logging or error messages.
/// * Access is explicit and grep-able: [expose] / [exposeBytes].
/// * Bytes can be [wipe]d once handed to the core. Dart strings are immutable
///   and garbage-collected, so zeroisation in Dart is best effort only; the
///   Rust side wraps the value in `secrecy`/`zeroize` immediately (ADR-0101).
final class SecretText {
  SecretText(String value) : _bytes = Uint8List.fromList(utf8.encode(value));

  SecretText.fromBytes(Uint8List bytes) : _bytes = Uint8List.fromList(bytes);

  final Uint8List _bytes;
  bool _wiped = false;

  bool get isWiped => _wiped;

  bool get isEmpty => _bytes.isEmpty;

  bool get isNotEmpty => _bytes.isNotEmpty;

  /// Length in UTF-8 bytes (not characters).
  int get byteLength => _bytes.length;

  /// Returns the plaintext. Call only at the boundary to the core.
  String expose() {
    _checkNotWiped();
    return utf8.decode(_bytes);
  }

  /// Returns a copy of the plaintext bytes (caller should wipe the copy).
  Uint8List exposeBytes() {
    _checkNotWiped();
    return Uint8List.fromList(_bytes);
  }

  /// Constant-time comparison (length is not hidden).
  bool constantTimeEquals(SecretText other) {
    _checkNotWiped();
    other._checkNotWiped();
    if (_bytes.length != other._bytes.length) return false;
    var diff = 0;
    for (var i = 0; i < _bytes.length; i++) {
      diff |= _bytes[i] ^ other._bytes[i];
    }
    return diff == 0;
  }

  /// Overwrites the buffer with zeros; further access throws.
  void wipe() {
    _bytes.fillRange(0, _bytes.length, 0);
    _wiped = true;
  }

  void _checkNotWiped() {
    if (_wiped) throw StateError('SecretText was wiped');
  }

  @override
  String toString() => 'SecretText(<redacted>)';
}
