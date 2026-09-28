import 'package:consolecrypt/core/models/ids.dart';

/// Mirrors `cc_protocol::meta::ServerInfo` (`GET /v1/meta`).
final class ServerInfo {
  const ServerInfo({
    required this.serverVersion,
    required this.protocolVersion,
    this.upgradeRequired = false,
    this.registrationOpen = true,
    this.emailVerificationRequired = false,
  });

  final String serverVersion;
  final String protocolVersion;
  final bool upgradeRequired;
  final bool registrationOpen;
  final bool emailVerificationRequired;
}

/// The signed-in account on this device (subset of `AccountInfo` plus the
/// self-hosted server URL). Tokens never reach Dart: app-core owns them.
final class AccountSession {
  const AccountSession({
    required this.serverUrl,
    required this.email,
    required this.userId,
    required this.deviceId,
    required this.deviceName,
    this.emailVerified = false,
  });

  final Uri serverUrl;
  final String email;
  final String userId;
  final DeviceId deviceId;
  final String deviceName;
  final bool emailVerified;
}

/// Authentication state stream item.
final class AuthState {
  const AuthState({this.session, this.lastServerUrl});

  static const signedOut = AuthState();

  final AccountSession? session;

  /// Pre-fills the server field after sign-out (never a hard-coded default).
  final Uri? lastServerUrl;

  bool get isSignedIn => session != null;
}

/// Why [validateServerUrl] rejected a server URL; the UI turns it into
/// localized text.
enum ServerUrlProblem {
  /// Nothing was entered.
  empty,

  /// Not a full URL (scheme and host are required).
  notAUrl,

  /// Neither `https` nor `http` to a loopback host.
  insecure,
}

/// Validates a user-entered self-hosted server URL. There is deliberately
/// no default endpoint (ADR-0005). Plain `http` is only allowed for
/// loopback (local development servers). Returns `null` when valid.
ServerUrlProblem? validateServerUrl(String input) {
  final text = input.trim();
  if (text.isEmpty) return ServerUrlProblem.empty;
  final uri = Uri.tryParse(text);
  if (uri == null || !uri.hasScheme || uri.host.isEmpty) {
    return ServerUrlProblem.notAUrl;
  }
  if (uri.scheme == 'https') return null;
  if (uri.scheme == 'http' && const {'localhost', '127.0.0.1', '::1', '[::1]'}.contains(uri.host)) {
    return null;
  }
  return ServerUrlProblem.insecure;
}
