import 'dart:io';

import 'package:consolecrypt/src/rust/api/app.dart' as rs_app;
import 'package:flutter/services.dart';

/// What the OS offers for user-presence checks.
enum LocalAuthKind { touchId, faceId, windowsHello, deviceCredential }

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

/// Touch ID / device-password prompt through the `consolecrypt/local_auth`
/// method channel (macOS: `LAContext` in the `cc_bridge` plugin,
/// `rust_builder/macos/Classes/CcBridgePlugin.swift`). The core never trusts
/// this directly: a success is passed as a single-use grant to its
/// authenticator right before the device-envelope unlock
/// (`vault_unlock_with_device_attested`).
///
/// Windows Hello is not wired yet (reports unsupported).
final class LocalAuthChannel {
  LocalAuthChannel._();

  static final instance = LocalAuthChannel._();

  static const _channel = MethodChannel('consolecrypt/local_auth');

  /// Queries the OS (never throws; unsupported on errors / other platforms).
  Future<LocalAuthAvailability> availability() async {
    if (!Platform.isMacOS && !Platform.isAndroid && !Platform.isIOS) return LocalAuthAvailability.unsupported;
    try {
      final r = await _channel.invokeMapMethod<String, Object?>('availability');
      final kind = switch (r?['kind']) {
        'touch_id' => LocalAuthKind.touchId,
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

  /// Shows the OS prompt with [reason]; `true` only on success.
  Future<bool> authenticate(String reason) async {
    if (!Platform.isMacOS && !Platform.isAndroid && !Platform.isIOS) return false;
    try {
      return await _channel.invokeMethod<bool>('authenticate', {'reason': reason}) ?? false;
    } on PlatformException {
      return false;
    } on MissingPluginException {
      return false;
    }
  }

  /// Queries the OS and tells the core what is available (start-up).
  Future<LocalAuthAvailability> reportToCore() async {
    final a = await availability();
    try {
      rs_app.osAuthReport(
        kind: switch (a.kind) {
          LocalAuthKind.touchId => rs_app.OsAuthKindChoice.touchId,
          LocalAuthKind.faceId => rs_app.OsAuthKindChoice.faceId,
          LocalAuthKind.windowsHello => rs_app.OsAuthKindChoice.windowsHello,
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
}
