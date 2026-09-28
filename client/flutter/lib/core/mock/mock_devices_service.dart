import 'dart:async';
import 'dart:math';

import 'package:consolecrypt/core/mock/mock_cloud.dart';
import 'package:consolecrypt/core/mock/mock_config.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/core/util/value_stream.dart';

final class _AccountDevices {
  final List<DeviceInfo> devices = [];
  final List<DeviceTrustRequest> pending = [];
}

/// Devices of the active synced profile's account (local profiles: none).
final class MockDevicesService implements DevicesService {
  MockDevicesService(this._cloud) {
    _sub = _cloud.changed.listen((_) => _recompute());
    _recompute();
  }

  final MockCloud _cloud;
  final Random _rng = Random.secure();
  final ValueStreamController<DevicesSnapshot> _snapshot = ValueStreamController(DevicesSnapshot.empty);
  final Map<String, _AccountDevices> _byAccount = {};
  final Map<DeviceRequestId, VerificationCode> _codes = {};
  final Map<DeviceId, VerificationCode> _ownCodes = {};
  late final StreamSubscription<void> _sub;
  Object? _signature;
  int _version = 0;

  VerificationCode _randomCode() => VerificationCode.fromDigest(List<int>.generate(32, (_) => _rng.nextInt(256)));

  _AccountDevices? _account() {
    final r = _cloud.activeRecord;
    if (r == null || !r.profile.isSynced || r.profile.accountEmail == null) return null;
    return _byAccount.putIfAbsent(r.profile.accountEmail!, () => _seed(r));
  }

  _AccountDevices _seed(MockProfileRecord r) {
    final acc = _AccountDevices();
    final vault = r.vault?.vaultId;
    final isDemo = r.profile.accountEmail == MockCloud.demoEmail || r.profile.accountEmail == MockCloud.newDeviceEmail;
    if (!isDemo || vault == null || !_cloud.config.seedDemoData) return acc;
    final now = DateTime.now().toUtc();
    acc.devices.addAll([
      DeviceInfo(
        deviceId: DeviceId.generate(),
        name: 'Office PC',
        platform: DevicePlatform.windows,
        status: DeviceStatus.active,
        trustedVaults: [vault],
        createdAt: now.subtract(const Duration(days: 200)),
        lastSeenAt: now.subtract(const Duration(days: 2, hours: 3)),
      ),
      DeviceInfo(
        deviceId: DeviceId.generate(),
        name: 'Old laptop',
        platform: DevicePlatform.macos,
        status: DeviceStatus.revoked,
        trustedVaults: const [],
        createdAt: now.subtract(const Duration(days: 400)),
        lastSeenAt: now.subtract(const Duration(days: 45)),
        revokedAt: now.subtract(const Duration(days: 40)),
      ),
    ]);
    if (r.profile.accountEmail == MockCloud.demoEmail) {
      final newDevice = DeviceInfo(
        deviceId: DeviceId.generate(),
        name: 'MacBook Air',
        platform: DevicePlatform.macos,
        status: DeviceStatus.active,
        trustedVaults: const [],
        createdAt: now.subtract(const Duration(minutes: 3)),
        lastSeenAt: now.subtract(const Duration(minutes: 1)),
      );
      final request = DeviceTrustRequest(
        requestId: DeviceRequestId.generate(),
        device: newDevice,
        vaultIds: [vault],
        status: DeviceRequestStatus.pending,
        createdAt: now.subtract(const Duration(minutes: 3)),
        expiresAt: now.add(const Duration(hours: 23, minutes: 57)),
      );
      acc
        ..devices.add(newDevice)
        ..pending.add(request);
    }
    return acc;
  }

  void _recompute() {
    final r = _cloud.activeRecord;
    final signature = (r?.profile.id, r?.profile.kind, r?.deviceTrusted, r?.vault?.vaultId, r?.deviceName, _version);
    if (signature == _signature) return;
    _signature = signature;
    final acc = _account();
    if (acc == null || r == null) {
      _snapshot.value = DevicesSnapshot.empty;
      return;
    }
    final vault = r.vault?.vaultId;
    final current = DeviceInfo(
      deviceId: r.profile.deviceId ?? const DeviceId('00000000-0000-4000-8000-000000000000'),
      name: r.deviceName,
      platform: DevicePlatform.macos,
      status: DeviceStatus.active,
      trustedVaults: [if (r.deviceTrusted && vault != null) vault],
      createdAt: r.profile.createdAt,
      lastSeenAt: DateTime.now().toUtc(),
      isCurrent: true,
    );
    _snapshot.value = DevicesSnapshot(
      devices: [current, ...acc.devices.where((d) => d.deviceId != current.deviceId)],
      pendingRequests: List.unmodifiable(acc.pending),
    );
  }

  void _changed() {
    _version++;
    _recompute();
  }

  _AccountDevices _requireAccount() {
    final acc = _account();
    if (acc == null) {
      throw const AppException(
        AppErrorCode.unsupported,
        'Devices apply to synced profiles only',
        reason: AppErrorReason.syncedProfileRequired,
      );
    }
    return acc;
  }

  @override
  Stream<DevicesSnapshot> watchDevices() => _snapshot.stream;

  /// Synchronous snapshot (tests / developer tooling).
  DevicesSnapshot get currentSnapshot => _snapshot.value;

  @override
  Future<void> refresh() async {
    await mockDelay(_cloud.config.latency);
    _changed();
  }

  @override
  Future<VerificationCode> verificationCodeFor(DeviceRequestId requestId) async {
    final acc = _requireAccount();
    if (!acc.pending.any((p) => p.requestId == requestId)) {
      throw const AppException(
        AppErrorCode.notFound,
        'The request no longer exists',
        reason: AppErrorReason.requestNotFound,
      );
    }
    await mockDelay(_cloud.config.latency);
    return _codes.putIfAbsent(requestId, _randomCode);
  }

  /// Mock-only: the code the *new* device displays for [requestId].
  VerificationCode? debugCodeShownOnNewDevice(DeviceRequestId requestId) => _codes[requestId];

  @override
  Future<VerificationCode> currentDeviceVerificationCode() async {
    final id = _cloud.activeRecord?.profile.deviceId;
    if (id == null) {
      throw const AppException(
        AppErrorCode.unsupported,
        'No device registered',
        reason: AppErrorReason.noDeviceRegistered,
      );
    }
    return _ownCodes.putIfAbsent(id, _randomCode);
  }

  @override
  Future<void> approve(DeviceRequestId requestId, {required VerificationCode confirmedCode}) async {
    final acc = _requireAccount();
    final r = _cloud.activeRecord!;
    if (r.phase != VaultPhase.unlocked || !r.deviceTrusted) {
      throw const AppException(
        AppErrorCode.deviceNotTrusted,
        'Only an unlocked trusted device can approve',
        reason: AppErrorReason.trustedDeviceRequired,
      );
    }
    final request = acc.pending.where((p) => p.requestId == requestId).firstOrNull;
    if (request == null) {
      throw const AppException(
        AppErrorCode.notFound,
        'The request no longer exists',
        reason: AppErrorReason.requestNotFound,
      );
    }
    if (request.isExpired()) throw const AppException(AppErrorCode.requestExpired, 'The request expired');
    final expected = _codes[requestId];
    if (expected == null || !expected.matches(confirmedCode)) {
      throw const AppException(AppErrorCode.verificationMismatch, 'Verification code mismatch — approval refused');
    }
    await mockDelay(_cloud.config.latency);
    acc.pending.remove(request);
    final i = acc.devices.indexWhere((d) => d.deviceId == request.device.deviceId);
    final approved = DeviceInfo(
      deviceId: request.device.deviceId,
      name: request.device.name,
      platform: request.device.platform,
      status: DeviceStatus.active,
      trustedVaults: request.vaultIds,
      createdAt: request.device.createdAt,
      lastSeenAt: DateTime.now().toUtc(),
    );
    if (i >= 0) {
      acc.devices[i] = approved;
    } else {
      acc.devices.add(approved);
    }
    _changed();
  }

  @override
  Future<void> reject(DeviceRequestId requestId) async {
    final acc = _requireAccount();
    await mockDelay(_cloud.config.latency);
    acc.pending.removeWhere((p) => p.requestId == requestId);
    _changed();
  }

  @override
  Future<void> revoke(DeviceId deviceId, {String? reason}) async {
    final acc = _requireAccount();
    if (deviceId == _cloud.activeRecord?.profile.deviceId) {
      throw const AppException(
        AppErrorCode.validation,
        'To remove this device, sign out or disconnect sync in Settings',
      );
    }
    await mockDelay(_cloud.config.latency);
    final i = acc.devices.indexWhere((d) => d.deviceId == deviceId);
    if (i < 0) {
      throw const AppException(AppErrorCode.notFound, 'Device not found', reason: AppErrorReason.deviceNotFound);
    }
    final d = acc.devices[i];
    acc.devices[i] = DeviceInfo(
      deviceId: d.deviceId,
      name: d.name,
      platform: d.platform,
      status: DeviceStatus.revoked,
      trustedVaults: const [],
      createdAt: d.createdAt,
      lastSeenAt: d.lastSeenAt,
      revokedAt: DateTime.now().toUtc(),
    );
    acc.pending.removeWhere((p) => p.device.deviceId == deviceId);
    _changed();
  }

  @override
  Future<void> rename(DeviceId deviceId, String name) async {
    final acc = _requireAccount();
    if (name.trim().isEmpty) {
      throw const AppException(AppErrorCode.validation, 'Enter a name', reason: AppErrorReason.nameRequired);
    }
    await mockDelay(_cloud.config.latency);
    final r = _cloud.activeRecord!;
    if (deviceId == r.profile.deviceId) {
      r.deviceName = name.trim();
    } else {
      final i = acc.devices.indexWhere((d) => d.deviceId == deviceId);
      if (i < 0) {
        throw const AppException(AppErrorCode.notFound, 'Device not found', reason: AppErrorReason.deviceNotFound);
      }
      final d = acc.devices[i];
      acc.devices[i] = DeviceInfo(
        deviceId: d.deviceId,
        name: name.trim(),
        platform: d.platform,
        status: d.status,
        trustedVaults: d.trustedVaults,
        createdAt: d.createdAt,
        lastSeenAt: d.lastSeenAt,
        revokedAt: d.revokedAt,
      );
    }
    _changed();
  }

  Future<void> dispose() async {
    await _sub.cancel();
    await _snapshot.close();
  }
}
