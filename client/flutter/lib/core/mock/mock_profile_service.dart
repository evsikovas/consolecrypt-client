import 'package:consolecrypt/core/mock/mock_cloud.dart';
import 'package:consolecrypt/core/mock/mock_config.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/app_services.dart';

final class MockProfileService implements ProfileService {
  MockProfileService(this._cloud);

  final MockCloud _cloud;

  @override
  Stream<ProfilesState> watchProfiles() => _cloud.profiles.stream;

  @override
  ProfilesState get currentProfiles => _cloud.profiles.value;

  @override
  Future<Profile> createLocalProfile({required String name}) async {
    await mockDelay(_cloud.config.latency);
    final trimmed = name.trim();
    if (trimmed.isEmpty) {
      throw const AppException(
        AppErrorCode.validation,
        'Enter a profile name',
        reason: AppErrorReason.profileNameRequired,
      );
    }
    final profile = Profile(
      id: ProfileId.generate(),
      name: trimmed,
      kind: ProfileKind.local,
      createdAt: DateTime.now().toUtc(),
    );
    _cloud.lockActive();
    _cloud.records[profile.id] = MockProfileRecord(profile);
    _cloud.publishProfiles(activeId: profile.id);
    return profile;
  }

  @override
  Future<Profile> createSyncedProfile({
    required Uri serverUrl,
    required String email,
    required SecretText password,
    required String deviceName,
    required bool createAccount,
    String? name,
  }) async {
    await mockDelay(_cloud.config.latency);
    final account = authenticateAccount(_cloud, email: email, password: password, createAccount: createAccount);

    // Signing into an account that already has a profile re-activates it.
    for (final record in _cloud.records.values) {
      if (record.profile.isSynced &&
          record.profile.accountEmail == account.email &&
          record.profile.serverUrl == serverUrl) {
        _cloud.lockActive();
        record.sessionActive = true;
        _cloud.publishProfiles(activeId: record.profile.id);
        return record.profile;
      }
    }

    final vault = account.vault;
    final profile = Profile(
      id: ProfileId.generate(),
      name: (name ?? '').trim().isEmpty ? account.email : name!.trim(),
      kind: ProfileKind.synced,
      serverUrl: serverUrl,
      accountEmail: account.email,
      deviceId: DeviceId.generate(),
      vaultId: vault?.vaultId,
      createdAt: DateTime.now().toUtc(),
    );
    final record = MockProfileRecord(profile)
      ..vault = vault
      ..phase = vault == null ? VaultPhase.none : VaultPhase.locked
      ..deviceTrusted = account.trustedOnLogin
      ..sessionActive = true
      ..deviceName = deviceName.trim().isEmpty ? 'This device' : deviceName.trim()
      ..lastSyncAt = DateTime.now().toUtc();
    _cloud.lockActive();
    _cloud.records[profile.id] = record;
    _cloud.publishProfiles(activeId: profile.id);
    return profile;
  }

  @override
  Future<void> switchTo(ProfileId id) async {
    if (!_cloud.records.containsKey(id)) {
      throw const AppException(AppErrorCode.notFound, 'Profile not found', reason: AppErrorReason.profileNotFound);
    }
    _cloud.lockActive();
    _cloud.publishProfiles(activeId: id);
  }

  @override
  Future<void> rename(ProfileId id, String name) async {
    final record = _cloud.records[id];
    if (record == null) {
      throw const AppException(AppErrorCode.notFound, 'Profile not found', reason: AppErrorReason.profileNotFound);
    }
    final p = record.profile;
    record.profile = Profile(
      id: p.id,
      name: name.trim().isEmpty ? p.name : name.trim(),
      kind: p.kind,
      serverUrl: p.serverUrl,
      accountEmail: p.accountEmail,
      deviceId: p.deviceId,
      vaultId: p.vaultId,
      createdAt: p.createdAt,
      lastUsedAt: p.lastUsedAt,
    );
    _cloud.publishProfiles();
  }

  @override
  Future<void> delete(ProfileId id) async {
    await mockDelay(_cloud.config.latency);
    final wasActive = _cloud.profiles.value.activeId == id;
    if (wasActive) _cloud.lockActive();
    _cloud.records.remove(id);
    if (wasActive) {
      final next = _cloud.records.keys.firstOrNull;
      _cloud.publishProfiles(activeId: next, clearActive: next == null);
    } else {
      _cloud.publishProfiles();
    }
  }
}

/// Shared by profile creation and the enable-sync wizard.
MockAccount authenticateAccount(
  MockCloud cloud, {
  required String email,
  required SecretText password,
  required bool createAccount,
}) {
  if (cloud.offline) {
    throw const AppException(AppErrorCode.serverUnreachable, 'Server unreachable. Check the URL and your connection.');
  }
  final normalized = email.trim().toLowerCase();
  if (!RegExp(r'^[^@\s]+@[^@\s]+\.[^@\s]+$').hasMatch(normalized)) {
    throw const AppException(
      AppErrorCode.validation,
      'Enter a valid e-mail address',
      reason: AppErrorReason.invalidEmail,
    );
  }
  final pass = password.expose();
  if (createAccount) {
    if (pass.length < 10) {
      throw const AppException(
        AppErrorCode.validation,
        'Account password must be at least 10 characters',
        reason: AppErrorReason.accountPasswordTooShort,
      );
    }
    if (cloud.accounts.containsKey(normalized)) {
      throw const AppException(AppErrorCode.emailTaken, 'An account with this e-mail already exists');
    }
    return cloud.accounts[normalized] = MockAccount(email: normalized, password: pass);
  }
  final account = cloud.accounts[normalized];
  if (account == null || account.password != pass) {
    throw const AppException(AppErrorCode.invalidCredentials, 'Wrong e-mail or password');
  }
  return account;
}
