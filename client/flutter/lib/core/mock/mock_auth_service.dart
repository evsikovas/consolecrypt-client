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
  late final StreamSubscription<void> _sub;
  (ProfileId?, bool)? _signature;

  void _recompute() {
    final record = _cloud.activeRecord;
    final signature = (record?.profile.id, record?.sessionActive ?? false);
    if (signature == _signature) return;
    _signature = signature;
    if (record == null || !record.profile.isSynced) {
      _state.value = AuthState.signedOut;
      return;
    }
    final p = record.profile;
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
    return const ServerInfo(serverVersion: '0.1.0 (mock)', protocolVersion: '1.0');
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
    final account = authenticateAccount(_cloud, email: email, password: password, createAccount: false);
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

  Future<void> dispose() async {
    await _sub.cancel();
    await _state.close();
  }
}
