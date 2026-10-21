import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/secret_text.dart';

/// Account session of the **active synced profile** (`cc-protocol` auth
/// endpoints). Tokens stay inside app-core. New synced profiles are created
/// through `ProfileService.createSyncedProfile`; local profiles have no
/// account and always report [AuthState.signedOut] here.
abstract interface class AuthService {
  /// Emits the current state first, then changes (also on profile switch).
  Stream<AuthState> watchAuthState();

  AuthState get currentAuthState;

  /// `GET /v1/meta` — version compatibility and whether registration is open.
  Future<ServerInfo> probeServer(Uri serverUrl);

  /// Re-authenticates the active synced profile (session expired or signed
  /// out). Fails with `unsupported` for local profiles.
  Future<AccountSession> signIn({required String email, required SecretText password});

  /// Revokes the session; local data of the profile stays on the device.
  Future<void> logout();

  /// Ends every session of the account and signs this device out. On a
  /// protocol 1.7 server a new device (quarantine) keeps trusted devices
  /// signed in; the result says how many.
  Future<LogoutAllResult> logoutAllSessions();

  /// Account-level reset e-mail. Does not grant vault access (ADR-0004).
  Future<void> requestPasswordReset({required Uri serverUrl, required String email});

  Future<void> changeAccountPassword({required SecretText current, required SecretText next});
}
