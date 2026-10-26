import 'dart:async';

import 'package:consolecrypt/core/mock/mock_cloud.dart';
import 'package:consolecrypt/core/mock/mock_config.dart';
import 'package:consolecrypt/core/mock/mock_profile_service.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/core/util/value_stream.dart';

final class MockAuthService implements AuthService {
  MockAuthService(this._cloud) {
    _sub = _cloud.changed.listen((_) => _recompute());
    _recompute();
  }

  final MockCloud _cloud;
  final ValueStreamController<AuthState> _state = ValueStreamController(AuthState.signedOut);
  final ValueStreamController<SignInHints> _hints = ValueStreamController(SignInHints.empty);
  late final StreamSubscription<void> _sub;
  (ProfileId?, bool, bool)? _signature;
  var _ticket = 0;

  void _recompute() {
    final record = _cloud.activeRecord;
    final signature = (record?.profile.id, record?.sessionActive ?? false, record?.accountDisabled ?? false);
    if (signature == _signature) return;
    _signature = signature;
    if (record == null || !record.profile.isSynced) {
      _state.value = AuthState.signedOut;
      return;
    }
    final p = record.profile;
    if (record.accountDisabled) {
      _state.value = AuthState(lastServerUrl: p.serverUrl, accountDisabled: true);
      return;
    }
    if (!record.sessionActive) {
      _state.value = AuthState(lastServerUrl: p.serverUrl);
      return;
    }
    final account = _cloud.accounts[p.accountEmail];
    _state.value = AuthState(
      lastServerUrl: p.serverUrl,
      session: AccountSession(
        serverUrl: p.serverUrl!,
        email: p.accountEmail!,
        userId: account?.userId ?? '',
        deviceId: p.deviceId!,
        deviceName: record.deviceName,
        emailVerified: true,
      ),
    );
  }

  @override
  Stream<AuthState> watchAuthState() => _state.stream;

  @override
  AuthState get currentAuthState => _state.value;

  @override
  Future<ServerInfo> probeServer(Uri serverUrl) async {
    await mockDelay(_cloud.config.latency);
    if (_cloud.offline || serverUrl.host.contains('unreachable')) {
      throw const AppException(
        AppErrorCode.serverUnreachable,
        'Server unreachable. Check the URL and your connection.',
      );
    }
    final auth = _cloud.authFor(serverUrl);
    return ServerInfo(
      serverVersion: '0.2.1',
      protocolVersion: '1.10',
      registrationOpen: !auth.isEnterprise,
      auth: auth,
      sourceCodeUrl: 'https://github.com/evsikovas/consolecrypt-server',
    );
  }

  @override
  Future<AccountSession> signIn({required String email, required SecretText password}) async {
    await mockDelay(_cloud.config.latency);
    final record = _cloud.activeRecord;
    if (record == null || !record.profile.isSynced) {
      throw const AppException(
        AppErrorCode.unsupported,
        'Local profiles have no account',
        reason: AppErrorReason.localProfileNoAccount,
      );
    }
    final account = authenticateAccount(
      _cloud,
      email: email,
      password: password,
      createAccount: false,
      serverUrl: record.profile.serverUrl,
    );
    if (account.email != record.profile.accountEmail) {
      throw const AppException(
        AppErrorCode.invalidCredentials,
        'This profile belongs to a different account',
        reason: AppErrorReason.accountMismatch,
      );
    }
    record.sessionActive = true;
    _cloud.notifyChanged();
    return _state.value.session!;
  }

  @override
  Future<LogoutAllResult> logoutAllSessions() async {
    await logout();
    return const LogoutAllResult();
  }

  @override
  Future<void> logout() async {
    await mockDelay(_cloud.config.latency);
    final record = _cloud.activeRecord;
    if (record == null || !record.profile.isSynced) return;
    _cloud.lockActive();
    record.sessionActive = false;
    _cloud.notifyChanged();
  }

  @override
  Future<void> requestPasswordReset({required Uri serverUrl, required String email}) =>
      mockDelay(_cloud.config.latency);

  @override
  Future<void> changeAccountPassword({required SecretText current, required SecretText next}) async {
    await mockDelay(_cloud.config.latency);
    final account = _cloud.accounts[_cloud.activeRecord?.profile.accountEmail];
    if (account == null) throw const AppException(AppErrorCode.unsupported, 'No account');
    if (account.password != current.expose()) {
      throw const AppException(
        AppErrorCode.invalidCredentials,
        'Current password is wrong',
        reason: AppErrorReason.currentPasswordWrong,
      );
    }
    if (next.byteLength < 10) {
      throw const AppException(
        AppErrorCode.validation,
        'Account password must be at least 10 characters',
        reason: AppErrorReason.accountPasswordTooShort,
      );
    }
    account.password = next.expose();
  }

  @override
  Future<OidcChallenge> beginOidc({
    required Uri serverUrl,
    required EnterpriseSignInPurpose purpose,
    String? loginHint,
    String? displayName,
    bool allowNewDeviceIdentity = false,
  }) async {
    await mockDelay(_cloud.config.latency);
    if (!_cloud.authFor(serverUrl).isOidc) {
      throw const AppException(AppErrorCode.enterpriseAuthRequired, 'enterprise_auth_required');
    }
    final ticket = 'mock-oidc-${++_ticket}';
    mockOidcTickets[ticket] = loginHint;
    return OidcChallenge(
      ticket: ticket,
      authorizationUrl: serverUrl.replace(path: '/v1/auth/oidc/start'),
      redirectUri: Uri.parse('http://127.0.0.1:1/auth/callback'),
      loopback: true,
      expiresAt: DateTime.now().toUtc().add(const Duration(minutes: 10)),
    );
  }

  @override
  Future<void> cancelOidc(String ticket) async => mockOidcTickets.remove(ticket);

  @override
  Future<AccountSession> signInEnterprise(EnterpriseSignIn signIn) async {
    await mockDelay(_cloud.config.latency);
    final record = _cloud.activeRecord;
    if (record == null || !record.profile.isSynced) {
      if (signIn case LdapSignIn(:final password)) password.wipe();
      throw const AppException(
        AppErrorCode.unsupported,
        'Local profiles have no account',
        reason: AppErrorReason.localProfileNoAccount,
      );
    }
    final MockAccount account;
    try {
      account = authenticateEnterprise(_cloud, serverUrl: record.profile.serverUrl!, signIn: signIn);
    } on AppException catch (e) {
      if (e.code == AppErrorCode.accountDisabled) {
        record
          ..accountDisabled = true
          ..sessionActive = false;
        _cloud.notifyChanged();
      }
      rethrow;
    }
    if (account.email != record.profile.accountEmail) {
      throw const AppException(
        AppErrorCode.invalidCredentials,
        'This profile belongs to a different account',
        reason: AppErrorReason.accountMismatch,
      );
    }
    record
      ..sessionActive = true
      ..accountDisabled = false;
    _cloud.notifyChanged();
    return _state.value.session!;
  }

  @override
  Stream<SignInHints> watchSignInHints() => _hints.stream;

  @override
  SignInHints get currentSignInHints => _hints.value;

  @override
  Future<void> rememberSignIn({
    required Uri serverUrl,
    required SignInMethod method,
    String? displayName,
    String? login,
    ProfileId? profileId,
  }) async {
    _hints.value = _hints.value.remember(
      SignInHint(
        serverUrl: serverUrl,
        method: method,
        displayName: displayName,
        login: (login ?? '').trim().isEmpty ? null : login!.trim(),
        at: DateTime.now().toUtc(),
      ),
      profileId: profileId,
    );
  }

  @override
  Future<void> forgetSignInHint(Uri serverUrl) async => _hints.value = _hints.value.forgetServer(serverUrl);

  @override
  Future<void> clearAccountDisabled() async {
    final record = _cloud.activeRecord;
    if (record == null || !record.accountDisabled) return;
    record.accountDisabled = false;
    _cloud.notifyChanged();
  }

  Future<void> dispose() async {
    await _sub.cancel();
    await _state.close();
    await _hints.close();
  }
}
