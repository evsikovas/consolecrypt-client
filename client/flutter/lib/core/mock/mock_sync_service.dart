import 'dart:async';

import 'package:consolecrypt/core/mock/mock_cloud.dart';
import 'package:consolecrypt/core/mock/mock_config.dart';
import 'package:consolecrypt/core/mock/mock_profile_service.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/core/util/value_stream.dart';

/// Simulated outbox worker: local mutations of synced profiles become
/// pending changes that are "pushed" shortly afterwards unless offline.
final class MockSyncService implements SyncService {
  MockSyncService(this._cloud) {
    _changedSub = _cloud.changed.listen((_) => _recompute());
    _mutationSub = _cloud.mutations.listen((_) => _onMutation());
    _recompute();
  }

  final MockCloud _cloud;
  final ValueStreamController<SyncStatus> _status = ValueStreamController(SyncStatus.paused);
  late final StreamSubscription<void> _changedSub;
  late final StreamSubscription<void> _mutationSub;
  Object? _signature;
  ProfileId? _unlockedProfile;
  bool _disposed = false;

  MockConfig get _config => _cloud.config;

  bool _canSync(MockProfileRecord r) => r.profile.isSynced && r.sessionActive && r.phase == VaultPhase.unlocked;

  void _recompute() {
    final r = _cloud.activeRecord;
    final signature = (
      r?.profile.id,
      r?.profile.kind,
      r?.phase,
      r?.sessionActive,
      r?.pendingChanges,
      r?.syncing,
      r?.lastSyncAt,
      r?.issues.length,
      _cloud.offline,
    );
    if (signature == _signature) return;
    _signature = signature;

    // Initial sync right after a synced vault gets unlocked.
    final unlocked = r != null && _canSync(r) ? r.profile.id : null;
    if (unlocked != null && unlocked != _unlockedProfile) {
      scheduleMicrotask(() => unawaited(_sync()));
    }
    _unlockedProfile = unlocked;

    if (r == null) {
      _status.value = SyncStatus.paused;
    } else if (r.profile.isLocal) {
      _status.value = SyncStatus.localOnly;
    } else {
      final base = SyncStatus(
        state: SyncState.idle,
        pendingChanges: r.pendingChanges,
        lastSyncAt: r.lastSyncAt,
        lastServerSequence: r.serverSequence,
        issues: List.unmodifiable(r.issues),
      );
      if (!_canSync(r)) {
        _status.value = base.copyWith(state: SyncState.paused);
      } else if (_cloud.offline) {
        _status.value = base.copyWith(
          state: SyncState.offline,
          nextRetryAt: DateTime.now().add(const Duration(seconds: 30)),
        );
      } else if (r.syncing) {
        _status.value = base.copyWith(state: SyncState.syncing);
      } else {
        _status.value = base;
      }
    }
  }

  void _onMutation() {
    final r = _cloud.activeRecord;
    if (r == null || !r.profile.isSynced) return; // local profiles: no outbox
    r.pendingChanges++;
    _cloud.notifyChanged();
    unawaited(_sync());
  }

  Future<void> _sync() async {
    final r = _cloud.activeRecord;
    if (_disposed || r == null || !_canSync(r) || r.syncing) return;
    if (_cloud.offline) {
      r.issues
        ..clear()
        ..add(SyncIssue(at: DateTime.now(), message: 'Server unreachable — changes are queued locally'));
      _cloud.notifyChanged();
      return;
    }
    r.syncing = true;
    _cloud.notifyChanged();
    await mockDelay(_config.latency * 2);
    if (_disposed) return;
    r.syncing = false;
    if (_cloud.offline) {
      _cloud.notifyChanged();
      return;
    }
    r
      ..serverSequence += r.pendingChanges + 1
      ..pendingChanges = 0
      ..lastSyncAt = DateTime.now().toUtc()
      ..issues.clear();
    _cloud.notifyChanged();
  }

  /// Developer control.
  void setOffline(bool offline) {
    _cloud.offline = offline;
    _cloud.notifyChanged();
    if (!offline) unawaited(_sync());
  }

  @override
  Stream<SyncStatus> watchStatus() => _status.stream;

  @override
  SyncStatus get currentStatus => _status.value;

  @override
  Future<void> syncNow() => _sync();

  @override
  Stream<EnableSyncProgress> enableSync({
    required Uri serverUrl,
    required String email,
    required SecretText password,
    required String deviceName,
    required bool createAccount,
  }) async* {
    final r = _cloud.activeRecord;
    final keys = r?.vault;
    if (r == null || keys == null || !r.profile.isLocal || r.phase != VaultPhase.unlocked) {
      yield const EnableSyncProgress(EnableSyncStep.failed, message: 'Unlock a local profile first');
      return;
    }
    yield const EnableSyncProgress(EnableSyncStep.authenticating);
    await mockDelay(_config.latency);
    final MockAccount account;
    try {
      account = authenticateAccount(_cloud, email: email, password: password, createAccount: createAccount);
    } on AppException catch (e) {
      yield EnableSyncProgress(EnableSyncStep.failed, message: e.message);
      return;
    }
    final existing = account.vault;
    if (existing != null && existing.vaultId != keys.vaultId) {
      yield const EnableSyncProgress(
        EnableSyncStep.failed,
        message:
            'This account already holds a different vault. Use another account, '
            'or add it as a separate synced profile.',
      );
      return;
    }
    yield EnableSyncProgress(existing == null ? EnableSyncStep.creatingRemoteVault : EnableSyncStep.reconnecting);
    await mockDelay(_config.latency);
    account.vault = keys;

    final total = _cloud.dataFor(keys.vaultId, keys.name).objectCount;
    for (var uploaded = 0; uploaded < total; uploaded += 8) {
      yield EnableSyncProgress(EnableSyncStep.uploading, uploaded: uploaded, total: total);
      await mockDelay(_config.streamStep * 3);
    }
    yield EnableSyncProgress(EnableSyncStep.uploading, uploaded: total, total: total);
    yield const EnableSyncProgress(EnableSyncStep.finishing);
    await mockDelay(_config.latency);

    final p = r.profile;
    r
      ..profile = Profile(
        id: p.id,
        name: p.name,
        kind: ProfileKind.synced,
        serverUrl: serverUrl,
        accountEmail: account.email,
        deviceId: DeviceId.generate(),
        vaultId: keys.vaultId,
        createdAt: p.createdAt,
        lastUsedAt: DateTime.now().toUtc(),
      )
      ..sessionActive = true
      ..deviceTrusted = true
      ..deviceName = deviceName.trim().isEmpty ? 'This device' : deviceName.trim()
      ..serverSequence = total
      ..lastSyncAt = DateTime.now().toUtc();
    _cloud.publishProfiles();
    yield const EnableSyncProgress(EnableSyncStep.done);
  }

  @override
  Future<void> disconnect({required bool revokeThisDevice}) async {
    final r = _cloud.activeRecord;
    if (r == null || !r.profile.isSynced) {
      throw const AppException(
        AppErrorCode.unsupported,
        'This profile is not synced',
        reason: AppErrorReason.syncedProfileRequired,
      );
    }
    await mockDelay(_config.latency);
    final p = r.profile;
    r
      ..profile = Profile(
        id: p.id,
        name: p.name,
        kind: ProfileKind.local,
        vaultId: p.vaultId,
        createdAt: p.createdAt,
        lastUsedAt: p.lastUsedAt,
      )
      ..sessionActive = false
      ..pendingChanges = 0
      ..issues.clear();
    _cloud.publishProfiles();
  }

  Future<void> dispose() async {
    _disposed = true;
    await _changedSub.cancel();
    await _mutationSub.cancel();
    await _status.close();
  }
}
