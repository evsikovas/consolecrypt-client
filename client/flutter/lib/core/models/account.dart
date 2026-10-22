import 'package:consolecrypt/core/models/ids.dart';
import 'package:consolecrypt/core/models/profile.dart';
import 'package:consolecrypt/core/security/secret_text.dart';

/// Mirrors `cc_protocol::meta::ServerInfo` (`GET /v1/meta`).
final class ServerInfo {
  const ServerInfo({
    required this.serverVersion,
    required this.protocolVersion,
    this.upgradeRequired = false,
    this.registrationOpen = true,
    this.emailVerificationRequired = false,
    this.auth = ServerAuth.standard,
  });

  final String serverVersion;
  final String protocolVersion;
  final bool upgradeRequired;
  final bool registrationOpen;
  final bool emailVerificationRequired;

  /// How accounts sign in (`capabilities.auth`, protocol 1.8).
  final ServerAuth auth;
}

/// `capabilities.auth.mode`: email + account password, or only the
/// organisation's sign-in. [unknown] = a mode newer than this app.
enum ServerAuthMode { standard, enterprise, unknown }

/// `capabilities.auth.provider` in enterprise mode.
enum ServerAuthProvider { oidc, ldap, unknown }

/// Mirrors `cc_protocol::meta::AuthCapability`. Older servers omit it,
/// which means [standard].
final class ServerAuth {
  const ServerAuth({this.mode = ServerAuthMode.standard, this.provider, this.displayName});

  static const standard = ServerAuth();

  final ServerAuthMode mode;
  final ServerAuthProvider? provider;

  /// Operator-chosen name ("Acme SSO"); may be absent.
  final String? displayName;

  bool get isEnterprise => mode == ServerAuthMode.enterprise;

  bool get isOidc => isEnterprise && provider == ServerAuthProvider.oidc;

  bool get isLdap => isEnterprise && provider == ServerAuthProvider.ldap;

  /// Enterprise with a provider this app cannot use, or an unknown mode.
  bool get needsNewerApp => mode == ServerAuthMode.unknown || (isEnterprise && !isOidc && !isLdap);

  /// The sign-in method this server expects.
  SignInMethod get method => isOidc ? SignInMethod.oidc : (isLdap ? SignInMethod.ldap : SignInMethod.email);
}

/// How an account signed in (remembered per server, never a secret).
enum SignInMethod {
  /// E-mail + account password (standard servers).
  email('email'),

  /// The organisation's IdP in the system browser (OIDC).
  oidc('oidc'),

  /// The organisation's directory login + password (LDAP / Active Directory).
  ldap('ldap');

  const SignInMethod(this.wireName);

  final String wireName;

  bool get isCorporate => this != email;

  static SignInMethod? fromWire(String? name) => values.where((m) => m.wireName == name).firstOrNull;
}

/// Device-local, non-secret memory of a successful sign-in: which method
/// and which login (a hint to prefill, never a password or token).
final class SignInHint {
  const SignInHint({required this.serverUrl, required this.method, required this.at, this.displayName, this.login});

  final Uri serverUrl;
  final SignInMethod method;

  /// The server's `display_name` at that time (corporate sign-in).
  final String? displayName;

  /// E-mail or directory login used (prefill only).
  final String? login;
  final DateTime at;

  Map<String, Object?> toJson() => {
    'server_url': serverUrl.toString(),
    'method': method.wireName,
    if (displayName != null) 'display_name': displayName,
    if (login != null) 'login': login,
    'at_ms': at.millisecondsSinceEpoch,
  };

  static SignInHint? fromJson(Map<String, Object?> j) {
    final url = Uri.tryParse(j['server_url'] as String? ?? '');
    final method = SignInMethod.fromWire(j['method'] as String?);
    if (url == null || method == null) return null;
    return SignInHint(
      serverUrl: url,
      method: method,
      displayName: j['display_name'] as String?,
      login: j['login'] as String?,
      at: DateTime.fromMillisecondsSinceEpoch((j['at_ms'] as num?)?.toInt() ?? 0, isUtc: true),
    );
  }

  /// Same server regardless of a trailing slash / letter case of the host.
  static String keyFor(Uri serverUrl) {
    final s = serverUrl.toString().trim();
    final trimmed = s.endsWith('/') ? s.substring(0, s.length - 1) : s;
    return trimmed.toLowerCase();
  }
}

/// Remembered sign-ins: by server (prefill) and by profile (how this
/// device signed in).
final class SignInHints {
  const SignInHints({this.byServer = const {}, this.byProfile = const {}});

  static const empty = SignInHints();

  final Map<String, SignInHint> byServer;
  final Map<String, SignInHint> byProfile;

  SignInHint? forServer(Uri? url) => url == null ? null : byServer[SignInHint.keyFor(url)];

  SignInHint? forProfile(ProfileId? id) => id == null ? null : byProfile[id.value];

  /// Most recent first.
  List<SignInHint> get recentServers => [...byServer.values]..sort((a, b) => b.at.compareTo(a.at));

  SignInHints remember(SignInHint hint, {ProfileId? profileId}) => SignInHints(
    byServer: {...byServer, SignInHint.keyFor(hint.serverUrl): hint},
    byProfile: profileId == null ? byProfile : {...byProfile, profileId.value: hint},
  );

  SignInHints forgetServer(Uri url) =>
      SignInHints(byServer: {...byServer}..remove(SignInHint.keyFor(url)), byProfile: byProfile);

  SignInHints forgetProfile(ProfileId id) =>
      SignInHints(byServer: byServer, byProfile: {...byProfile}..remove(id.value));

  Map<String, Object?> toJson() => {
    'servers': [for (final h in byServer.values) h.toJson()],
    'profiles': {for (final e in byProfile.entries) e.key: e.value.toJson()},
  };

  static SignInHints fromJson(Map<String, Object?> j) {
    final servers = <String, SignInHint>{};
    for (final raw in j['servers'] as List? ?? const []) {
      final h = raw is Map ? SignInHint.fromJson(raw.cast<String, Object?>()) : null;
      if (h != null) servers[SignInHint.keyFor(h.serverUrl)] = h;
    }
    final profiles = <String, SignInHint>{};
    final rawProfiles = j['profiles'];
    if (rawProfiles is Map) {
      for (final e in rawProfiles.entries) {
        final h = e.value is Map ? SignInHint.fromJson((e.value as Map).cast<String, Object?>()) : null;
        if (h != null) profiles['${e.key}'] = h;
      }
    }
    return SignInHints(byServer: servers, byProfile: profiles);
  }
}

/// What a browser sign-in is for (`EnterpriseSignInPurpose`).
enum EnterpriseSignInPurpose {
  newProfile('new_profile'),
  reauthenticate('reauthenticate'),
  enableSync('enable_sync');

  const EnterpriseSignInPurpose(this.wireName);

  final String wireName;
}

/// A started browser (OIDC) sign-in: open [authorizationUrl] in the system
/// browser. With [loopback] the core receives the redirect itself.
final class OidcChallenge {
  const OidcChallenge({
    required this.ticket,
    required this.authorizationUrl,
    required this.redirectUri,
    required this.loopback,
    required this.expiresAt,
  });

  final String ticket;
  final Uri authorizationUrl;
  final Uri redirectUri;
  final bool loopback;
  final DateTime expiresAt;
}

/// Credentials of the organisation's sign-in.
sealed class EnterpriseSignIn {
  const EnterpriseSignIn();
}

/// Directory (LDAP / Active Directory) login + password. The password is
/// wiped by the service after use.
final class LdapSignIn extends EnterpriseSignIn {
  const LdapSignIn({required this.login, required this.password});

  final String login;
  final SecretText password;
}

/// A finished browser hand-off: [callbackUrl] is what the OS auth session
/// returned (mobile), `null` when the core owns the loopback redirect.
final class OidcSignIn extends EnterpriseSignIn {
  const OidcSignIn({required this.ticket, this.callbackUrl});

  final String ticket;
  final String? callbackUrl;
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
  const AuthState({this.session, this.lastServerUrl, this.accountDisabled = false});

  static const signedOut = AuthState();

  final AccountSession? session;

  /// The active synced profile's account was disabled by the administrator
  /// (enterprise sign-in): the profile stays locked until it signs in again.
  final bool accountDisabled;

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
