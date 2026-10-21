import 'dart:io';

import 'package:consolecrypt/src/rust/api/app.dart' as rs_app;
import 'package:flutter/services.dart';

/// What the OS offers for user-presence checks.
enum LocalAuthKind { fingerprint, faceId, deviceCredential }

/// Result of [LocalAuthChannel.availability].
final class LocalAuthAvailability {
  const LocalAuthAvailability({this.kind, this.notEnrolled = false});

  static const unsupported = LocalAuthAvailability();

  /// `null` = nothing usable.
  final LocalAuthKind? kind;

  /// Hardware present, nothing enrolled.
  final bool notEnrolled;

  bool get available => kind != null;
}

/// A device unlock key created inside a successful system prompt.
final class LocalAuthKey {
  const LocalAuthKey({required this.material, required this.handle});

  /// Random key material for the core (wipe after use).
  final Uint8List material;

  /// The Keystore ciphertext of [material] (not secret).
  final Uint8List handle;
}

/// The system prompt was cancelled or failed: no key.
final class LocalAuthCancelled implements Exception {
  const LocalAuthCancelled();
}

/// The Keystore key was permanently invalidated (biometrics changed).
final class LocalAuthInvalidated implements Exception {
  const LocalAuthInvalidated();
}

/// Android device unlock (S5) through the `consolecrypt/local_auth` method
/// channel (`MainActivity.kt`): an Android Keystore AES key that requires
/// user authentication for every use, invalidated by new biometric
/// enrollment, StrongBox when available. Its `Cipher` is unlocked only
/// inside a successful `BiometricPrompt` (`CryptoObject`), which then
/// encrypts (enable) or decrypts (unlock) the 32-byte key material. The core
/// never trusts this channel: it only checks the material against the
/// profile's unlock envelope.
///
/// On every other platform the core asks the OS itself (Secure Enclave,
/// Windows Hello, Secret Service); this channel is not used there.
final class LocalAuthChannel {
  LocalAuthChannel._();

  static final instance = LocalAuthChannel._();

  static const _channel = MethodChannel('consolecrypt/local_auth');

  /// Keystore alias of a profile's key.
  static String aliasFor(String profileId) => 'cc-device-unlock-$profileId';

  bool get _supported => Platform.isAndroid;

  /// Queries the OS (never throws; unsupported on errors / other platforms).
  Future<LocalAuthAvailability> availability() async {
    if (!_supported) return LocalAuthAvailability.unsupported;
    try {
      final r = await _channel.invokeMapMethod<String, Object?>('availability');
      final kind = switch (r?['kind']) {
        'fingerprint' => LocalAuthKind.fingerprint,
        'face_id' => LocalAuthKind.faceId,
        'device_credential' => LocalAuthKind.deviceCredential,
        _ => null,
      };
      return LocalAuthAvailability(kind: kind, notEnrolled: r?['not_enrolled'] == true);
    } on PlatformException {
      return LocalAuthAvailability.unsupported;
    } on MissingPluginException {
      return LocalAuthAvailability.unsupported;
    }
  }

  /// Tells the core what Android offers (start-up, resume). Display only.
  Future<LocalAuthAvailability> reportToCore() async {
    final a = await availability();
    if (!_supported) return a;
    try {
      rs_app.osAuthReport(
        kind: switch (a.kind) {
          LocalAuthKind.fingerprint => rs_app.OsAuthKindChoice.fingerprint,
          LocalAuthKind.faceId => rs_app.OsAuthKindChoice.faceId,
          LocalAuthKind.deviceCredential => rs_app.OsAuthKindChoice.deviceCredential,
          null => null,
        },
        notEnrolled: a.notEnrolled,
      );
    } on Object {
      // Core not loaded (tests): nothing to report.
    }
    return a;
  }

  /// Creates the Keystore key [alias] and, inside the prompt with [reason],
  /// encrypts fresh key material with it.
  Future<LocalAuthKey> createKey(String alias, String reason) async {
    final r = await _invoke<Map<Object?, Object?>>('createKey', {'alias': alias, 'reason': reason});
    final material = r['material'];
    final handle = r['handle'];
    if (material is! Uint8List || handle is! Uint8List) throw const LocalAuthCancelled();
    return LocalAuthKey(material: material, handle: handle);
  }

  /// Decrypts the key material in [handle] inside the prompt with [reason].
  Future<Uint8List> releaseKey(Uint8List handle, String reason) async {
    final r = await _invoke<Uint8List>('releaseKey', {'handle': handle, 'reason': reason});
    return r;
  }

  /// Deletes the Keystore key [alias] (best effort).
  Future<void> deleteKey(String alias) async {
    if (!_supported) return;
    try {
      await _channel.invokeMethod<void>('deleteKey', {'alias': alias});
    } on PlatformException {
      // Already gone.
    } on MissingPluginException {
      // Not on Android.
    }
  }

  Future<T> _invoke<T>(String method, Map<String, Object?> args) async {
    if (!_supported) throw const LocalAuthCancelled();
    try {
      final r = await _channel.invokeMethod<T>(method, args);
      if (r == null) throw const LocalAuthCancelled();
      return r;
    } on PlatformException catch (e) {
      if (e.code == 'invalidated') throw const LocalAuthInvalidated();
      throw const LocalAuthCancelled();
    } on MissingPluginException {
      throw const LocalAuthCancelled();
    }
  }
}
